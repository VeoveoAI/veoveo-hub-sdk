//! In-process hub for testing a hosted server without a deployment.
//!
//! Enable the `testing` feature in a server's dev-dependencies. [`for_domain`]
//! starts the ordinary [`HostedServer`] builder with a fixed test deployment and
//! the test signing key's trust bundle. The caller adds its handler and any
//! server routes, builds, and wraps the result in a [`TestHub`]. Requests run
//! through the complete router: host validation, hub authentication, the
//! stateless MCP transport and the domain.
//!
//! ```ignore
//! let hub = TestHub::new(
//!     testing::for_domain::<MyDomain>()
//!         .handler(|| Hosted::new(MyDomain::new()))
//!         .build(),
//! );
//! let tools = hub.rpc("tools/list", json!({})).await;
//! assert_eq!(tools["result"]["tools"][0]["name"], "my_tool");
//! ```
//!
//! The signing key is a published test key. Production servers trust only the
//! keys their registration response and the Hub's JWK Set name, which never
//! contain it. The test deployment never refreshes its keys. [`TestHub::granting`]
//! sets the permissions its tokens carry.

use std::{collections::BTreeSet, sync::Arc};

mod mesh;
pub use mesh::{MESH_PROFILE, RunningMesh, TestMesh};

use crate::registration::{AccessTokenTrust, Ed25519JwkSet, ResourceIndicator};
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use chrono::{TimeDelta, Utc};
use tower::ServiceExt;
use veoveo_types::{
    AccessSubject, GroupId, InvocationAuthority, InvocationProvenance, PermissionName,
    PolicyVersion, PrincipalId, RoleId, ScopeName, TenantId, WorkContextId,
    WorkContextMembershipLevel, WorkContextOutputPolicy,
};

use super::{
    DomainServer, HostedServer, HostedServerBuilder, Missing, Provided, server::Deployment,
};
use crate::{
    ForwardedRequest, ForwardedTokenVerifier, HubInternalSigningKey, HubInternalTokenIssuer,
    HubInternalTokenVerifier, HubInternalTrustBundle, HubProfileId, OAuthClientId, Principal,
    PrincipalKind, PublicDeployment, TokenIssuer, TokenSubject,
};

/// The host authority of the test deployment.
pub const TEST_HOST: &str = "veoveo.test";
/// The protocol version test requests declare.
pub const TEST_PROTOCOL_VERSION: &str = "2026-07-28";
/// The test Hub's issuer URL.
pub const TEST_ISSUER: &str = "https://hub.veoveo.test";
/// The OAuth client the test user calls through.
pub const TEST_CLIENT: &str = "test-client";
/// The resource URL test tokens are addressed to.
pub const TEST_RESOURCE: &str = "https://veoveo.test/mcp";

const PRIVATE_KEY_DER_B64: &str =
    "MC4CAQAwBQYDK2VwBCIEII4AsVspz8h7mpqvOkgslJP07HfqpiWMZA+6Ii90lVBl";

/// The test signing key, named by its RFC 7638 thumbprint.
pub fn signing_key() -> HubInternalSigningKey {
    HubInternalSigningKey::new(STANDARD.decode(PRIVATE_KEY_DER_B64).unwrap()).unwrap()
}

fn test_keys() -> Ed25519JwkSet {
    Ed25519JwkSet::new(vec![signing_key().verification_key()]).unwrap()
}

/// A trust bundle holding the test public key.
pub fn trust_bundle() -> HubInternalTrustBundle {
    HubInternalTrustBundle::from_key_set(&test_keys()).unwrap()
}

/// Fixed deployment identity of the in-process fixture, never an installation identity.
pub fn deployment_id() -> crate::DeploymentId {
    crate::DeploymentId::parse("019a05cb-6c00-7000-8712-3456789abcde")
        .expect("fixture deployment UUIDv7")
}

/// The fixture's captured execution selection.
pub fn execution_context() -> crate::ExecutionContext {
    crate::ResolvedComposition::new(
        crate::EnvironmentId::parse("019a05cb-6c00-7000-8712-3456789abcdf")
            .expect("fixture environment UUIDv7"),
        Default::default(),
    )
    .context()
}

/// The test Hub's issuer, key and audience, as a registration response names them.
pub fn access_token_trust() -> AccessTokenTrust {
    AccessTokenTrust {
        deployment: deployment_id(),
        issuer: TokenIssuer::parse(TEST_ISSUER).unwrap(),
        jwks: test_keys(),
        resource: ResourceIndicator::parse(TEST_RESOURCE).unwrap(),
    }
}

/// The test user: `user-1` in `tenant-a`, operator, without data labels.
pub fn principal() -> Principal {
    Principal {
        id: PrincipalId::parse("https://idp.example.com#user-1").unwrap(),
        kind: PrincipalKind::User,
        issuer: TokenIssuer::parse("https://idp.example.com").unwrap(),
        subject: TokenSubject::parse("user-1").unwrap(),
        tenant: Some(TenantId::parse("tenant-a").unwrap()),
        groups: BTreeSet::from([GroupId::parse("engineering").unwrap()]),
        group_roles: BTreeSet::new(),
        roles: BTreeSet::from([RoleId::parse("operator").unwrap()]),
        scopes: BTreeSet::from([ScopeName::parse("operator:use").unwrap()]),
        data_labels: BTreeSet::new(),
        assurances: BTreeSet::new(),
        authenticated_at: Some(Utc::now()),
    }
}

/// The test user's direct invocation authority as owner of work context `mission`.
pub fn authority() -> InvocationAuthority {
    InvocationAuthority {
        work_context: WorkContextId::parse("mission").unwrap(),
        tenant: TenantId::parse("tenant-a").unwrap(),
        membership: WorkContextMembershipLevel::Owner,
        policy_revision: PolicyVersion::parse("r1").unwrap(),
        output_policy: WorkContextOutputPolicy {
            owner: AccessSubject::Principal(principal().id),
            initial_grants: Vec::new(),
            classification: None,
            data_labels: BTreeSet::new(),
        },
        provenance: InvocationProvenance::Direct {
            initiator: principal().id,
        },
    }
}

/// A verifier of `D`'s tokens that holds the test key and never refreshes it.
pub(crate) fn test_verifier<D: DomainServer>() -> ForwardedTokenVerifier {
    ForwardedTokenVerifier::fixed(
        HubInternalTokenVerifier::from_access_tokens(
            &access_token_trust(),
            crate::server_contract::provider_slug::<D::Contract>(),
        )
        .unwrap(),
    )
}

/// Starts the builder for `D` on the test deployment, trusting the test key.
pub fn for_domain<D: DomainServer>()
-> HostedServerBuilder<D, Provided<Deployment>, Provided<Arc<ForwardedTokenVerifier>>, Missing> {
    let deployment = PublicDeployment::new(format!("https://{TEST_HOST}")).unwrap();
    HostedServer::for_domain::<D>()
        .deployment(&deployment, false)
        .unwrap()
        .with_verifier(test_verifier::<D>())
}

/// Starts the builder for `D` under `/{slug}` on the test host as an internal
/// server, trusting the test key.
pub fn internal_for_domain<D: DomainServer>()
-> HostedServerBuilder<D, Provided<Deployment>, Provided<Arc<ForwardedTokenVerifier>>, Missing> {
    HostedServer::for_domain::<D>()
        .internal([TEST_HOST.to_owned()])
        .with_verifier(test_verifier::<D>())
}

/// Starts the builder for `D` at the internal root on the test host, trusting
/// the test key.
pub fn internal_root_for_domain<D: DomainServer>()
-> HostedServerBuilder<D, Provided<Deployment>, Provided<Arc<ForwardedTokenVerifier>>, Missing> {
    HostedServer::for_domain::<D>()
        .internal_root([TEST_HOST.to_owned()])
        .with_verifier(test_verifier::<D>())
}

/// A built server and the hub calls that reach it.
pub struct TestHub {
    router: Router,
    slug: crate::ServerSlug,
    mount: String,
    permissions: BTreeSet<PermissionName>,
}

impl TestHub {
    pub fn new(server: HostedServer) -> Self {
        Self {
            slug: server.slug.clone(),
            mount: server.mount.prefix().to_owned(),
            router: server.router,
            permissions: BTreeSet::new(),
        }
    }

    /// Grants `permissions` in every token this hub issues, as role evaluation
    /// would. Each must belong to the server's namespace.
    pub fn granting(mut self, permissions: impl IntoIterator<Item = PermissionName>) -> Self {
        self.permissions = permissions.into_iter().collect();
        self
    }

    /// An access token for the test user, addressed to this server and carrying the
    /// permissions set by [`TestHub::granting`].
    pub fn token(&self) -> String {
        self.token_granting(self.permissions.iter().cloned())
    }

    /// An access token for the test user that grants exactly `permissions`.
    pub fn token_granting(&self, permissions: impl IntoIterator<Item = PermissionName>) -> String {
        HubInternalTokenIssuer::new(TokenIssuer::parse(TEST_ISSUER).unwrap(), signing_key())
            .unwrap()
            .issue(ForwardedRequest {
                deployment: deployment_id(),
                profile: HubProfileId::parse("operations").unwrap(),
                server: self.slug.clone(),
                audience: ResourceIndicator::parse(TEST_RESOURCE).unwrap(),
                client: OAuthClientId::parse(TEST_CLIENT).unwrap(),
                actor: principal(),
                authority: authority(),
                permissions: permissions.into_iter().collect(),
                request_context: None,
                acting_server: None,
                acting_task: None,
                delegation_context: None,
                expires_at: Utc::now() + TimeDelta::minutes(5),
            })
            .unwrap()
            .bearer_token
    }

    /// Sends a raw request to the router.
    pub async fn send(&self, request: Request<Body>) -> (StatusCode, String) {
        let response = self.router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, String::from_utf8(body.to_vec()).unwrap())
    }

    /// A request builder for `path` under the server's mount, on the test host.
    pub fn request(&self, path: &str) -> axum::http::request::Builder {
        Self::root_request(&format!("{}{path}", self.mount))
    }

    /// A request builder for `path` at the root of the listener, such as `/livez`, on
    /// the test host.
    pub fn root_request(path: &str) -> axum::http::request::Builder {
        Request::builder().uri(path).header("host", TEST_HOST)
    }

    /// Calls `method` as the test user and returns the JSON-RPC response.
    pub async fn rpc(&self, method: &str, params: serde_json::Value) -> serde_json::Value {
        let token = self.token();
        self.rpc_with(method, params, Some(&token)).await.1
    }

    /// Calls `method` with `bearer`, or with no Authorization header.
    pub async fn rpc_with(
        &self,
        method: &str,
        mut params: serde_json::Value,
        bearer: Option<&str>,
    ) -> (StatusCode, serde_json::Value) {
        params["_meta"] = serde_json::json!({
            "io.modelcontextprotocol/protocolVersion": TEST_PROTOCOL_VERSION,
            "io.modelcontextprotocol/clientInfo": {"name": "test-hub", "version": "1"},
            "io.modelcontextprotocol/clientCapabilities": {}
        });
        let mut request = self
            .request("/mcp")
            .method("POST")
            .header("accept", "application/json, text/event-stream")
            .header("content-type", "application/json")
            .header("mcp-protocol-version", TEST_PROTOCOL_VERSION)
            .header("mcp-method", method);
        if let Some(bearer) = bearer {
            request = request.header("authorization", format!("Bearer {bearer}"));
        }
        if let Some(name) = params["name"].as_str().or(params["uri"].as_str()) {
            request = request.header("mcp-name", name);
        }
        let body =
            serde_json::json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
        let (status, text) = self
            .send(request.body(Body::from(body.to_string())).unwrap())
            .await;
        (
            status,
            serde_json::from_str(&text).unwrap_or(serde_json::Value::Null),
        )
    }
}
