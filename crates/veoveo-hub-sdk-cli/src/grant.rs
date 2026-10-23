//! A person's own grant from `login`: one file per Hub origin and profile in the user's
//! configuration directory, readable only by its owner and replaced atomically. Its
//! bearer renews the access token by refresh-token rotation before it expires and sends
//! each refresh token at most once.

use std::{
    ffi::OsString,
    fmt,
    fs::{self, File, OpenOptions, TryLockError},
    future::Future,
    io::{self, Write as _},
    path::{Path, PathBuf},
    pin::Pin,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _, PermissionsExt as _};

use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};
use url::Url;
use veoveo_hub_sdk::{
    HubProfileId,
    client::{Bearer, BearerSource, CallError},
};
use veoveo_types::{OAuthClientId, ServerSlug};

use crate::oauth::{self, TokenFailure, TokenResponse};

type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The longest a process waits for another to release a grant's lock.
pub const LOCK_WAIT: Duration = Duration::from_secs(60);

/// What every error that a new sign-in resolves tells the person to do.
pub const LOGIN: &str = "run `veoveo-hub-sdk-cli login`";

/// A token value. It serializes as itself into the grant file and nowhere else, and its
/// `Debug` form shows no part of it.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Secret(String);

impl Secret {
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<redacted>")
    }
}

/// The tokens one sign-in holds for one profile, with where to renew and revoke them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grant {
    /// The Hub's origin, as `--hub` names it.
    pub hub: Url,
    pub profile: HubProfileId,
    /// The public OAuth client the person signed in through.
    pub client_id: OAuthClientId,
    /// The profile's protected resource, which the tokens name as their audience.
    pub resource: Url,
    pub token_endpoint: Url,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revocation_endpoint: Option<Url>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    pub access_token: Secret,
    /// When the access token was requested, so its lifetime counts from before the
    /// request.
    pub access_token_issued_at: DateTime<Utc>,
    pub access_token_expires_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<Secret>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_token_expires_at: Option<DateTime<Utc>>,
    /// When a renewal sent the grant's last refresh token. The file holds no refresh
    /// token while that renewal is unanswered, and keeps none when its answer never
    /// arrived, because the token may be spent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub renewal_sent_at: Option<DateTime<Utc>>,
}

impl Grant {
    /// The grant a token response issues, requested at `requested_at`.
    pub(crate) fn issued(
        hub: &Url,
        profile: &HubProfileId,
        client_id: &OAuthClientId,
        server: &oauth::AuthorizationServer,
        response: TokenResponse,
        requested_at: DateTime<Utc>,
    ) -> Self {
        Self {
            hub: origin(hub),
            profile: profile.clone(),
            client_id: client_id.clone(),
            resource: server.resource.clone(),
            token_endpoint: server.token_endpoint.clone(),
            revocation_endpoint: server.revocation_endpoint.clone(),
            scope: response.scope,
            access_token: Secret(response.access_token),
            access_token_issued_at: requested_at,
            access_token_expires_at: after(requested_at, response.expires_in),
            refresh_token: response.refresh_token.map(Secret),
            refresh_token_expires_at: response
                .refresh_token_expires_in
                .map(|seconds| after(requested_at, seconds)),
            renewal_sent_at: None,
        }
    }

    /// This grant after a refresh at `requested_at`. RFC 6749 §6: a response without a
    /// refresh token leaves the current one in force.
    fn renewed(&self, response: TokenResponse, requested_at: DateTime<Utc>) -> Self {
        Self {
            access_token: Secret(response.access_token),
            access_token_issued_at: requested_at,
            access_token_expires_at: after(requested_at, response.expires_in),
            refresh_token_expires_at: match &response.refresh_token {
                Some(_) => response
                    .refresh_token_expires_in
                    .map(|seconds| after(requested_at, seconds))
                    .or(self.refresh_token_expires_at),
                None => self.refresh_token_expires_at,
            },
            refresh_token: response
                .refresh_token
                .map(Secret)
                .or_else(|| self.refresh_token.clone()),
            scope: response.scope.or_else(|| self.scope.clone()),
            renewal_sent_at: None,
            ..self.clone()
        }
    }

    /// The access token serves until a third of its lifetime remains.
    pub fn is_fresh(&self, now: DateTime<Utc>) -> bool {
        let lifetime = self.access_token_expires_at - self.access_token_issued_at;
        now < self.access_token_issued_at + lifetime * 2 / 3
    }

    /// The refresh token a renewal at `now` may send, or why there is none.
    fn renewable(&self, now: DateTime<Utc>) -> Result<&Secret, String> {
        let Some(refresh) = &self.refresh_token else {
            return Err(match self.renewal_sent_at {
                Some(sent) => format!(
                    "the sign-in's last renewal, sent at {}, left no refresh token to send; {LOGIN}",
                    sent.to_rfc3339()
                ),
                None => format!(
                    "the sign-in's access token has expired and the sign-in holds no refresh token; {LOGIN}"
                ),
            });
        };
        if let Some(expiry) = self.refresh_token_expires_at
            && expiry <= now
        {
            return Err(format!(
                "the sign-in expired at {}; {LOGIN}",
                expiry.to_rfc3339()
            ));
        }
        Ok(refresh)
    }

    /// A grant serves while its access token is fresh or a renewal can replace it.
    fn usable(&self, now: DateTime<Utc>) -> Result<(), String> {
        if self.is_fresh(now) {
            return Ok(());
        }
        self.renewable(now).map(|_| ())
    }
}

fn after(at: DateTime<Utc>, seconds: u64) -> DateTime<Utc> {
    let seconds = i64::try_from(seconds).unwrap_or(i64::MAX);
    at.checked_add_signed(TimeDelta::try_seconds(seconds).unwrap_or(TimeDelta::MAX))
        .unwrap_or(DateTime::<Utc>::MAX_UTC)
}

/// The URL of `hub`'s origin, which keys its grants.
pub(crate) fn origin(hub: &Url) -> Url {
    Url::parse(&hub.origin().ascii_serialization()).unwrap_or_else(|_| hub.clone())
}

/// The grants of one user: `<directory>/<scheme>_<host>_<port>_<profile>.json`, each
/// with a lock file beside it that serializes its renewals across processes.
#[derive(Debug, Clone)]
pub struct GrantStore {
    directory: PathBuf,
}

impl GrantStore {
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self {
            directory: directory.into(),
        }
    }

    /// `$XDG_CONFIG_HOME/veoveo-hub-sdk-cli/grants`, or `~/.config/...` when
    /// `XDG_CONFIG_HOME` names no absolute directory.
    pub fn default_directory(
        xdg_config_home: Option<OsString>,
        home: Option<OsString>,
    ) -> Result<PathBuf> {
        let base = match xdg_config_home.map(PathBuf::from) {
            Some(path) if path.is_absolute() => path,
            _ => PathBuf::from(home.filter(|home| !home.is_empty()).context(
                "neither XDG_CONFIG_HOME nor HOME names a configuration directory; name one with --grants",
            )?)
            .join(".config"),
        };
        Ok(base.join("veoveo-hub-sdk-cli").join("grants"))
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    /// The file holding the grant for `hub`'s origin and `profile`.
    pub fn path(&self, hub: &Url, profile: &HubProfileId) -> PathBuf {
        self.directory.join(format!("{}.json", stem(hub, profile)))
    }

    fn lock_path(&self, hub: &Url, profile: &HubProfileId) -> PathBuf {
        self.directory.join(format!(".{}.lock", stem(hub, profile)))
    }

    /// The stored grant, or none when there is no sign-in. A grant file other users can
    /// read, or one that names another Hub or profile, is refused.
    pub fn load(&self, hub: &Url, profile: &HubProfileId) -> Result<Option<Grant>> {
        let path = self.path(hub, profile);
        let file = match File::open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error).with_context(|| format!("read {}", path.display())),
        };
        #[cfg(unix)]
        {
            let mode = file
                .metadata()
                .with_context(|| format!("read {}", path.display()))?
                .permissions()
                .mode();
            if mode & 0o077 != 0 {
                bail!(
                    "{} is open to other users (mode {:o}); delete it and {LOGIN}",
                    path.display(),
                    mode & 0o777
                );
            }
        }
        let grant: Grant = serde_json::from_reader(io::BufReader::new(file))
            .with_context(|| format!("{} holds no grant; {LOGIN}", path.display()))?;
        if grant.hub.origin() != hub.origin() || &grant.profile != profile {
            bail!(
                "{} holds the grant of another Hub or profile; {LOGIN}",
                path.display()
            );
        }
        Ok(Some(grant))
    }

    /// Writes `grant` to a new file readable only by its owner, then renames it over the
    /// grant's file, so a reader sees the whole previous grant or the whole new one.
    pub(crate) fn replace(&self, grant: &Grant) -> Result<PathBuf> {
        self.create_directory()?;
        let path = self.path(&grant.hub, &grant.profile);
        let bytes = serde_json::to_vec_pretty(grant)?;
        let temporary = self.directory.join(format!(
            ".{}.{}.tmp",
            stem(&grant.hub, &grant.profile),
            &oauth::random_value()?[..16]
        ));
        let written =
            write_private(&temporary, &bytes).and_then(|()| fs::rename(&temporary, &path));
        if let Err(error) = written {
            let _ = fs::remove_file(&temporary);
            return Err(error).with_context(|| format!("write {}", path.display()));
        }
        sync_directory(&self.directory);
        Ok(path)
    }

    /// Deletes the grant's file; false when there was none.
    pub(crate) fn delete(&self, hub: &Url, profile: &HubProfileId) -> Result<bool> {
        let path = self.path(hub, profile);
        match fs::remove_file(&path) {
            Ok(()) => {
                sync_directory(&self.directory);
                Ok(true)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error).with_context(|| format!("delete {}", path.display())),
        }
    }

    /// The exclusive lock of the grant for `hub` and `profile`, held by one process at a
    /// time until the returned guard drops. A holder keeps it for at most one token
    /// request, so a lock still held after [`LOCK_WAIT`] belongs to a stopped process.
    pub(crate) async fn lock(&self, hub: &Url, profile: &HubProfileId) -> Result<GrantLock> {
        self.create_directory()?;
        let path = self.lock_path(hub, profile);
        tokio::task::spawn_blocking(move || {
            let mut options = OpenOptions::new();
            options.read(true).write(true).create(true).truncate(false);
            #[cfg(unix)]
            options.mode(0o600);
            let file = options
                .open(&path)
                .with_context(|| format!("open {}", path.display()))?;
            let deadline = Instant::now() + LOCK_WAIT;
            loop {
                match file.try_lock() {
                    Ok(()) => return Ok(GrantLock { _file: file }),
                    Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                        std::thread::sleep(Duration::from_millis(50));
                    }
                    Err(TryLockError::WouldBlock) => bail!(
                        "another veoveo-hub-sdk-cli process has held {} for {} seconds; end it and try again",
                        path.display(),
                        LOCK_WAIT.as_secs()
                    ),
                    Err(TryLockError::Error(error)) => {
                        return Err(error).with_context(|| format!("lock {}", path.display()));
                    }
                }
            }
        })
        .await
        .map_err(|error| anyhow!("the grant lock task failed: {error}"))?
    }

    fn create_directory(&self) -> Result<()> {
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        builder.mode(0o700);
        builder
            .create(&self.directory)
            .with_context(|| format!("create {}", self.directory.display()))
    }
}

/// Holds a grant's lock; the operating system releases it when the file closes.
pub(crate) struct GrantLock {
    _file: File,
}

fn stem(hub: &Url, profile: &HubProfileId) -> String {
    let host: String = hub
        .host_str()
        .unwrap_or_default()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '-') {
                character
            } else {
                '_'
            }
        })
        .collect();
    format!(
        "{}_{host}_{}_{}",
        hub.scheme(),
        hub.port_or_known_default().unwrap_or_default(),
        profile.as_str()
    )
}

fn write_private(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()
}

/// Makes a rename in `directory` durable where the platform syncs directories.
fn sync_directory(directory: &Path) {
    if let Ok(directory) = File::open(directory) {
        let _ = directory.sync_all();
    }
}

/// The person's stored grant as the bearer of every request a `HubClient` makes. It
/// serves the access token until a third of its lifetime remains or the Hub stops
/// admitting it, then renews under the grant's lock. A renewal first reads the stored
/// grant again and takes a token another process renewed instead of sending a refresh
/// token that process already spent.
#[derive(Clone)]
pub struct GrantBearer {
    inner: Arc<Inner>,
}

struct Inner {
    grants: GrantStore,
    hub: Url,
    profile: HubProfileId,
    http: reqwest::Client,
    state: Mutex<State>,
    renewing: tokio::sync::Mutex<()>,
}

struct State {
    grant: Grant,
    /// The Hub stopped admitting the grant's access token.
    rejected: bool,
}

impl fmt::Debug for GrantBearer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GrantBearer")
            .field("hub", &self.inner.hub.as_str())
            .field("profile", &self.inner.profile)
            .finish_non_exhaustive()
    }
}

impl GrantBearer {
    /// The stored grant for `hub` and `profile`, renewed through `http`, which follows
    /// no redirect. Fails before any request when there is no usable grant.
    pub fn load(
        grants: GrantStore,
        hub: &Url,
        profile: &HubProfileId,
        http: reqwest::Client,
    ) -> Result<Self> {
        let grant = grants.load(hub, profile)?.with_context(|| {
            format!(
                "there is no sign-in to {} for profile {profile}; {LOGIN}",
                hub.origin().ascii_serialization()
            )
        })?;
        grant.usable(Utc::now()).map_err(|reason| anyhow!(reason))?;
        Ok(Self {
            inner: Arc::new(Inner {
                grants,
                hub: hub.clone(),
                profile: profile.clone(),
                http,
                state: Mutex::new(State {
                    grant,
                    rejected: false,
                }),
                renewing: tokio::sync::Mutex::new(()),
            }),
        })
    }

    fn fresh(&self) -> Option<Bearer> {
        let state = self.state();
        (!state.rejected && state.grant.is_fresh(Utc::now()))
            .then(|| Bearer::new(state.grant.access_token.expose()))
    }

    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.inner
            .state
            .lock()
            .expect("the grant lock is never poisoned")
    }

    async fn current(&self) -> Result<Bearer, CallError> {
        if let Some(bearer) = self.fresh() {
            return Ok(bearer);
        }
        let _renewing = self.inner.renewing.lock().await;
        // Another request of this process may have renewed it while this one waited.
        if let Some(bearer) = self.fresh() {
            return Ok(bearer);
        }
        self.renew().await.map_err(CallError::Transport)
    }

    async fn renew(&self) -> Result<Bearer, String> {
        let inner = &self.inner;
        let _lock = inner
            .grants
            .lock(&inner.hub, &inner.profile)
            .await
            .map_err(|error| format!("{error:#}"))?;
        let stored = inner
            .grants
            .load(&inner.hub, &inner.profile)
            .map_err(|error| format!("{error:#}"))?
            .ok_or_else(|| {
                format!(
                    "the sign-in to {} for profile {} was removed; {LOGIN}",
                    inner.hub.origin().ascii_serialization(),
                    inner.profile
                )
            })?;
        let (held, rejected) = {
            let state = self.state();
            (state.grant.access_token.clone(), state.rejected)
        };
        let grant = if stored.is_fresh(Utc::now()) && !(rejected && stored.access_token == held) {
            // Another process renewed the grant, spending the refresh token this one held.
            stored
        } else {
            renew(&inner.http, &inner.grants, stored).await?
        };
        let bearer = Bearer::new(grant.access_token.expose());
        *self.state() = State {
            grant,
            rejected: false,
        };
        Ok(bearer)
    }

    fn reject(&self) {
        self.state().rejected = true;
    }
}

/// One refresh-token rotation of `grant`, under its lock. The stored grant gives up its
/// refresh token before the token is sent, so a process that ends before the answer is
/// stored leaves a grant that cannot send it again. A request that never connected
/// restores it; any other failure leaves it spent.
async fn renew(http: &reqwest::Client, grants: &GrantStore, grant: Grant) -> Result<Grant, String> {
    let requested_at = Utc::now();
    let refresh = grant.renewable(requested_at)?.clone();
    let mut sending = grant.clone();
    sending.refresh_token = None;
    sending.renewal_sent_at = Some(requested_at);
    grants
        .replace(&sending)
        .map_err(|error| format!("cannot record the renewal before sending it: {error:#}"))?;
    let form = [
        ("grant_type", "refresh_token"),
        ("client_id", grant.client_id.as_str()),
        ("refresh_token", refresh.expose()),
        ("resource", grant.resource.as_str()),
    ];
    match oauth::token_request(http, &grant.token_endpoint, &form).await {
        Ok(response) => {
            let renewed = grant.renewed(response, requested_at);
            grants.replace(&renewed).map_err(|error| {
                format!("the Hub renewed the sign-in, but storing it failed: {error:#}; {LOGIN}")
            })?;
            Ok(renewed)
        }
        Err(TokenFailure::Unsent(reason)) => {
            grants.replace(&grant).map_err(|error| {
                format!("cannot restore the unsent refresh token: {error:#}; {LOGIN}")
            })?;
            Err(format!(
                "cannot reach the Hub's token endpoint to renew the sign-in: {reason}"
            ))
        }
        Err(TokenFailure::Refused(refusal)) => Err(format!(
            "the Hub refused to renew the sign-in: {}; {LOGIN}",
            oauth::refusal_text(&refusal)
        )),
        Err(TokenFailure::Unconfirmed(reason)) => Err(format!(
            "the renewal's answer did not arrive ({reason}), so its refresh token may be spent and is not sent again; {LOGIN}"
        )),
    }
}

impl BearerSource for GrantBearer {
    fn bearer<'a>(&'a self, _: &'a ServerSlug) -> BoxFuture<'a, Result<Bearer, CallError>> {
        Box::pin(self.current())
    }

    fn forget(&self, _: &ServerSlug) {
        self.reject();
    }

    fn caller_bearer(&self) -> BoxFuture<'_, Result<Bearer, CallError>> {
        Box::pin(self.current())
    }

    fn forget_caller(&self) {
        self.reject();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grant(issued: DateTime<Utc>, lifetime: i64) -> Grant {
        let hub = Url::parse("https://hub.example").unwrap();
        Grant {
            hub: hub.clone(),
            profile: HubProfileId::parse("local").unwrap(),
            client_id: OAuthClientId::parse("veoveo-hub-sdk-cli").unwrap(),
            resource: hub.join("mcp/local").unwrap(),
            token_endpoint: hub.join("oauth/token").unwrap(),
            revocation_endpoint: None,
            scope: Some("mcp".into()),
            access_token: Secret("access".into()),
            access_token_issued_at: issued,
            access_token_expires_at: issued + TimeDelta::seconds(lifetime),
            refresh_token: Some(Secret("refresh".into())),
            refresh_token_expires_at: Some(issued + TimeDelta::days(7)),
            renewal_sent_at: None,
        }
    }

    #[test]
    fn an_access_token_renews_once_a_third_of_its_lifetime_remains() {
        let issued = Utc::now();
        let grant = grant(issued, 900);
        assert!(grant.is_fresh(issued + TimeDelta::seconds(599)));
        assert!(!grant.is_fresh(issued + TimeDelta::seconds(600)));
    }

    #[test]
    fn a_grant_whose_refresh_token_was_sent_cannot_renew() {
        let issued = Utc::now() - TimeDelta::hours(1);
        let mut sent = grant(issued, 900);
        sent.refresh_token = None;
        sent.renewal_sent_at = Some(issued);
        let reason = sent.usable(Utc::now()).unwrap_err();
        assert!(reason.contains("left no refresh token") && reason.ends_with(LOGIN));
    }

    #[test]
    fn debug_output_shows_no_token() {
        let text = format!("{:?}", grant(Utc::now(), 900));
        assert!(!text.contains("\"access\"") && !text.contains("\"refresh\""));
        assert!(text.contains("<redacted>"));
    }

    #[test]
    fn grant_files_are_named_by_origin_and_profile() {
        let store = GrantStore::new("/grants");
        let profile = HubProfileId::parse("local").unwrap();
        let path = store.path(&Url::parse("https://veoveo.example/").unwrap(), &profile);
        assert_eq!(
            path,
            Path::new("/grants/https_veoveo.example_443_local.json")
        );
        let path = store.path(&Url::parse("http://[::1]:8080").unwrap(), &profile);
        assert_eq!(path, Path::new("/grants/http____1__8080_local.json"));
    }

    #[test]
    fn the_default_directory_follows_xdg() {
        let path = GrantStore::default_directory(Some("/xdg".into()), Some("/home/person".into()))
            .unwrap();
        assert_eq!(path, Path::new("/xdg/veoveo-hub-sdk-cli/grants"));
        let path =
            GrantStore::default_directory(Some("relative".into()), Some("/home/person".into()))
                .unwrap();
        assert_eq!(
            path,
            Path::new("/home/person/.config/veoveo-hub-sdk-cli/grants")
        );
        assert!(GrantStore::default_directory(None, None).is_err());
    }
}
