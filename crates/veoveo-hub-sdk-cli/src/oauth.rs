//! The OAuth requests of a person's sign-in: discovery of the profile's authorization
//! server, the token endpoint's authorization-code and refresh-token grants and RFC 7009
//! revocation. Every request goes through a client that follows no redirect, so no code,
//! verifier or refresh token is sent to an address the metadata did not name.

use std::{fmt, time::Duration};

use anyhow::{Context, Result, anyhow, bail, ensure};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::{
    StatusCode,
    header::{ACCEPT, CONTENT_TYPE},
};
use serde::Deserialize;
use sha2::{Digest as _, Sha256};
use url::Url;
use veoveo_hub_sdk::{calls::TokenErrorResponse, registration::oauth::AuthorizationServerMetadata};

/// The longest one discovery, token or revocation request may take.
pub(crate) const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

/// RFC 9728 protected resource metadata, the members sign-in reads.
#[derive(Deserialize)]
struct ProtectedResourceMetadata {
    resource: Url,
    authorization_servers: Vec<Url>,
}

/// RFC 8414 authorization server metadata, the members sign-in reads.
#[derive(Deserialize)]
struct ServerMetadata {
    issuer: Url,
    #[serde(default)]
    authorization_endpoint: Option<Url>,
    token_endpoint: Url,
    #[serde(default)]
    revocation_endpoint: Option<Url>,
    #[serde(default)]
    code_challenge_methods_supported: Vec<String>,
}

/// Where a profile's browser sign-in happens.
#[derive(Debug, Clone)]
pub struct AuthorizationServer {
    /// The profile's protected resource, which tokens name as their audience.
    pub resource: Url,
    pub issuer: Url,
    pub authorization_endpoint: Url,
    pub token_endpoint: Url,
    pub revocation_endpoint: Option<Url>,
}

/// The authorization server of the profile whose MCP endpoint is `endpoint`: the
/// endpoint's RFC 9728 metadata names its issuer, and the issuer's RFC 8414 metadata
/// names the authorization, token and revocation endpoints.
pub async fn discover(
    http: &reqwest::Client,
    hub: &Url,
    endpoint: &Url,
) -> Result<AuthorizationServer> {
    let protected: ProtectedResourceMetadata =
        fetch(http, &protected_resource_metadata_url(endpoint)).await?;
    let issuer = protected
        .authorization_servers
        .first()
        .context("the profile's metadata names no authorization server")?;
    let metadata_url = AuthorizationServerMetadata::url_for(issuer);
    let server: ServerMetadata = fetch(http, &metadata_url).await?;
    ensure!(
        &server.issuer == issuer,
        "{metadata_url} names the issuer {}, not {issuer}",
        server.issuer
    );
    let authorization_endpoint = server
        .authorization_endpoint
        .context("the Hub offers no browser sign-in for this profile")?;
    ensure!(
        server
            .code_challenge_methods_supported
            .iter()
            .any(|method| method == "S256"),
        "the Hub offers no PKCE S256 sign-in for this profile"
    );
    for (member, url) in [
        ("authorization_endpoint", Some(&authorization_endpoint)),
        ("token_endpoint", Some(&server.token_endpoint)),
        ("revocation_endpoint", server.revocation_endpoint.as_ref()),
    ] {
        if let Some(url) = url {
            endpoint_url(hub, member, url)?;
        }
    }
    Ok(AuthorizationServer {
        resource: protected.resource,
        issuer: server.issuer,
        authorization_endpoint,
        token_endpoint: server.token_endpoint,
        revocation_endpoint: server.revocation_endpoint,
    })
}

/// An endpoint uses HTTPS, or HTTP when the Hub's own origin does, and carries no
/// credentials or fragment.
fn endpoint_url(hub: &Url, member: &str, url: &Url) -> Result<()> {
    match url.scheme() {
        "https" => {}
        "http" if hub.scheme() == "http" => {}
        scheme => bail!("the Hub's {member} uses {scheme}; sign-in needs https"),
    }
    ensure!(
        url.host_str().is_some_and(|host| !host.is_empty())
            && url.username().is_empty()
            && url.password().is_none()
            && url.fragment().is_none(),
        "the Hub's {member} must name a host without credentials or a fragment"
    );
    Ok(())
}

fn protected_resource_metadata_url(resource: &Url) -> Url {
    let mut url = resource.clone();
    let path = resource.path().trim_end_matches('/');
    url.set_path(&format!("/.well-known/oauth-protected-resource{path}"));
    url.set_query(None);
    url.set_fragment(None);
    url
}

async fn fetch<T: serde::de::DeserializeOwned>(http: &reqwest::Client, url: &Url) -> Result<T> {
    let response = http
        .get(url.clone())
        .header(ACCEPT, "application/json")
        .timeout(REQUEST_TIMEOUT)
        .send()
        .await
        .map_err(|error| anyhow!("discovery at {url} failed: {}", describe(&error)))?;
    let status = response.status();
    ensure!(
        status == StatusCode::OK,
        "discovery at {url} answered HTTP {status}"
    );
    let body = response
        .bytes()
        .await
        .map_err(|error| anyhow!("discovery at {url} failed: {}", describe(&error)))?;
    serde_json::from_slice(&body).with_context(|| format!("{url} answered with no metadata"))
}

/// An RFC 6749 §5.1 token response.
#[derive(Deserialize)]
pub(crate) struct TokenResponse {
    pub(crate) access_token: String,
    pub(crate) token_type: String,
    pub(crate) expires_in: u64,
    #[serde(default)]
    pub(crate) scope: Option<String>,
    #[serde(default)]
    pub(crate) refresh_token: Option<String>,
    #[serde(default)]
    pub(crate) refresh_token_expires_in: Option<u64>,
}

impl fmt::Debug for TokenResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TokenResponse")
            .field("token_type", &self.token_type)
            .field("expires_in", &self.expires_in)
            .field("scope", &self.scope)
            .field("refresh_token_expires_in", &self.refresh_token_expires_in)
            .finish_non_exhaustive()
    }
}

/// Why a token request produced no token, told apart by whether the request may have
/// reached the token endpoint: a refresh token sent there may be spent.
#[derive(Debug)]
pub(crate) enum TokenFailure {
    /// No connection to the endpoint was made, so the request never left this host.
    Unsent(String),
    /// The endpoint answered with an RFC 6749 §5.2 error.
    Refused(TokenErrorResponse),
    /// The request may have reached the endpoint, but no usable answer arrived.
    Unconfirmed(String),
}

impl fmt::Display for TokenFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsent(reason) => write!(f, "the token endpoint is unreachable: {reason}"),
            Self::Refused(refusal) => write!(f, "the Hub refused: {}", refusal_text(refusal)),
            Self::Unconfirmed(reason) => write!(f, "no answer arrived: {reason}"),
        }
    }
}

/// An OAuth error as its RFC 6749 §5.2 code and description.
pub(crate) fn refusal_text(refusal: &TokenErrorResponse) -> String {
    let code = serde_json::to_value(refusal.error)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| format!("{:?}", refusal.error));
    match refusal.error_description.as_deref() {
        Some(description) => format!("{code} ({})", printable(description)),
        None => code,
    }
}

/// RFC 6749 §5.2 limits error text to printable ASCII; anything else is dropped before
/// it reaches a terminal.
pub(crate) fn printable(text: &str) -> String {
    text.chars()
        .filter(|character| matches!(character, ' '..='~'))
        .take(300)
        .collect()
}

/// One form POST to a token endpoint.
pub(crate) async fn token_request(
    http: &reqwest::Client,
    endpoint: &Url,
    form: &[(&str, &str)],
) -> Result<TokenResponse, TokenFailure> {
    let body = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(form)
        .finish();
    let response = http
        .post(endpoint.clone())
        .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header(ACCEPT, "application/json")
        .timeout(REQUEST_TIMEOUT)
        .body(body)
        .send()
        .await
        .map_err(|error| {
            if error.is_connect() {
                TokenFailure::Unsent(describe(&error))
            } else {
                TokenFailure::Unconfirmed(describe(&error))
            }
        })?;
    let status = response.status();
    let body = response
        .bytes()
        .await
        .map_err(|error| TokenFailure::Unconfirmed(describe(&error)))?;
    if status != StatusCode::OK {
        return Err(match serde_json::from_slice::<TokenErrorResponse>(&body) {
            Ok(refusal) => TokenFailure::Refused(refusal),
            Err(_) => {
                TokenFailure::Unconfirmed(format!("the token endpoint answered HTTP {status}"))
            }
        });
    }
    let token: TokenResponse = serde_json::from_slice(&body).map_err(|_| {
        TokenFailure::Unconfirmed("the token endpoint's answer holds no token".into())
    })?;
    if !token.token_type.eq_ignore_ascii_case("bearer") {
        return Err(TokenFailure::Unconfirmed(format!(
            "the token endpoint issued a {} token, not a Bearer token",
            printable(&token.token_type)
        )));
    }
    Ok(token)
}

/// RFC 7009 revocation of a refresh token by a public client.
pub(crate) async fn revoke(
    http: &reqwest::Client,
    endpoint: &Url,
    form: &[(&str, &str)],
) -> Result<(), String> {
    let body = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(form)
        .finish();
    let response = http
        .post(endpoint.clone())
        .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header(ACCEPT, "application/json")
        .timeout(REQUEST_TIMEOUT)
        .body(body)
        .send()
        .await
        .map_err(|error| describe(&error))?;
    let status = response.status();
    if status == StatusCode::OK {
        return Ok(());
    }
    let body = response.bytes().await.unwrap_or_default();
    Err(match serde_json::from_slice::<TokenErrorResponse>(&body) {
        Ok(refusal) => refusal_text(&refusal),
        Err(_) => format!("the revocation endpoint answered HTTP {status}"),
    })
}

/// 32 bytes from the TLS provider's secure random source, base64url without padding:
/// 43 characters, as RFC 7636 §4.1 recommends for a verifier.
pub(crate) fn random_value() -> Result<String> {
    let mut bytes = [0_u8; 32];
    rustls::crypto::ring::default_provider()
        .secure_random
        .fill(&mut bytes)
        .map_err(|_| anyhow!("the secure random source failed"))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

/// RFC 7636 §4.2 `S256`: the base64url SHA-256 of the verifier's ASCII.
pub fn s256_challenge(verifier: &str) -> String {
    let digest: [u8; 32] = Sha256::digest(verifier.as_bytes()).into();
    URL_SAFE_NO_PAD.encode(digest)
}

/// A reqwest error and its causes, which name addresses but never request bodies.
pub(crate) fn describe(error: &reqwest::Error) -> String {
    let mut text = error.to_string();
    let mut source = std::error::Error::source(error);
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_challenge_is_rfc_7636s_s256() {
        // RFC 7636 Appendix B.
        assert_eq!(
            s256_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn random_values_are_verifier_shaped_and_distinct() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let first = random_value().unwrap();
        assert_eq!(first.len(), 43);
        assert!(
            first
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
        );
        assert_ne!(first, random_value().unwrap());
    }

    #[test]
    fn endpoints_use_https_unless_the_hub_itself_is_http() {
        let public = Url::parse("https://hub.example/").unwrap();
        let local = Url::parse("http://vv-hub.internal:8080/").unwrap();
        let plain = Url::parse("http://hub.example/oauth/token").unwrap();
        assert!(endpoint_url(&public, "token_endpoint", &plain).is_err());
        assert!(endpoint_url(&local, "token_endpoint", &plain).is_ok());
        let file = Url::parse("file:///etc/passwd").unwrap();
        assert!(endpoint_url(&local, "authorization_endpoint", &file).is_err());
    }

    #[test]
    fn error_text_keeps_only_printable_ascii() {
        assert_eq!(printable("bad\u{1b}[31m grant\n"), "bad[31m grant");
    }
}
