//! A caller's own bearer: an OAuth 2.0 client-credentials token, RFC 6749 section 4.4,
//! for one Gateway profile, requested with an RFC 7523 client assertion signed by the
//! caller's key.

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use futures::future::BoxFuture;
use reqwest::{
    StatusCode,
    header::{ACCEPT, CONTENT_TYPE},
};
use serde::Deserialize;
use veoveo_oauth_client::{
    AssertionError, CLIENT_ASSERTION_TYPE, ClientAssertionKeyId, ClientAssertionSigner,
};
use veoveo_types::{OAuthClientId, ScopeName, ServerSlug};

use super::{Bearer, BearerSource, CallError};
use crate::{calls::TokenErrorResponse, registration::oauth::AuthorizationServerMetadata};

/// The longest one discovery or token request may take.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

/// Why a caller's credentials could not be prepared.
#[derive(Debug, thiserror::Error)]
pub enum CredentialsError {
    #[error("cannot read {path}: {source}")]
    KeyFile {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("{path} names no key: {reason}")]
    KeySet { path: PathBuf, reason: String },
    #[error("the caller's key cannot sign assertions: {0}")]
    Key(#[from] AssertionError),
    #[error("discovery at {url} failed: {reason}")]
    Discovery { url: url::Url, reason: String },
}

/// RFC 9728 protected resource metadata, the members discovery reads.
#[derive(Deserialize)]
struct ProtectedResourceMetadata {
    resource: url::Url,
    authorization_servers: Vec<url::Url>,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: u64,
}

/// A client-credentials bearer for one Gateway profile. It serves the same token for
/// every provider and for profile-wide requests, and requests another once a third of
/// the token's lifetime remains or the Gateway stops admitting it.
#[derive(Clone)]
pub struct ClientCredentialsBearer {
    inner: Arc<Inner>,
}

struct Inner {
    client: OAuthClientId,
    signer: ClientAssertionSigner,
    token_endpoint: url::Url,
    resource: url::Url,
    scope: ScopeName,
    http: reqwest::Client,
    current: Mutex<Option<Issued>>,
    minting: tokio::sync::Mutex<()>,
}

#[derive(Clone)]
struct Issued {
    bearer: Bearer,
    renew_at: Instant,
}

impl std::fmt::Debug for ClientCredentialsBearer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClientCredentialsBearer")
            .field("client", &self.inner.client)
            .field("resource", &self.inner.resource.as_str())
            .finish_non_exhaustive()
    }
}

impl ClientCredentialsBearer {
    /// A bearer for the profile whose MCP endpoint is `endpoint`. The endpoint's RFC
    /// 9728 protected resource metadata names its authorization server, and that
    /// server's RFC 8414 metadata names the token endpoint.
    pub async fn discover(
        endpoint: &url::Url,
        client: OAuthClientId,
        signer: ClientAssertionSigner,
        scope: ScopeName,
        http: reqwest::Client,
    ) -> Result<Self, CredentialsError> {
        let protected: ProtectedResourceMetadata =
            fetch(&http, &protected_resource_metadata_url(endpoint)).await?;
        let issuer =
            protected
                .authorization_servers
                .first()
                .ok_or_else(|| CredentialsError::Discovery {
                    url: protected_resource_metadata_url(endpoint),
                    reason: "the metadata names no authorization server".into(),
                })?;
        let metadata_url = AuthorizationServerMetadata::url_for(issuer);
        let server: AuthorizationServerMetadata = fetch(&http, &metadata_url).await?;
        let discovery = |reason: String| CredentialsError::Discovery {
            url: metadata_url.clone(),
            reason,
        };
        if &server.issuer != issuer {
            return Err(discovery("the metadata names another issuer".into()));
        }
        let token_endpoint = server
            .token_endpoint_url()
            .map_err(|error| discovery(error.to_string()))?
            .clone();
        Ok(Self::new(
            token_endpoint,
            protected.resource,
            client,
            signer,
            scope,
            http,
        ))
    }

    /// A bearer requested from `token_endpoint` for the protected `resource`.
    pub fn new(
        token_endpoint: url::Url,
        resource: url::Url,
        client: OAuthClientId,
        signer: ClientAssertionSigner,
        scope: ScopeName,
        http: reqwest::Client,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                client,
                signer,
                token_endpoint,
                resource,
                scope,
                http,
                current: Mutex::new(None),
                minting: tokio::sync::Mutex::new(()),
            }),
        }
    }

    fn fresh(&self) -> Option<Bearer> {
        self.inner
            .current
            .lock()
            .expect("the bearer lock is never poisoned")
            .as_ref()
            .filter(|issued| Instant::now() < issued.renew_at)
            .map(|issued| issued.bearer.clone())
    }

    async fn current(&self) -> Result<Bearer, CallError> {
        if let Some(bearer) = self.fresh() {
            return Ok(bearer);
        }
        let _minting = self.inner.minting.lock().await;
        // Another request may have minted one while this one waited.
        if let Some(bearer) = self.fresh() {
            return Ok(bearer);
        }
        let requested_at = Instant::now();
        let issued = self.request().await?;
        let lifetime = Duration::from_secs(issued.expires_in);
        let bearer = Bearer::new(issued.access_token);
        *self
            .inner
            .current
            .lock()
            .expect("the bearer lock is never poisoned") = Some(Issued {
            bearer: bearer.clone(),
            renew_at: requested_at + lifetime.saturating_mul(2) / 3,
        });
        Ok(bearer)
    }

    async fn request(&self) -> Result<TokenResponse, CallError> {
        let inner = &self.inner;
        let assertion = inner
            .signer
            .assertion(&inner.client, &inner.token_endpoint)
            .map_err(|error| CallError::Transport(format!("cannot sign the assertion: {error}")))?;
        let body = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("grant_type", "client_credentials")
            .append_pair("client_id", inner.client.as_str())
            .append_pair("client_assertion_type", CLIENT_ASSERTION_TYPE)
            .append_pair("client_assertion", assertion.expose_secret())
            .append_pair("resource", inner.resource.as_str())
            .append_pair("scope", inner.scope.as_str())
            .finish();
        let response = inner
            .http
            .post(inner.token_endpoint.clone())
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
            .header(ACCEPT, "application/json")
            .timeout(REQUEST_TIMEOUT)
            .body(body)
            .send()
            .await
            .map_err(|error| CallError::Transport(format!("the token request failed: {error}")))?;
        let status = response.status();
        let body = response
            .bytes()
            .await
            .map_err(|error| CallError::Transport(format!("the token request failed: {error}")))?;
        if status == StatusCode::OK {
            return Ok(serde_json::from_slice(&body)?);
        }
        match serde_json::from_slice::<TokenErrorResponse>(&body) {
            Ok(refusal) => Err(CallError::Refused {
                error: refusal.error,
                description: refusal.error_description,
            }),
            Err(_) => Err(CallError::Transport(format!(
                "the token endpoint answered HTTP {status}"
            ))),
        }
    }

    fn drop_current(&self) {
        *self
            .inner
            .current
            .lock()
            .expect("the bearer lock is never poisoned") = None;
    }
}

impl BearerSource for ClientCredentialsBearer {
    fn bearer<'a>(&'a self, _: &'a ServerSlug) -> BoxFuture<'a, Result<Bearer, CallError>> {
        Box::pin(self.current())
    }

    fn forget(&self, _: &ServerSlug) {
        self.drop_current();
    }

    fn caller_bearer(&self) -> BoxFuture<'_, Result<Bearer, CallError>> {
        Box::pin(self.current())
    }

    fn forget_caller(&self) {
        self.drop_current();
    }
}

/// The signer of `client`'s assertions from an installation's caller key files:
/// `<client>.pem`, an RSA private key, and `<client>.jwks.json`, whose first key's
/// `kid` names it.
pub fn caller_signer(
    directory: &Path,
    client: &OAuthClientId,
) -> Result<ClientAssertionSigner, CredentialsError> {
    let read = |path: PathBuf| {
        std::fs::read(&path).map_err(|source| CredentialsError::KeyFile { path, source })
    };
    let key_set_path = directory.join(format!("{client}.jwks.json"));
    let key_set: serde_json::Value =
        serde_json::from_slice(&read(key_set_path.clone())?).map_err(|error| {
            CredentialsError::KeySet {
                path: key_set_path.clone(),
                reason: error.to_string(),
            }
        })?;
    let key_id = key_set["keys"][0]["kid"]
        .as_str()
        .ok_or_else(|| CredentialsError::KeySet {
            path: key_set_path.clone(),
            reason: "its first key has no kid".into(),
        })
        .and_then(|kid| {
            ClientAssertionKeyId::parse(kid).map_err(|error| CredentialsError::KeySet {
                path: key_set_path.clone(),
                reason: error.to_string(),
            })
        })?;
    let pem = read(directory.join(format!("{client}.pem")))?;
    Ok(ClientAssertionSigner::from_rsa_pem(key_id, &pem)?)
}

/// RFC 9728 section 3.1: the well-known URL inserted before the resource's path.
fn protected_resource_metadata_url(resource: &url::Url) -> url::Url {
    let mut url = resource.clone();
    let path = resource.path().trim_end_matches('/');
    url.set_path(&format!("/.well-known/oauth-protected-resource{path}"));
    url.set_query(None);
    url.set_fragment(None);
    url
}

async fn fetch<T: serde::de::DeserializeOwned>(
    http: &reqwest::Client,
    url: &url::Url,
) -> Result<T, CredentialsError> {
    let discovery = |reason: String| CredentialsError::Discovery {
        url: url.clone(),
        reason,
    };
    let response = http
        .get(url.clone())
        .header(ACCEPT, "application/json")
        .timeout(REQUEST_TIMEOUT)
        .send()
        .await
        .map_err(|error| discovery(error.to_string()))?;
    let status = response.status();
    if status != StatusCode::OK {
        return Err(discovery(format!("HTTP {status}")));
    }
    let body = response
        .bytes()
        .await
        .map_err(|error| discovery(error.to_string()))?;
    serde_json::from_slice(&body).map_err(|error| discovery(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::protected_resource_metadata_url;

    #[test]
    fn the_metadata_url_inserts_the_well_known_segment_before_the_resource_path() {
        let resource = url::Url::parse("http://gateway.test:8080/mcp/local").unwrap();
        assert_eq!(
            protected_resource_metadata_url(&resource).as_str(),
            "http://gateway.test:8080/.well-known/oauth-protected-resource/mcp/local"
        );
    }
}
