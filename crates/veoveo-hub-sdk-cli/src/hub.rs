//! How the CLI reaches an installation's Hub as one of its OAuth clients: the SDK's
//! `HubClient` for the profile's MCP endpoint, with the caller's own
//! client-credentials bearer or a bearer supplied by the environment.

use std::{net::SocketAddr, path::PathBuf};

use anyhow::{Context, Result, anyhow};
use url::Url;
use veoveo_hub_sdk::{
    HubProfileId,
    client::{ClientCredentialsBearer, HubClient, StaticBearer, caller_signer},
};
use veoveo_types::{OAuthClientId, ScopeName};

/// One installation caller and where its Hub answers.
#[derive(Debug, Clone)]
pub struct HubAccess {
    /// The Hub's public origin, such as `http://vv-hub.internal:8080`. Tokens and
    /// assertions name it, whatever address the requests travel to.
    pub hub: Url,
    /// The address the origin's host reaches, for a host that cannot resolve it, such as
    /// `127.0.0.1:8080` where a local installation publishes its edge.
    pub connect: Option<SocketAddr>,
    pub authentication: HubAuthentication,
    /// The Hub profile whose MCP endpoint the caller uses.
    pub profile: HubProfileId,
}

/// Credential acquisition belongs to the caller or its trusted runtime.
#[derive(Debug, Clone)]
pub enum HubAuthentication {
    ClientCredentials {
        keys: PathBuf,
        caller: OAuthClientId,
        scope: ScopeName,
    },
    /// Names a variable, never a token value. Supports native opaque references.
    BearerFromEnvironment { variable: String },
}

impl HubAccess {
    /// The profile's MCP endpoint, `{hub}/mcp/{profile}`.
    pub fn endpoint(&self) -> Result<Url> {
        let mut endpoint = self.hub.clone();
        endpoint
            .path_segments_mut()
            .map_err(|()| anyhow!("the Hub origin {} cannot hold a path", self.hub))?
            .pop_if_empty()
            .extend(["mcp", self.profile.as_str()]);
        Ok(endpoint)
    }

    /// An HTTP client that sends requests for the origin's host to `connect` when it is
    /// set, keeping the origin's name in each request.
    fn http(&self) -> Result<reqwest::Client> {
        let mut builder = reqwest::Client::builder();
        if let Some(address) = self.connect {
            let host = self
                .hub
                .host_str()
                .context("the Hub origin names no host")?;
            builder = builder.resolve(host, address);
        }
        Ok(builder.build()?)
    }

    /// A client of the profile that acts as the caller.
    pub async fn client(&self) -> Result<HubClient> {
        let http = self.http()?;
        let endpoint = self.endpoint()?;
        let client = match &self.authentication {
            HubAuthentication::BearerFromEnvironment { variable } => {
                let token = std::env::var(variable)
                    .ok()
                    .filter(|value| !value.trim().is_empty())
                    .with_context(|| format!("{variable} must hold the Hub bearer"))?;
                HubClient::new(endpoint, http, StaticBearer::new(token))
            }
            HubAuthentication::ClientCredentials {
                keys,
                caller,
                scope,
            } => {
                let signer = caller_signer(keys, caller)?;
                let bearer = ClientCredentialsBearer::discover(
                    &endpoint,
                    caller.clone(),
                    signer,
                    scope.clone(),
                    http.clone(),
                )
                .await?;
                HubClient::new(endpoint, http, bearer)
            }
        };
        Ok(client.named("veoveo-hub-sdk-cli", env!("CARGO_PKG_VERSION")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn an_absent_bearer_fails_before_contacting_the_hub() {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let variable = format!("VEOVEO_MISSING_{}", uuid::Uuid::new_v4().simple());
        let access = HubAccess {
            hub: Url::parse("http://127.0.0.1:1").unwrap(),
            connect: None,
            authentication: HubAuthentication::BearerFromEnvironment {
                variable: variable.clone(),
            },
            profile: HubProfileId::parse("local").unwrap(),
        };
        let Err(error) = access.client().await else {
            panic!("missing bearer must be rejected");
        };
        assert_eq!(
            error.to_string(),
            format!("{variable} must hold the Hub bearer")
        );
    }
}
