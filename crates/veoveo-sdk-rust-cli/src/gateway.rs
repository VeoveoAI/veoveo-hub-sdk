//! How the CLI reaches an installation's Gateway as one of its OAuth clients: the SDK's
//! `GatewayClient` for the profile's MCP endpoint, with the caller's own
//! client-credentials bearer, signed by the caller's key.

use std::{net::SocketAddr, path::PathBuf};

use anyhow::{Context, Result, anyhow};
use url::Url;
use veoveo_sdk_rust::{
    GatewayProfileId,
    client::{ClientCredentialsBearer, GatewayClient, caller_signer},
};
use veoveo_types::{OAuthClientId, ScopeName};

/// One installation caller and where its Gateway answers.
#[derive(Debug, Clone)]
pub struct GatewayAccess {
    /// The Gateway's public origin, such as `http://vv-gateway.internal:8080`. Tokens and
    /// assertions name it, whatever address the requests travel to.
    pub gateway: Url,
    /// The address the origin's host reaches, for a host that cannot resolve it, such as
    /// `127.0.0.1:8080` where a local installation publishes its edge.
    pub connect: Option<SocketAddr>,
    /// The directory holding `<caller>.pem` and `<caller>.jwks.json`.
    pub keys: PathBuf,
    pub caller: OAuthClientId,
    /// The Gateway profile whose MCP endpoint the caller uses.
    pub profile: GatewayProfileId,
    /// The OAuth scope the caller requests.
    pub scope: ScopeName,
}

impl GatewayAccess {
    /// The profile's MCP endpoint, `{gateway}/mcp/{profile}`.
    pub fn endpoint(&self) -> Result<Url> {
        let mut endpoint = self.gateway.clone();
        endpoint
            .path_segments_mut()
            .map_err(|()| anyhow!("the Gateway origin {} cannot hold a path", self.gateway))?
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
                .gateway
                .host_str()
                .context("the Gateway origin names no host")?;
            builder = builder.resolve(host, address);
        }
        Ok(builder.build()?)
    }

    /// A client of the profile that acts as the caller.
    pub async fn client(&self) -> Result<GatewayClient> {
        let http = self.http()?;
        let endpoint = self.endpoint()?;
        let signer = caller_signer(&self.keys, &self.caller)?;
        let bearer = ClientCredentialsBearer::discover(
            &endpoint,
            self.caller.clone(),
            signer,
            self.scope.clone(),
            http.clone(),
        )
        .await?;
        Ok(GatewayClient::new(endpoint, http, bearer)
            .named("veoveo-sdk-rust-cli", env!("CARGO_PKG_VERSION")))
    }
}
