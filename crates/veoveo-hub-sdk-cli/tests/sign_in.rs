//! A person's sign-in against a local fake authorization server that keeps the Hub's
//! rules: exact redirect matching, S256 PKCE, single-use codes, refresh-token rotation
//! and revocation of the whole family when a spent refresh token comes back.

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    process::Stdio,
    sync::{Arc, Mutex},
    time::Duration,
};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt as _;

use axum::{
    Form, Json, Router,
    extract::{Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_json::json;
use sha2::{Digest as _, Sha256};
use tokio::io::{AsyncBufReadExt as _, BufReader};
use url::Url;
use veoveo_hub_sdk::{HubProfileId, client::BearerSource};
use veoveo_hub_sdk_cli::{
    grant::{GrantBearer, GrantStore, LOGIN},
    hub::{HubAccess, HubAuthentication},
    sign_in::{self, Revocation, SignIn, SignedIn, SignedOut},
};
use veoveo_types::{OAuthClientId, ScopeName};

#[derive(Clone, Copy, PartialEq, Eq)]
enum RefreshMode {
    Rotate,
    Refuse,
    /// Rotates, then answers with a server failure, as when the answer is lost.
    ConsumeWithoutAnswer,
}

#[derive(Clone)]
struct Config {
    redirect_uri: String,
    code_lifetime: u64,
    refresh_lifetime: u64,
    refresh_delay: Duration,
    refresh_mode: RefreshMode,
    authorize_error: Option<&'static str>,
}

impl Config {
    fn new(port: u16) -> Self {
        Self {
            redirect_uri: format!("http://127.0.0.1:{port}/callback"),
            code_lifetime: 900,
            refresh_lifetime: 900,
            refresh_delay: Duration::ZERO,
            refresh_mode: RefreshMode::Rotate,
            authorize_error: None,
        }
    }
}

struct Pending {
    challenge: String,
    redirect_uri: String,
}

type Pairs = Vec<(String, String)>;

#[derive(Default)]
struct Ledger {
    codes: HashMap<String, Pending>,
    authorizations: Vec<HashMap<String, String>>,
    code_exchanges: Vec<HashMap<String, String>>,
    refreshes: Vec<HashMap<String, String>>,
    revocations: Vec<HashMap<String, String>>,
    pkce_verified: u32,
    presentations: HashMap<String, u32>,
    current_refresh: Option<String>,
    consumed: HashSet<String>,
    replays: u32,
    family_revoked: bool,
    user_agents: Vec<String>,
    codes_issued: u32,
    tokens_issued: u32,
}

struct Shared {
    base: Url,
    config: Config,
    ledger: Mutex<Ledger>,
}

struct Fake {
    base: Url,
    shared: Arc<Shared>,
    server: tokio::task::JoinHandle<()>,
}

impl Fake {
    async fn start(config: Config) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
        let shared = Arc::new(Shared {
            base: base.clone(),
            config,
            ledger: Mutex::default(),
        });
        let app = Router::new()
            .route(
                "/.well-known/oauth-protected-resource/mcp/local",
                get(protected_resource),
            )
            .route(
                "/.well-known/oauth-authorization-server/oauth",
                get(server_metadata),
            )
            .route("/oauth/authorize", get(authorize))
            .route("/oauth/token", post(token))
            .route("/oauth/revoke", post(revoke))
            .with_state(shared.clone());
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self {
            base,
            shared,
            server,
        }
    }

    fn ledger(&self) -> std::sync::MutexGuard<'_, Ledger> {
        self.shared.ledger.lock().unwrap()
    }
}

fn record(shared: &Shared, headers: &HeaderMap) {
    let agent = headers
        .get(header::USER_AGENT)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    shared.ledger.lock().unwrap().user_agents.push(agent);
}

fn map(form: Pairs) -> HashMap<String, String> {
    form.into_iter().collect()
}

fn refusal(error: &str) -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({ "error": error }))).into_response()
}

async fn protected_resource(State(shared): State<Arc<Shared>>, headers: HeaderMap) -> Response {
    record(&shared, &headers);
    Json(json!({
        "resource": shared.base.join("mcp/local").unwrap(),
        "authorization_servers": [shared.base.join("oauth").unwrap()],
        "scopes_supported": ["mcp"],
        "bearer_methods_supported": ["header"],
    }))
    .into_response()
}

async fn server_metadata(State(shared): State<Arc<Shared>>, headers: HeaderMap) -> Response {
    record(&shared, &headers);
    let base = &shared.base;
    Json(json!({
        "issuer": base.join("oauth").unwrap(),
        "authorization_endpoint": base.join("oauth/authorize").unwrap(),
        "token_endpoint": base.join("oauth/token").unwrap(),
        "revocation_endpoint": base.join("oauth/revoke").unwrap(),
        "response_types_supported": ["code"],
        "grant_types_supported": ["authorization_code", "refresh_token"],
        "code_challenge_methods_supported": ["S256"],
        "token_endpoint_auth_methods_supported": ["none"],
    }))
    .into_response()
}

/// The browser's request, so its User-Agent is not the CLI's.
async fn authorize(State(shared): State<Arc<Shared>>, Query(query): Query<Pairs>) -> Response {
    let query = map(query);
    let mut ledger = shared.ledger.lock().unwrap();
    ledger.authorizations.push(query.clone());
    let config = &shared.config;
    if query.get("client_id").map(String::as_str) != Some("veoveo-hub-sdk-cli")
        || query.get("redirect_uri") != Some(&config.redirect_uri)
        || query.get("response_type").map(String::as_str) != Some("code")
        || query.get("code_challenge_method").map(String::as_str) != Some("S256")
        || query.get("resource").map(String::as_str)
            != Some(shared.base.join("mcp/local").unwrap().as_str())
    {
        return refusal("invalid_request");
    }
    let mut redirect = Url::parse(&config.redirect_uri).unwrap();
    if let Some(error) = config.authorize_error {
        redirect.query_pairs_mut().append_pair("error", error);
    } else {
        ledger.codes_issued += 1;
        let code = format!("code-{}", ledger.codes_issued);
        ledger.codes.insert(
            code.clone(),
            Pending {
                challenge: query["code_challenge"].clone(),
                redirect_uri: query["redirect_uri"].clone(),
            },
        );
        redirect.query_pairs_mut().append_pair("code", &code);
    }
    if let Some(state) = query.get("state") {
        redirect.query_pairs_mut().append_pair("state", state);
    }
    (
        StatusCode::FOUND,
        [(header::LOCATION, redirect.to_string())],
    )
        .into_response()
}

async fn token(
    State(shared): State<Arc<Shared>>,
    headers: HeaderMap,
    Form(form): Form<Pairs>,
) -> Response {
    record(&shared, &headers);
    let form = map(form);
    match form.get("grant_type").map(String::as_str) {
        Some("authorization_code") => exchange_code(&shared, form),
        Some("refresh_token") => refresh(&shared, form).await,
        _ => refusal("unsupported_grant_type"),
    }
}

fn issue(ledger: &mut Ledger, lifetime: u64) -> Response {
    ledger.tokens_issued += 1;
    let refresh = format!("rt-{}", ledger.tokens_issued);
    ledger.current_refresh = Some(refresh.clone());
    Json(json!({
        "access_token": format!("at-{}", ledger.tokens_issued),
        "token_type": "Bearer",
        "expires_in": lifetime,
        "scope": "mcp",
        "refresh_token": refresh,
        "refresh_token_expires_in": 604_800,
    }))
    .into_response()
}

fn exchange_code(shared: &Shared, form: HashMap<String, String>) -> Response {
    let mut ledger = shared.ledger.lock().unwrap();
    ledger.code_exchanges.push(form.clone());
    // The Hub refuses a scope in the code exchange.
    if form.contains_key("scope") {
        return refusal("invalid_scope");
    }
    let Some(pending) = form.get("code").and_then(|code| ledger.codes.remove(code)) else {
        return refusal("invalid_grant");
    };
    let verifier = form.get("code_verifier").cloned().unwrap_or_default();
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    if form.get("redirect_uri") != Some(&pending.redirect_uri)
        || form.get("client_id").map(String::as_str) != Some("veoveo-hub-sdk-cli")
        || challenge != pending.challenge
    {
        return refusal("invalid_grant");
    }
    ledger.pkce_verified += 1;
    issue(&mut ledger, shared.config.code_lifetime)
}

async fn refresh(shared: &Shared, form: HashMap<String, String>) -> Response {
    let presented = form.get("refresh_token").cloned().unwrap_or_default();
    {
        let mut ledger = shared.ledger.lock().unwrap();
        ledger.refreshes.push(form.clone());
        *ledger.presentations.entry(presented.clone()).or_default() += 1;
        if ledger.consumed.contains(&presented) {
            ledger.replays += 1;
            ledger.family_revoked = true;
            return refusal("invalid_grant");
        }
        if ledger.family_revoked
            || ledger.current_refresh.as_ref() != Some(&presented)
            || form.contains_key("scope")
            || form.get("client_id").map(String::as_str) != Some("veoveo-hub-sdk-cli")
            || form.get("resource").map(String::as_str)
                != Some(shared.base.join("mcp/local").unwrap().as_str())
        {
            return refusal("invalid_grant");
        }
        ledger.consumed.insert(presented);
    }
    tokio::time::sleep(shared.config.refresh_delay).await;
    let mut ledger = shared.ledger.lock().unwrap();
    match shared.config.refresh_mode {
        RefreshMode::Rotate => issue(&mut ledger, shared.config.refresh_lifetime),
        RefreshMode::Refuse => refusal("invalid_grant"),
        RefreshMode::ConsumeWithoutAnswer => {
            let _ = issue(&mut ledger, shared.config.refresh_lifetime);
            (StatusCode::BAD_GATEWAY, "upstream failure").into_response()
        }
    }
}

async fn revoke(
    State(shared): State<Arc<Shared>>,
    headers: HeaderMap,
    Form(form): Form<Pairs>,
) -> Response {
    record(&shared, &headers);
    let form = map(form);
    let mut ledger = shared.ledger.lock().unwrap();
    if ledger.current_refresh.as_ref() == form.get("token") {
        ledger.family_revoked = true;
    }
    ledger.revocations.push(form);
    StatusCode::OK.into_response()
}

/// A directory removed when the test ends.
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "veoveo-cli-sign-in-{}",
            uuid::Uuid::new_v4().simple()
        ));
        Self(path)
    }

    fn grants(&self) -> GrantStore {
        GrantStore::new(self.0.join("grants"))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn profile() -> HubProfileId {
    HubProfileId::parse("local").unwrap()
}

fn access(fake: &Fake, grants: &GrantStore) -> HubAccess {
    let _ = rustls::crypto::ring::default_provider().install_default();
    HubAccess {
        hub: fake.base.clone(),
        connect: None,
        authentication: HubAuthentication::StoredGrant {
            grants: grants.clone(),
        },
        profile: profile(),
    }
}

fn request(fake: &Fake) -> SignIn {
    SignIn {
        client: OAuthClientId::parse("veoveo-hub-sdk-cli").unwrap(),
        redirect_uri: Url::parse(&fake.shared.config.redirect_uri).unwrap(),
        scope: ScopeName::parse("mcp").unwrap(),
        timeout: Duration::from_secs(20),
    }
}

fn browser() -> reqwest::Client {
    let _ = rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap()
}

/// What a browser does with the sign-in address: follows the authorization server's
/// redirect back to the loopback listener.
async fn browse(address: Url) -> (StatusCode, String) {
    let http = browser();
    let authorized = http.get(address).send().await.unwrap();
    assert_eq!(authorized.status(), StatusCode::FOUND);
    let location = authorized.headers()[header::LOCATION]
        .to_str()
        .unwrap()
        .to_owned();
    let callback = http.get(location).send().await.unwrap();
    let status = callback.status();
    (status, callback.text().await.unwrap())
}

/// Signs in with a browser that first sends `stray` requests to the listener.
async fn login_after(
    fake: &Fake,
    grants: &GrantStore,
    stray: Vec<(String, StatusCode)>,
) -> (anyhow::Result<SignedIn>, (StatusCode, String)) {
    let (sender, receiver) = tokio::sync::oneshot::channel::<Url>();
    let browsing = tokio::spawn(async move {
        let address = receiver.await.unwrap();
        for (url, expected) in stray {
            let answer = browser().get(url.as_str()).send().await.unwrap();
            assert_eq!(answer.status(), expected, "{url}");
        }
        browse(address).await
    });
    let signed_in = sign_in::sign_in(
        &access(fake, grants),
        grants,
        &request(fake),
        move |address| {
            let _ = sender.send(address.clone());
        },
    )
    .await;
    (signed_in, browsing.await.unwrap())
}

async fn login(fake: &Fake, grants: &GrantStore) -> SignedIn {
    let (signed_in, (status, _)) = login_after(fake, grants, Vec::new()).await;
    assert_eq!(status, StatusCode::OK);
    signed_in.unwrap()
}

fn bearer(fake: &Fake, grants: &GrantStore) -> anyhow::Result<GrantBearer> {
    let access = access(fake, grants);
    GrantBearer::load(grants.clone(), &fake.base, &profile(), access.oauth_http()?)
}

async fn caller_token(bearer: &GrantBearer) -> Result<String, String> {
    bearer
        .caller_bearer()
        .await
        .map(|bearer| bearer.expose().to_owned())
        .map_err(|error| error.to_string())
}

fn files(directory: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[cfg(unix)]
fn mode(path: &Path) -> u32 {
    std::fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[tokio::test]
async fn a_browser_sign_in_stores_a_private_grant() {
    let fake = Fake::start(Config::new(free_port())).await;
    let scratch = Scratch::new();
    let grants = scratch.grants();
    let (signed_in, (status, page)) = login_after(&fake, &grants, Vec::new()).await;
    let signed_in = signed_in.unwrap();
    assert_eq!(status, StatusCode::OK);
    assert!(page.contains("reached veoveo-hub-sdk-cli"));

    let ledger = fake.ledger();
    let authorization = &ledger.authorizations[0];
    assert_eq!(authorization["scope"], "mcp");
    assert_eq!(authorization["code_challenge_method"], "S256");
    assert_eq!(authorization["state"].len(), 43);
    assert_eq!(ledger.code_exchanges.len(), 1);
    assert_eq!(ledger.pkce_verified, 1);
    let exchange = &ledger.code_exchanges[0];
    assert_eq!(exchange["code_verifier"].len(), 43);
    assert_ne!(exchange["code_verifier"], authorization["code_challenge"]);
    assert_eq!(
        exchange["resource"],
        fake.base.join("mcp/local").unwrap().as_str()
    );
    assert!(!exchange.contains_key("scope"));
    assert!(
        ledger
            .user_agents
            .iter()
            .all(|agent| agent.starts_with("veoveo-hub-sdk-cli/")),
        "{:?}",
        ledger.user_agents
    );

    let path = grants.path(&fake.base, &profile());
    assert_eq!(signed_in.path, path);
    let grant = grants.load(&fake.base, &profile()).unwrap().unwrap();
    assert_eq!(grant.access_token.expose(), "at-1");
    assert_eq!(grant.refresh_token.as_ref().unwrap().expose(), "rt-1");
    assert_eq!(grant.client_id.as_str(), "veoveo-hub-sdk-cli");
    let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
    assert_eq!(
        files(grants.directory()),
        [format!(".{stem}.lock"), format!("{stem}.json")]
    );
    #[cfg(unix)]
    {
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(grants.directory()), 0o700);
    }
}

#[tokio::test]
async fn a_stray_callback_neither_ends_nor_hijacks_the_sign_in() {
    let config = Config::new(free_port());
    let callback = config.redirect_uri.clone();
    let fake = Fake::start(config).await;
    let scratch = Scratch::new();
    let grants = scratch.grants();
    let stray = vec![
        (
            format!("{callback}?code=forged&state=forged"),
            StatusCode::BAD_REQUEST,
        ),
        (
            callback.replace("/callback", "/favicon.ico"),
            StatusCode::NOT_FOUND,
        ),
    ];
    let (signed_in, (status, _)) = login_after(&fake, &grants, stray).await;
    signed_in.unwrap();
    assert_eq!(status, StatusCode::OK);
    let ledger = fake.ledger();
    assert_eq!(ledger.code_exchanges.len(), 1);
    assert_eq!(ledger.code_exchanges[0]["code"], "code-1");
}

#[tokio::test]
async fn a_refused_sign_in_reports_the_hubs_error() {
    let mut config = Config::new(free_port());
    config.authorize_error = Some("access_denied");
    let fake = Fake::start(config).await;
    let scratch = Scratch::new();
    let grants = scratch.grants();
    let (signed_in, (status, _)) = login_after(&fake, &grants, Vec::new()).await;
    assert_eq!(status, StatusCode::OK);
    let error = signed_in.unwrap_err().to_string();
    assert!(error.contains("access_denied"), "{error}");
    assert!(fake.ledger().code_exchanges.is_empty());
    assert!(grants.load(&fake.base, &profile()).unwrap().is_none());
}

#[tokio::test]
async fn a_stale_access_token_rotates_once_into_a_replaced_grant() {
    let mut config = Config::new(free_port());
    config.code_lifetime = 0;
    let fake = Fake::start(config).await;
    let scratch = Scratch::new();
    let grants = scratch.grants();
    login(&fake, &grants).await;
    let bearer = bearer(&fake, &grants).unwrap();
    assert_eq!(caller_token(&bearer).await.unwrap(), "at-2");
    assert_eq!(caller_token(&bearer).await.unwrap(), "at-2");

    let ledger = fake.ledger();
    assert_eq!(ledger.refreshes.len(), 1);
    let refresh = &ledger.refreshes[0];
    assert_eq!(refresh["refresh_token"], "rt-1");
    assert_eq!(refresh["client_id"], "veoveo-hub-sdk-cli");
    assert_eq!(
        refresh["resource"],
        fake.base.join("mcp/local").unwrap().as_str()
    );
    assert!(!refresh.contains_key("scope"));
    assert!(
        ledger
            .user_agents
            .iter()
            .all(|agent| agent.starts_with("veoveo-hub-sdk-cli/"))
    );
    drop(ledger);

    let grant = grants.load(&fake.base, &profile()).unwrap().unwrap();
    assert_eq!(grant.access_token.expose(), "at-2");
    assert_eq!(grant.refresh_token.unwrap().expose(), "rt-2");
    assert!(grant.renewal_sent_at.is_none());
    assert!(
        files(grants.directory())
            .iter()
            .all(|name| !name.ends_with(".tmp"))
    );
    #[cfg(unix)]
    assert_eq!(mode(&grants.path(&fake.base, &profile())), 0o600);
}

#[tokio::test]
async fn concurrent_renewals_in_two_processes_send_each_refresh_token_once() {
    let mut config = Config::new(free_port());
    config.code_lifetime = 0;
    config.refresh_delay = Duration::from_millis(300);
    let fake = Fake::start(config).await;
    let scratch = Scratch::new();
    let grants = scratch.grants();
    login(&fake, &grants).await;
    // Two bearers loaded apart hold separate memory and separate lock files, as two
    // processes would.
    let first = bearer(&fake, &grants).unwrap();
    let second = bearer(&fake, &grants).unwrap();
    let (first, second) = tokio::join!(caller_token(&first), caller_token(&second));
    assert_eq!(first.unwrap(), "at-2");
    assert_eq!(second.unwrap(), "at-2");
    let ledger = fake.ledger();
    assert_eq!(ledger.presentations.get("rt-1"), Some(&1));
    assert_eq!(ledger.refreshes.len(), 1);
    assert_eq!(ledger.replays, 0);
    assert!(!ledger.family_revoked);
}

#[tokio::test]
async fn a_bearer_holding_a_spent_refresh_token_takes_the_renewed_grant() {
    let mut config = Config::new(free_port());
    config.code_lifetime = 0;
    let fake = Fake::start(config).await;
    let scratch = Scratch::new();
    let grants = scratch.grants();
    login(&fake, &grants).await;
    let holding_rt1 = bearer(&fake, &grants).unwrap();
    let other = bearer(&fake, &grants).unwrap();
    assert_eq!(caller_token(&other).await.unwrap(), "at-2");
    assert_eq!(caller_token(&holding_rt1).await.unwrap(), "at-2");
    // The Hub stops admitting the token: the next renewal sends the current refresh
    // token, never the spent one.
    holding_rt1.forget_caller();
    assert_eq!(caller_token(&holding_rt1).await.unwrap(), "at-3");
    let ledger = fake.ledger();
    assert_eq!(ledger.presentations.get("rt-1"), Some(&1));
    assert_eq!(ledger.presentations.get("rt-2"), Some(&1));
    assert_eq!(ledger.replays, 0);
}

#[tokio::test]
async fn an_unanswered_renewal_never_resends_its_refresh_token() {
    let mut config = Config::new(free_port());
    config.code_lifetime = 0;
    config.refresh_mode = RefreshMode::ConsumeWithoutAnswer;
    let fake = Fake::start(config).await;
    let scratch = Scratch::new();
    let grants = scratch.grants();
    login(&fake, &grants).await;
    let error = caller_token(&bearer(&fake, &grants).unwrap())
        .await
        .unwrap_err();
    assert!(
        error.contains("may be spent") && error.contains(LOGIN),
        "{error}"
    );
    assert!(!error.contains("rt-") && !error.contains("at-"), "{error}");

    let grant = grants.load(&fake.base, &profile()).unwrap().unwrap();
    assert!(grant.refresh_token.is_none());
    assert!(grant.renewal_sent_at.is_some());
    let error = bearer(&fake, &grants).unwrap_err().to_string();
    assert!(
        error.contains("left no refresh token") && error.contains(LOGIN),
        "{error}"
    );
    let ledger = fake.ledger();
    assert_eq!(ledger.presentations.get("rt-1"), Some(&1));
    assert_eq!(ledger.replays, 0);
}

#[tokio::test]
async fn a_refused_renewal_names_login() {
    let mut config = Config::new(free_port());
    config.code_lifetime = 0;
    config.refresh_mode = RefreshMode::Refuse;
    let fake = Fake::start(config).await;
    let scratch = Scratch::new();
    let grants = scratch.grants();
    login(&fake, &grants).await;
    let error = caller_token(&bearer(&fake, &grants).unwrap())
        .await
        .unwrap_err();
    assert!(
        error.contains("invalid_grant") && error.contains(LOGIN),
        "{error}"
    );
    assert!(!error.contains("rt-1"), "{error}");
}

#[tokio::test]
async fn an_unreachable_token_endpoint_keeps_the_unsent_refresh_token() {
    let mut config = Config::new(free_port());
    config.code_lifetime = 0;
    let fake = Fake::start(config).await;
    let scratch = Scratch::new();
    let grants = scratch.grants();
    login(&fake, &grants).await;
    fake.server.abort();
    while !fake.server.is_finished() {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let error = caller_token(&bearer(&fake, &grants).unwrap())
        .await
        .unwrap_err();
    assert!(error.contains("cannot reach"), "{error}");
    let grant = grants.load(&fake.base, &profile()).unwrap().unwrap();
    assert_eq!(grant.refresh_token.unwrap().expose(), "rt-1");
    assert!(grant.renewal_sent_at.is_none());
}

#[tokio::test]
async fn logout_revokes_the_refresh_token_and_deletes_the_grant() {
    let fake = Fake::start(Config::new(free_port())).await;
    let scratch = Scratch::new();
    let grants = scratch.grants();
    login(&fake, &grants).await;
    let access = access(&fake, &grants);
    assert_eq!(
        sign_in::sign_out(&access, &grants).await.unwrap(),
        SignedOut::Deleted {
            path: grants.path(&fake.base, &profile()),
            revocation: Revocation::Revoked,
        }
    );
    {
        let ledger = fake.ledger();
        let revocation = &ledger.revocations[0];
        assert_eq!(revocation["token"], "rt-1");
        assert_eq!(revocation["token_type_hint"], "refresh_token");
        assert_eq!(revocation["client_id"], "veoveo-hub-sdk-cli");
        assert!(ledger.family_revoked);
    }
    assert!(grants.load(&fake.base, &profile()).unwrap().is_none());
    assert_eq!(
        sign_in::sign_out(&access, &grants).await.unwrap(),
        SignedOut::NotSignedIn
    );
}

#[tokio::test]
async fn a_missing_grant_names_login_before_contacting_the_hub() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let scratch = Scratch::new();
    let access = HubAccess {
        hub: Url::parse("http://127.0.0.1:1").unwrap(),
        connect: None,
        authentication: HubAuthentication::StoredGrant {
            grants: scratch.grants(),
        },
        profile: profile(),
    };
    let Err(error) = access.client().await else {
        panic!("no grant must be refused");
    };
    assert_eq!(
        error.to_string(),
        format!("there is no sign-in to http://127.0.0.1:1 for profile local; {LOGIN}")
    );
}

#[cfg(unix)]
#[tokio::test]
async fn a_grant_other_users_can_read_is_refused() {
    let fake = Fake::start(Config::new(free_port())).await;
    let scratch = Scratch::new();
    let grants = scratch.grants();
    let signed_in = login(&fake, &grants).await;
    std::fs::set_permissions(&signed_in.path, std::fs::Permissions::from_mode(0o644)).unwrap();
    let error = bearer(&fake, &grants).unwrap_err().to_string();
    assert!(error.contains("open to other users"), "{error}");
}

/// The binary's own `login --no-browser` and `logout`, with the address read from its
/// standard error as a person would copy it.
#[tokio::test]
async fn the_cli_signs_in_and_out() {
    let config = Config::new(free_port());
    let redirect = config.redirect_uri.clone();
    let fake = Fake::start(config).await;
    let scratch = Scratch::new();
    let grants = scratch.grants().directory().to_owned();
    let command = |arguments: &[&str]| {
        let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_veoveo-hub-sdk-cli"));
        command
            .args(["--hub", fake.base.as_str(), "--grants"])
            .arg(&grants)
            .args(arguments)
            .env_remove("VEOVEO_CLI_KEY_DIRECTORY")
            .env_remove("VEOVEO_CLI_BEARER_ENV")
            .env_remove("VEOVEO_CLI_PROFILE")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        command
    };
    let mut login = command(&["login", "--no-browser", "--redirect-uri", &redirect])
        .spawn()
        .unwrap();
    let mut errors = BufReader::new(login.stderr.take().unwrap()).lines();
    let address = loop {
        let line = errors.next_line().await.unwrap().expect("an address line");
        if let Some(address) = line.trim().strip_prefix("http") {
            break Url::parse(&format!("http{address}")).unwrap();
        }
    };
    let (status, _) = browse(address).await;
    assert_eq!(status, StatusCode::OK);
    let output = login.wait_with_output().await.unwrap();
    assert!(output.status.success());
    let printed = String::from_utf8(output.stdout).unwrap();
    assert!(printed.starts_with("Signed in to "), "{printed}");
    assert!(!printed.contains("rt-1") && !printed.contains("at-1"));

    let output = command(&["logout"]).output().await.unwrap();
    assert!(output.status.success());
    let printed = String::from_utf8(output.stdout).unwrap();
    assert!(printed.contains("The Hub revoked the grant."), "{printed}");
    assert!(fake.ledger().family_revoked);

    let output = command(&["tools"]).output().await.unwrap();
    assert!(!output.status.success());
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(error.contains(LOGIN), "{error}");
}
