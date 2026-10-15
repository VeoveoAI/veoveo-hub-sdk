//! How the CLI reaches an installation's Gateway as one of its OAuth clients: an RFC
//! 6749 client-credentials token, authenticated with an RFC 7523 assertion signed by the
//! caller's own key, for the profile's MCP endpoint.

use std::{
    net::SocketAddr,
    path::PathBuf,
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;
use url::Url;
use veoveo_oauth_client::{CLIENT_ASSERTION_TYPE, ClientAssertionKeyId, ClientAssertionSigner};
use veoveo_types::{OAuthClientId, ScopeName};

/// The longest a token request may take.
const TOKEN_REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

/// One installation caller and where its Gateway answers.
#[derive(Debug, Clone)]
pub struct GatewayAccess {
    /// The Gateway's public origin, such as `http://vv-gateway.internal:8080`. Tokens and
    /// assertions name it, whatever address the requests travel to.
    pub issuer: Url,
    /// The address the issuer's host reaches, for a host that cannot resolve it, such as
    /// `127.0.0.1:8080` where a local installation publishes its edge.
    pub connect: Option<SocketAddr>,
    /// The directory holding `<caller>.pem` and `<caller>.jwks.json`.
    pub keys: PathBuf,
    pub caller: OAuthClientId,
    /// The Gateway profile whose MCP endpoint the caller uses.
    pub profile: String,
    /// The OAuth scope the caller requests.
    pub scope: ScopeName,
}

/// An access token and when it was issued.
pub struct AccessToken {
    pub value: String,
    pub expires_in: Duration,
    pub issued_at: Instant,
}

impl std::fmt::Debug for AccessToken {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("AccessToken")
            .field("value", &"[REDACTED]")
            .field("expires_in", &self.expires_in)
            .finish_non_exhaustive()
    }
}

impl AccessToken {
    /// Whether less than a third of its lifetime remains.
    pub fn is_stale(&self) -> bool {
        self.issued_at.elapsed() > self.expires_in.saturating_mul(2) / 3
    }
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: u64,
}

impl GatewayAccess {
    /// The profile's MCP endpoint, which is also the token's resource.
    pub fn mcp_endpoint(&self) -> Result<Url> {
        Ok(self.issuer.join(&format!("/mcp/{}", self.profile))?)
    }

    fn token_endpoint(&self) -> Result<Url> {
        Ok(self.issuer.join("/oauth/token")?)
    }

    /// An HTTP client that sends requests for the issuer's host to `connect` when it is
    /// set, keeping the issuer's name in each request.
    pub fn http_client(&self) -> Result<reqwest::Client> {
        let mut builder = reqwest::Client::builder();
        if let Some(address) = self.connect {
            let host = self
                .issuer
                .host_str()
                .context("the Gateway's issuer names no host")?;
            builder = builder.resolve(host, address);
        }
        Ok(builder.build()?)
    }

    /// A client-credentials token for the profile's MCP endpoint.
    pub async fn access_token(&self, http: &reqwest::Client) -> Result<AccessToken> {
        let jwks: serde_json::Value = serde_json::from_slice(
            &std::fs::read(self.keys.join(format!("{}.jwks.json", self.caller)))
                .with_context(|| format!("read the JWK Set of `{}`", self.caller))?,
        )?;
        let key_id = jwks["keys"][0]["kid"]
            .as_str()
            .with_context(|| format!("the JWK Set of `{}` names no key", self.caller))?;
        let pem = std::fs::read(self.keys.join(format!("{}.pem", self.caller)))
            .with_context(|| format!("read the key of `{}`", self.caller))?;
        let signer =
            ClientAssertionSigner::from_rsa_pem(ClientAssertionKeyId::parse(key_id)?, &pem)?;
        let endpoint = self.token_endpoint()?;
        let assertion = signer.assertion(&self.caller, &endpoint)?;
        let form = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("grant_type", "client_credentials")
            .append_pair("client_id", self.caller.as_str())
            .append_pair("client_assertion_type", CLIENT_ASSERTION_TYPE)
            .append_pair("client_assertion", assertion.expose_secret())
            .append_pair("resource", self.mcp_endpoint()?.as_str())
            .append_pair("scope", self.scope.as_str())
            .finish();
        let issued_at = Instant::now();
        let response = http
            .post(endpoint)
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            )
            .body(form)
            .timeout(TOKEN_REQUEST_TIMEOUT)
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            // An OAuth error body names the error and holds no token.
            let body = response.text().await.unwrap_or_default();
            bail!("the Gateway's token endpoint answered {status}: {body}");
        }
        let token: TokenResponse = response.json().await?;
        ensure!(
            !token.access_token.is_empty(),
            "the token endpoint issued an empty token"
        );
        Ok(AccessToken {
            value: token.access_token,
            expires_in: Duration::from_secs(token.expires_in),
            issued_at,
        })
    }
}
