//! A person's browser sign-in to a Hub profile: the OAuth 2.0 authorization code grant,
//! RFC 6749 §4.1, with PKCE S256, RFC 7636, as a native app whose redirect returns to a
//! loopback listener, RFC 8252 §7.3. The resulting grant is stored for the other
//! commands; signing out revokes its refresh token, RFC 7009, and deletes it.

use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result, anyhow, bail, ensure};
use chrono::{DateTime, Utc};
use tokio::{
    io::{AsyncReadExt as _, AsyncWriteExt as _},
    net::{TcpListener, TcpStream},
    sync::mpsc,
};
use url::Url;
use veoveo_types::{OAuthClientId, ScopeName};

use crate::{
    grant::{Grant, GrantStore},
    hub::HubAccess,
    oauth::{self, TokenFailure},
};

/// The longest a callback connection may take to send its request.
const CALLBACK_READ_TIMEOUT: Duration = Duration::from_secs(10);
/// The most of a callback request read: its request line and headers.
const CALLBACK_HEAD_LIMIT: usize = 16 * 1024;

/// What one sign-in asks for.
#[derive(Debug, Clone)]
pub struct SignIn {
    /// The public client the installation registers for this redirect.
    pub client: OAuthClientId,
    /// A registered `http://127.0.0.1:<port>/<path>` redirect; the Hub matches it exactly.
    pub redirect_uri: Url,
    pub scope: ScopeName,
    /// How long to wait for the browser's callback.
    pub timeout: Duration,
}

/// A completed sign-in.
#[derive(Debug, Clone)]
pub struct SignedIn {
    pub path: PathBuf,
    pub access_token_expires_at: DateTime<Utc>,
    pub refresh_token_expires_at: Option<DateTime<Utc>>,
}

/// Signs the person in through their browser and stores the grant in `grants`.
/// `open` receives the authorization address once the callback listener is ready; it
/// opens a browser there or shows it to the person.
pub async fn sign_in(
    access: &HubAccess,
    grants: &GrantStore,
    request: &SignIn,
    open: impl FnOnce(&Url),
) -> Result<SignedIn> {
    let listen = loopback_address(&request.redirect_uri)?;
    let http = access.oauth_http()?;
    let server = oauth::discover(&http, &access.hub, &access.endpoint()?).await?;
    let listener = TcpListener::bind(listen).await.with_context(|| {
        format!(
            "cannot listen on {listen} for the sign-in's redirect; another sign-in may be waiting there"
        )
    })?;
    let verifier = oauth::random_value()?;
    let state = oauth::random_value()?;
    let mut authorization = server.authorization_endpoint.clone();
    authorization
        .query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", request.client.as_str())
        .append_pair("redirect_uri", request.redirect_uri.as_str())
        .append_pair("code_challenge", &oauth::s256_challenge(&verifier))
        .append_pair("code_challenge_method", "S256")
        .append_pair("scope", request.scope.as_str())
        .append_pair("resource", server.resource.as_str())
        .append_pair("state", &state);
    open(&authorization);
    let code = tokio::time::timeout(
        request.timeout,
        callback(listener, request.redirect_uri.path(), &state),
    )
    .await
    .map_err(|_| {
        anyhow!(
            "no sign-in reached {} within {} seconds",
            request.redirect_uri,
            request.timeout.as_secs()
        )
    })??;
    let requested_at = Utc::now();
    let form = [
        ("grant_type", "authorization_code"),
        ("client_id", request.client.as_str()),
        ("code", code.as_str()),
        ("redirect_uri", request.redirect_uri.as_str()),
        ("code_verifier", verifier.as_str()),
        ("resource", server.resource.as_str()),
    ];
    let response = oauth::token_request(&http, &server.token_endpoint, &form)
        .await
        .map_err(|failure| match failure {
            TokenFailure::Refused(refusal) => anyhow!(
                "the Hub refused the sign-in's code: {}",
                oauth::refusal_text(&refusal)
            ),
            other => anyhow!("the sign-in's code was not exchanged: {other}"),
        })?;
    let grant = Grant::issued(
        &access.hub,
        &access.profile,
        &request.client,
        &server,
        response,
        requested_at,
    );
    let _lock = grants.lock(&access.hub, &access.profile).await?;
    let path = grants.replace(&grant)?;
    Ok(SignedIn {
        path,
        access_token_expires_at: grant.access_token_expires_at,
        refresh_token_expires_at: grant.refresh_token_expires_at,
    })
}

/// What signing out did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignedOut {
    /// There was no grant to delete.
    NotSignedIn,
    /// The grant file is deleted.
    Deleted {
        path: PathBuf,
        revocation: Revocation,
    },
}

/// Whether the Hub revoked a deleted grant's refresh token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Revocation {
    Revoked,
    /// The grant held no refresh token, or the Hub names no revocation endpoint.
    Unavailable,
    /// The Hub did not confirm the revocation; the token stays valid until it expires.
    Failed(String),
}

/// Revokes the stored grant's refresh token at the Hub and deletes the grant, whether or
/// not the Hub confirms. An unreadable grant file is deleted without revocation.
pub async fn sign_out(access: &HubAccess, grants: &GrantStore) -> Result<SignedOut> {
    let _lock = grants.lock(&access.hub, &access.profile).await?;
    let grant = grants.load(&access.hub, &access.profile).ok().flatten();
    let revocation = match &grant {
        Some(Grant {
            refresh_token: Some(refresh),
            revocation_endpoint: Some(endpoint),
            client_id,
            resource,
            ..
        }) => {
            let form = [
                ("token", refresh.expose()),
                ("token_type_hint", "refresh_token"),
                ("client_id", client_id.as_str()),
                ("resource", resource.as_str()),
            ];
            match oauth::revoke(&access.oauth_http()?, endpoint, &form).await {
                Ok(()) => Revocation::Revoked,
                Err(reason) => Revocation::Failed(reason),
            }
        }
        _ => Revocation::Unavailable,
    };
    if !grants.delete(&access.hub, &access.profile)? {
        return Ok(SignedOut::NotSignedIn);
    }
    Ok(SignedOut::Deleted {
        path: grants.path(&access.hub, &access.profile),
        revocation,
    })
}

/// The loopback address a redirect returns to: plain HTTP to an IP literal with a port,
/// RFC 8252 §7.3 and §8.3.
fn loopback_address(redirect: &Url) -> Result<SocketAddr> {
    ensure!(
        redirect.scheme() == "http",
        "the redirect URI {redirect} must use http to a loopback address"
    );
    let ip = match redirect.host() {
        Some(url::Host::Ipv4(ip)) => IpAddr::V4(ip),
        Some(url::Host::Ipv6(ip)) => IpAddr::V6(ip),
        _ => {
            bail!("the redirect URI {redirect} must name a loopback IP address, such as 127.0.0.1")
        }
    };
    ensure!(
        ip.is_loopback(),
        "the redirect URI {redirect} must name a loopback IP address"
    );
    let port = redirect
        .port()
        .with_context(|| format!("the redirect URI {redirect} must name its port"))?;
    ensure!(
        redirect.query().is_none() && redirect.fragment().is_none(),
        "the redirect URI {redirect} must carry no query or fragment"
    );
    Ok(SocketAddr::new(ip, port))
}

/// The authorization response that reaches `path` with this sign-in's `state`.
enum Outcome {
    Code(String),
    Refused {
        error: String,
        description: Option<String>,
    },
}

/// Answers each connection to the listener until one carries this sign-in's response.
/// Requests for other paths or with another `state` are answered and ignored, so a stale
/// tab or a stray request cannot end or hijack the sign-in.
async fn callback(listener: TcpListener, path: &str, state: &str) -> Result<String> {
    let expected = Arc::new((path.to_owned(), state.to_owned()));
    let (outcomes, mut received) = mpsc::channel(4);
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, _) = accepted.context("the sign-in's redirect listener failed")?;
                let outcomes = outcomes.clone();
                let expected = expected.clone();
                tokio::spawn(async move {
                    if let Some(outcome) = answer(stream, &expected.0, &expected.1).await {
                        let _ = outcomes.send(outcome).await;
                    }
                });
            }
            Some(outcome) = received.recv() => {
                return match outcome {
                    Outcome::Code(code) => Ok(code),
                    Outcome::Refused { error, description } => Err(anyhow!(
                        "the Hub did not sign you in: {error}{}",
                        description.map(|text| format!(" ({text})")).unwrap_or_default()
                    )),
                };
            }
        }
    }
}

const SIGNED_IN_PAGE: &str =
    "The sign-in reached veoveo-hub-sdk-cli. You can close this window and return to the terminal.";
const REFUSED_PAGE: &str =
    "The Hub did not sign you in. veoveo-hub-sdk-cli shows why in the terminal.";
const FOREIGN_PAGE: &str =
    "This address does not belong to the sign-in veoveo-hub-sdk-cli is waiting for.";

async fn answer(mut stream: TcpStream, path: &str, state: &str) -> Option<Outcome> {
    let head = tokio::time::timeout(CALLBACK_READ_TIMEOUT, read_head(&mut stream))
        .await
        .ok()?
        .ok()?;
    let Some(target) = request_target(&head) else {
        respond(&mut stream, "400 Bad Request", FOREIGN_PAGE).await;
        return None;
    };
    let Ok(url) = Url::parse("http://127.0.0.1").and_then(|base| base.join(target)) else {
        respond(&mut stream, "400 Bad Request", FOREIGN_PAGE).await;
        return None;
    };
    if url.path() != path {
        respond(&mut stream, "404 Not Found", FOREIGN_PAGE).await;
        return None;
    }
    let mut parameters: HashMap<String, Vec<String>> = HashMap::new();
    for (name, value) in url.query_pairs() {
        parameters
            .entry(name.into_owned())
            .or_default()
            .push(value.into_owned());
    }
    // RFC 6749 §3.1: each parameter appears at most once.
    let single = |name: &str| match parameters.get(name).map(Vec::as_slice) {
        Some([value]) => Some(value.as_str()),
        _ => None,
    };
    if single("state") != Some(state) {
        respond(&mut stream, "400 Bad Request", FOREIGN_PAGE).await;
        return None;
    }
    if let Some(error) = single("error") {
        respond(&mut stream, "200 OK", REFUSED_PAGE).await;
        return Some(Outcome::Refused {
            error: oauth::printable(error),
            description: single("error_description").map(oauth::printable),
        });
    }
    match single("code") {
        Some(code) => {
            respond(&mut stream, "200 OK", SIGNED_IN_PAGE).await;
            Some(Outcome::Code(code.to_owned()))
        }
        None => {
            respond(&mut stream, "400 Bad Request", REFUSED_PAGE).await;
            Some(Outcome::Refused {
                error: "invalid_request".into(),
                description: Some("the redirect carried no code".into()),
            })
        }
    }
}

/// The request line and headers, up to the blank line that ends them.
async fn read_head(stream: &mut TcpStream) -> std::io::Result<Vec<u8>> {
    let mut head = Vec::new();
    let mut buffer = [0_u8; 2048];
    while !head.windows(4).any(|window| window == b"\r\n\r\n") {
        let read = stream.read(&mut buffer).await?;
        if read == 0 || head.len() + read > CALLBACK_HEAD_LIMIT {
            return Err(std::io::ErrorKind::InvalidData.into());
        }
        head.extend_from_slice(&buffer[..read]);
    }
    Ok(head)
}

/// The origin-form target of a `GET` request line.
fn request_target(head: &[u8]) -> Option<&str> {
    let line = head.split(|byte| *byte == b'\r').next()?;
    let line = std::str::from_utf8(line).ok()?;
    let mut parts = line.split(' ');
    let (Some("GET"), Some(target), Some(version), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return None;
    };
    (target.starts_with('/') && version.starts_with("HTTP/1.")).then_some(target)
}

async fn respond(stream: &mut TcpStream, status: &str, text: &str) {
    let body = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>veoveo-hub-sdk-cli</title></head><body><p>{text}</p></body></html>"
    );
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.shutdown().await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redirects_return_to_a_loopback_ip_and_port() {
        let address =
            loopback_address(&Url::parse("http://127.0.0.1:3211/callback").unwrap()).unwrap();
        assert_eq!(address, "127.0.0.1:3211".parse().unwrap());
        assert!(loopback_address(&Url::parse("http://[::1]:3211/callback").unwrap()).is_ok());
        for refused in [
            "https://127.0.0.1:3211/callback",
            "http://localhost:3211/callback",
            "http://10.0.0.1:3211/callback",
            "http://127.0.0.1/callback",
            "http://127.0.0.1:3211/callback?x=1",
        ] {
            assert!(
                loopback_address(&Url::parse(refused).unwrap()).is_err(),
                "{refused}"
            );
        }
    }

    #[test]
    fn only_a_get_request_line_has_a_target() {
        assert_eq!(
            request_target(b"GET /callback?code=a&state=b HTTP/1.1\r\nHost: x\r\n\r\n"),
            Some("/callback?code=a&state=b")
        );
        assert_eq!(request_target(b"POST /callback HTTP/1.1\r\n\r\n"), None);
        assert_eq!(request_target(b"GET http://evil/ HTTP/1.1\r\n\r\n"), None);
        assert_eq!(request_target(b"GET /a b HTTP/1.1\r\n\r\n"), None);
    }
}
