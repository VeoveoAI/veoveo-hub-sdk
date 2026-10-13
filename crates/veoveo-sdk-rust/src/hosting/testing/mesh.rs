//! An in-process Gateway for calls between servers.
//!
//! [`TestMesh`] binds a loopback listener, registers servers with their authorization
//! declarations and gives each the [`ServerCalls`] its handlers call through. Started,
//! it serves the two Gateway endpoints those calls use: the token endpoint, which
//! performs the RFC 8693 exchange with the Gateway's narrowing, and the profile's MCP
//! endpoint, which routes each call to its provider with a delegated forwarded token.
//! The test user reaches a server with [`RunningMesh::call`].
//!
//! ```ignore
//! let mut mesh = TestMesh::bind().await;
//! let calls = mesh.register(relay::authorization()?);
//! mesh.register(ledger::authorization()?);
//! mesh.serve(testing::for_domain::<RelayDomain>().calls(calls).handler(relay::hosted).build());
//! mesh.serve(testing::for_domain::<LedgerDomain>().handler(ledger::hosted).build());
//! let mesh = mesh.granting(permissions).start();
//! let answer = mesh.call("relay", "relay_inspect", json!({})).await;
//! ```
//!
//! The mesh judges a Task subject live by asking the Task's server for it, where the
//! Gateway reads core's Task runtime.
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    sync::Arc,
};

use axum::{
    Form, Router,
    body::{Body, Bytes},
    extract::{Path, State},
    http::{HeaderMap, Request, StatusCode, header},
    response::{IntoResponse, Response},
    routing::post,
};
use chrono::{TimeDelta, Utc};
use serde::Deserialize;
use tower::ServiceExt;
use veoveo_types::{OAuthClientId, PermissionName, ResourceScheme, ServerSlug};

use super::{
    TEST_HOST, TEST_PROTOCOL_VERSION, access_token_trust, authority, principal, signing_key,
};
use crate::{
    ActingServer, DelegatedCaller, ExchangeGrant, ExchangedTokenVerifier, ForwardedRequest,
    GatewayInternalTokenIssuer, GatewayInternalTokenVerifier, GatewayProfileId,
    GatewayRequestContext, OAuthClientId as ClientId, OpaqueTaskId, TokenIssuer,
    calls::{
        ACCESS_TOKEN_TOKEN_TYPE, TASK_TOKEN_TYPE, TOKEN_EXCHANGE_GRANT_TYPE, TokenErrorCode,
        TokenErrorResponse, TokenExchangeResponse,
    },
    gateway::GatewayToolName,
    hosting::{HostedServer, ServerCalls},
    identity::AccessTokenSubject,
    registration::{
        AuthorizationDeclaration, ClientMetadata, PermissionsDetail, RegistrationDeclaration,
        ResourceIndicator, ServerEndpoint, ServerPublicKey, ServerVersion,
        assertion::{CLIENT_ASSERTION_TYPE, verify_client_assertion},
        statement::ServerKeyPair,
    },
};

/// The profile every mesh call goes through.
pub const MESH_PROFILE: &str = "operations";
/// The longest an exchanged token lives for a Task subject.
const TASK_DELEGATION: TimeDelta = TimeDelta::minutes(5);

struct Registered {
    declaration: AuthorizationDeclaration,
    client: OAuthClientId,
    key: ServerPublicKey,
    router: Option<(Router, String)>,
}

/// A Task a server created for a caller through the mesh.
#[derive(Clone)]
struct Route {
    server: ServerSlug,
    caller: DelegatedCaller,
}

struct Shared {
    base: url::Url,
    issuer: GatewayInternalTokenIssuer,
    exchanged: ExchangedTokenVerifier,
    servers: BTreeMap<ServerSlug, Registered>,
    grants: BTreeSet<PermissionName>,
    routes: parking_lot::Mutex<HashMap<String, Route>>,
    assertions: parking_lot::Mutex<HashSet<String>>,
}

/// An in-process Gateway that routes calls between registered servers.
pub struct TestMesh {
    listener: tokio::net::TcpListener,
    base: url::Url,
    servers: BTreeMap<ServerSlug, Registered>,
    grants: BTreeSet<PermissionName>,
}

impl TestMesh {
    /// Binds a loopback listener for the mesh's token and MCP endpoints.
    pub async fn bind() -> Self {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("the mesh binds a loopback port");
        let base = url::Url::parse(&format!(
            "http://{}",
            listener.local_addr().expect("a bound address")
        ))
        .expect("a loopback URL");
        Self {
            listener,
            base,
            servers: BTreeMap::new(),
            grants: BTreeSet::new(),
        }
    }

    /// The mesh's origin, where its token endpoint and profile live.
    pub fn base(&self) -> &url::Url {
        &self.base
    }

    /// Registers a server with `declaration` under the slug its permissions name, and
    /// returns the calls its handlers and Tasks make through the mesh.
    pub fn register(&mut self, declaration: AuthorizationDeclaration) -> ServerCalls {
        let slug = declaration
            .permissions
            .first()
            .map(|permission| permission.permission.namespace().to_owned())
            .and_then(|namespace| ServerSlug::parse(namespace).ok())
            .expect("a registered server declares at least one permission");
        let key = ServerKeyPair::generate().expect("a server key");
        let client = OAuthClientId::parse(format!("{slug}-mesh")).expect("a client identifier");
        let metadata = ClientMetadata::new(
            *key.public_key(),
            RegistrationDeclaration {
                slug: slug.clone(),
                uri_scheme: ResourceScheme::parse(slug.as_str()).expect("a scheme"),
                version: ServerVersion::new("0.1.0").expect("a version"),
                endpoint: ServerEndpoint::new(
                    url::Url::parse(super::TEST_RESOURCE).expect("the test resource"),
                )
                .expect("an endpoint"),
            },
            declaration.clone(),
        )
        .expect("the declaration admits");
        let mut token_endpoint = self.base.clone();
        token_endpoint.set_path("/oauth/token");
        let calls = ServerCalls::with_identity(
            client.clone(),
            &metadata,
            &key,
            self.base.clone(),
            token_endpoint,
            reqwest::Client::new(),
        )
        .expect("the mesh's calls");
        self.servers.insert(
            slug,
            Registered {
                declaration,
                client,
                key: *key.public_key(),
                router: None,
            },
        );
        calls
    }

    /// Routes a registered server's slug to its built router.
    pub fn serve(&mut self, server: HostedServer) {
        let registered = self
            .servers
            .get_mut(&server.slug)
            .expect("the server was registered first");
        registered.router = Some((server.router, server.mount.prefix().to_owned()));
    }

    /// The permissions the test user holds, in every namespace, as role evaluation
    /// would grant them.
    pub fn granting(mut self, permissions: impl IntoIterator<Item = PermissionName>) -> Self {
        self.grants = permissions.into_iter().collect();
        self
    }

    /// Serves the token and MCP endpoints until the returned mesh is dropped.
    pub fn start(self) -> RunningMesh {
        let shared = Arc::new(Shared {
            base: self.base,
            issuer: GatewayInternalTokenIssuer::new(
                TokenIssuer::parse(super::TEST_ISSUER).expect("the test issuer"),
                signing_key(),
            )
            .expect("the test issuer signs"),
            exchanged: ExchangedTokenVerifier::new(
                TokenIssuer::parse(super::TEST_ISSUER).expect("the test issuer"),
                super::trust_bundle(),
            ),
            servers: self.servers,
            grants: self.grants,
            routes: parking_lot::Mutex::default(),
            assertions: parking_lot::Mutex::default(),
        });
        let router = Router::new()
            .route("/oauth/token", post(token))
            .route("/mcp/{profile}", post(mcp))
            .fallback(ingress)
            .with_state(shared.clone());
        let listener = self.listener;
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });
        RunningMesh { shared, task }
    }
}

/// A started mesh.
pub struct RunningMesh {
    shared: Arc<Shared>,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for RunningMesh {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl RunningMesh {
    /// The mesh's origin. A request under a served server's mount reaches that server's
    /// routes there, as an ingress would deliver it, so a provider calls back a server's
    /// public routes at this origin.
    pub fn base(&self) -> url::Url {
        self.shared.base.clone()
    }

    /// The MCP endpoint of the mesh's profile.
    pub fn endpoint(&self) -> url::Url {
        let mut endpoint = self.shared.base.clone();
        endpoint.set_path(&format!("/mcp/{MESH_PROFILE}"));
        endpoint
    }

    /// Calls `tool` on `server` as the test user and returns the JSON-RPC response.
    pub async fn call(
        &self,
        server: &str,
        tool: &str,
        arguments: serde_json::Value,
    ) -> serde_json::Value {
        self.call_with(server, tool, arguments, false).await
    }

    /// Calls `tool` on `server` as the test user with Task support, so a tool that runs
    /// as a Task starts one. The mesh records the Task for its server's exchanges.
    pub async fn start_task(
        &self,
        server: &str,
        tool: &str,
        arguments: serde_json::Value,
    ) -> serde_json::Value {
        self.call_with(server, tool, arguments, true).await
    }

    /// Reads a Task of `server` as the test user.
    pub async fn task(&self, server: &str, task: &str) -> serde_json::Value {
        let server = ServerSlug::parse(server).expect("a server slug");
        let caller = user_caller();
        let body = serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "tasks/get",
            "params": {"taskId": task, "_meta": meta(true)}
        });
        let response = forward(
            &self.shared,
            &server,
            &caller,
            None,
            "tasks/get",
            None,
            body,
        )
        .await;
        json_body(response).await
    }

    async fn call_with(
        &self,
        server: &str,
        tool: &str,
        arguments: serde_json::Value,
        tasks: bool,
    ) -> serde_json::Value {
        let server = ServerSlug::parse(server).expect("a server slug");
        let caller = user_caller();
        let body = serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/call",
            "params": {"name": tool, "arguments": arguments, "_meta": meta(tasks)}
        });
        let response = forward(
            &self.shared,
            &server,
            &caller,
            None,
            "tools/call",
            Some(tool),
            body,
        )
        .await;
        let answer = json_body(response).await;
        if let Some(task) = answer["result"]["taskId"].as_str() {
            self.shared
                .routes
                .lock()
                .insert(task.to_owned(), Route { server, caller });
        }
        answer
    }
}

fn meta(tasks: bool) -> serde_json::Value {
    serde_json::json!({
        "io.modelcontextprotocol/protocolVersion": TEST_PROTOCOL_VERSION,
        "io.modelcontextprotocol/clientInfo": {"name": "test-mesh", "version": "1"},
        "io.modelcontextprotocol/clientCapabilities": if tasks {
            serde_json::json!({"extensions": {rmcp::model::TASKS_EXTENSION_ID: {}}})
        } else {
            serde_json::json!({})
        }
    })
}

/// The test user calling directly, with the request context the Gateway signs.
fn user_caller() -> DelegatedCaller {
    let actor = principal();
    let authority = authority();
    let now = Utc::now();
    DelegatedCaller {
        client: ClientId::parse(super::TEST_CLIENT).expect("the test client"),
        request_context: GatewayRequestContext {
            audit: veoveo_audit_contract::AuditRequest::background(),
            access_token: AccessTokenSubject {
                issuer: actor.issuer.clone(),
                subject: actor.subject.clone(),
                oauth_client_id: ClientId::parse(super::TEST_CLIENT).expect("the test client"),
                session_family: None,
                audience: crate::ProtectedResourceId::parse(super::TEST_RESOURCE)
                    .expect("the test resource"),
                work_context: authority.work_context.clone(),
                invocation_mode: authority.provenance.mode(),
                initiator: Some(actor.id.clone()),
                delegation_id: None,
                scopes: actor.scopes.clone(),
                jwt_id: None,
                issued_at: now,
                not_before: None,
                expires_at: now + TimeDelta::hours(1),
            },
            principal: actor.clone(),
        },
        actor,
        authority,
    }
}

/// A call another server made: the server, the permissions its exchanged token carries
/// and the Task it called from.
struct Delegation<'a> {
    acting: &'a ActingServer,
    permissions: &'a BTreeSet<PermissionName>,
    task: Option<&'a OpaqueTaskId>,
}

/// Sends one JSON-RPC request to `server` for `caller`, with the permissions the user
/// holds in its namespace, narrowed to the delegation's for a call another server made.
async fn forward(
    shared: &Shared,
    server: &ServerSlug,
    caller: &DelegatedCaller,
    delegation: Option<Delegation<'_>>,
    method: &str,
    name: Option<&str>,
    body: serde_json::Value,
) -> Response {
    let Some(Registered {
        router: Some((router, mount)),
        ..
    }) = shared.servers.get(server)
    else {
        return (
            StatusCode::NOT_FOUND,
            "no registered server serves this call",
        )
            .into_response();
    };
    let permissions = shared
        .grants
        .iter()
        .filter(|permission| permission.is_owned_by(server))
        .filter(|permission| {
            delegation
                .as_ref()
                .is_none_or(|delegation| delegation.permissions.contains(*permission))
        })
        .cloned()
        .collect();
    let token = shared
        .issuer
        .issue(ForwardedRequest {
            profile: GatewayProfileId::parse(MESH_PROFILE).expect("the mesh profile"),
            server: server.clone(),
            audience: ResourceIndicator::parse(super::TEST_RESOURCE).expect("the test resource"),
            client: caller.client.clone(),
            actor: caller.actor.clone(),
            authority: caller.authority.clone(),
            permissions,
            request_context: Some(caller.request_context.clone()),
            acting_server: delegation
                .as_ref()
                .map(|delegation| delegation.acting.clone()),
            acting_task: delegation
                .as_ref()
                .and_then(|delegation| delegation.task.cloned()),
            expires_at: Utc::now() + TimeDelta::seconds(60),
        })
        .expect("the mesh signs forwarded tokens")
        .bearer_token;
    let mut request = Request::builder()
        .method("POST")
        .uri(format!("{mount}/mcp"))
        .header(header::HOST, TEST_HOST)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, "application/json, text/event-stream")
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .header("mcp-protocol-version", TEST_PROTOCOL_VERSION)
        .header("mcp-method", method);
    // MCP 2026-07-28 names the request's target in `Mcp-Name`.
    let name = name.map(str::to_owned).or_else(|| {
        let params = &body["params"];
        params["taskId"]
            .as_str()
            .or(params["name"].as_str())
            .or(params["uri"].as_str())
            .map(str::to_owned)
    });
    if let Some(name) = name {
        request = request.header("mcp-name", name);
    }
    let request = request
        .body(Body::from(body.to_string()))
        .expect("a forwarded request");
    match router.clone().oneshot(request).await {
        Ok(response) => response.into_response(),
        Err(never) => match never {},
    }
}

/// The registered server whose URI scheme a resource URI names. A mesh server's scheme is
/// its slug.
fn resource_owner(shared: &Shared, uri: &str) -> Option<ServerSlug> {
    let scheme = url::Url::parse(uri).ok()?.scheme().to_owned();
    let server = ServerSlug::parse(scheme).ok()?;
    shared.servers.contains_key(&server).then_some(server)
}

/// The route of `server`'s Task `task` while the Task runs, as its server reports it.
async fn live_route(shared: &Shared, server: &ServerSlug, task: &str) -> Option<Route> {
    let route = shared.routes.lock().get(task).cloned()?;
    if &route.server != server {
        return None;
    }
    let current = forward(
        shared,
        server,
        &route.caller,
        None,
        "tasks/get",
        None,
        serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "tasks/get",
            "params": {"taskId": task, "_meta": meta(true)}
        }),
    )
    .await;
    let status = json_body(current).await["result"]["status"]
        .as_str()
        .map(str::to_owned);
    matches!(status.as_deref(), Some("working" | "input_required")).then_some(route)
}

async fn json_body(response: Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap_or_default();
    let text = String::from_utf8_lossy(&bytes);
    // A Streamable HTTP answer may arrive as one SSE event.
    let json = text
        .lines()
        .find_map(|line| line.strip_prefix("data:"))
        .unwrap_or(&text);
    serde_json::from_str(json.trim()).unwrap_or(serde_json::Value::Null)
}

#[derive(Deserialize)]
struct ExchangeForm {
    grant_type: String,
    #[serde(default)]
    client_id: Option<String>,
    client_assertion_type: Option<String>,
    client_assertion: Option<String>,
    subject_token: Option<String>,
    subject_token_type: Option<String>,
    resource: Option<String>,
    authorization_details: Option<String>,
}

fn refused(error: TokenErrorCode, description: impl Into<String>) -> Response {
    let status = if error == TokenErrorCode::InvalidClient {
        StatusCode::UNAUTHORIZED
    } else {
        StatusCode::BAD_REQUEST
    };
    (
        status,
        [(header::CACHE_CONTROL, "no-store")],
        axum::Json(TokenErrorResponse {
            error,
            error_description: Some(description.into()),
        }),
    )
        .into_response()
}

/// The mesh's RFC 8693 token exchange, narrowed as the Gateway narrows it.
async fn token(State(shared): State<Arc<Shared>>, Form(form): Form<ExchangeForm>) -> Response {
    if form.grant_type != TOKEN_EXCHANGE_GRANT_TYPE {
        return refused(
            TokenErrorCode::UnsupportedGrantType,
            "the mesh exchanges tokens only",
        );
    }
    // The registered server authenticates with its pinned key.
    let (Some(CLIENT_ASSERTION_TYPE), Some(assertion)) = (
        form.client_assertion_type.as_deref(),
        form.client_assertion.as_deref(),
    ) else {
        return refused(
            TokenErrorCode::InvalidClient,
            "a client assertion is required",
        );
    };
    let Some((server, registered)) = shared
        .servers
        .iter()
        .find(|(_, registered)| form.client_id.as_deref() == Some(registered.client.as_str()))
    else {
        return refused(TokenErrorCode::InvalidClient, "unknown client");
    };
    let mut endpoint = shared.base.clone();
    endpoint.set_path("/oauth/token");
    let Ok(verified) = verify_client_assertion(
        assertion,
        &registered.client,
        &registered.key,
        &endpoint,
        Utc::now(),
    ) else {
        return refused(
            TokenErrorCode::InvalidClient,
            "the client assertion did not verify",
        );
    };
    if !shared.assertions.lock().insert(verified.jwt_id.to_string()) {
        return refused(
            TokenErrorCode::InvalidClient,
            "the client assertion was replayed",
        );
    }
    let acting = ActingServer {
        server: server.clone(),
        client: registered.client.clone(),
    };
    // The subject: this server's forwarded token, or one of its live Tasks.
    let (caller, task, expires_at) = match (
        form.subject_token.as_deref(),
        form.subject_token_type.as_deref(),
    ) {
        (Some(token), Some(ACCESS_TOKEN_TOKEN_TYPE)) => {
            let verifier = GatewayInternalTokenVerifier::from_access_tokens(
                &access_token_trust(),
                server.clone(),
            )
            .expect("the mesh's trust");
            let Ok(identity) = verifier.verify(token) else {
                return refused(
                    TokenErrorCode::InvalidGrant,
                    "the subject token is not this server's",
                );
            };
            let Ok(caller) = DelegatedCaller::of(&identity) else {
                return refused(
                    TokenErrorCode::InvalidGrant,
                    "a delegated identity cannot be delegated again",
                );
            };
            (caller, None, identity.expires_at)
        }
        (Some(task), Some(TASK_TOKEN_TYPE)) => {
            let Ok(task) = OpaqueTaskId::parse(task) else {
                return refused(
                    TokenErrorCode::InvalidGrant,
                    "the Task identifier is invalid",
                );
            };
            let Some(route) = live_route(&shared, server, task.as_str()).await else {
                return refused(
                    TokenErrorCode::InvalidGrant,
                    "the Task has ended or is not this server's",
                );
            };
            (route.caller, Some(task), Utc::now() + TASK_DELEGATION)
        }
        _ => {
            return refused(
                TokenErrorCode::InvalidRequest,
                "a subject token is required",
            );
        }
    };
    let profile_resource = {
        let mut resource = shared.base.clone();
        resource.set_path(&format!("/mcp/{MESH_PROFILE}"));
        resource
    };
    if form.resource.as_deref() != Some(profile_resource.as_str()) {
        return refused(
            TokenErrorCode::InvalidTarget,
            "the resource is not the mesh's profile",
        );
    }
    let uses: BTreeSet<&PermissionName> = registered.declaration.uses.iter().collect();
    let requested: BTreeSet<PermissionName> = match form.authorization_details.as_deref() {
        None => uses
            .iter()
            .map(|permission| (*permission).clone())
            .collect(),
        Some(details) => match serde_json::from_str::<Vec<PermissionsDetail>>(details) {
            Ok(details) => details
                .into_iter()
                .flat_map(|detail| detail.actions)
                .collect(),
            Err(_) => {
                return refused(
                    TokenErrorCode::InvalidAuthorizationDetails,
                    "authorization_details is not a permissions detail",
                );
            }
        },
    };
    if let Some(undeclared) = requested
        .iter()
        .find(|permission| !uses.contains(permission))
    {
        return refused(
            TokenErrorCode::InvalidAuthorizationDetails,
            format!("this server declares no use of {undeclared}"),
        );
    }
    let missing: Vec<String> = requested
        .iter()
        .filter(|permission| {
            !shared.servers.values().any(|server| {
                server
                    .declaration
                    .permissions
                    .iter()
                    .any(|declared| &declared.permission == *permission)
            })
        })
        .map(ToString::to_string)
        .collect();
    if !missing.is_empty() {
        return refused(
            TokenErrorCode::InvalidTarget,
            format!("no registered server provides {}", missing.join(", ")),
        );
    }
    let permissions: BTreeSet<PermissionName> = requested
        .into_iter()
        .filter(|permission| shared.grants.contains(permission))
        .collect();
    let expires_at = expires_at
        .min(Utc::now() + TimeDelta::seconds(60))
        .max(Utc::now() + TimeDelta::seconds(1));
    let issued = match shared.issuer.issue_exchanged(ExchangeGrant {
        profile: GatewayProfileId::parse(MESH_PROFILE).expect("the mesh profile"),
        resource: ResourceIndicator::parse(profile_resource.as_str())
            .expect("the profile resource"),
        caller,
        acting_server: acting,
        permissions,
        task,
        expires_at,
    }) {
        Ok(issued) => issued,
        Err(error) => return refused(TokenErrorCode::InvalidGrant, error.to_string()),
    };
    let expires_in = (issued.identity.expires_at - Utc::now())
        .num_seconds()
        .max(1) as u64;
    axum::Json(TokenExchangeResponse {
        access_token: issued.bearer_token,
        issued_token_type: ACCESS_TOKEN_TOKEN_TYPE.into(),
        token_type: "Bearer".into(),
        expires_in,
        authorization_details: (!issued.identity.permissions.is_empty())
            .then(|| PermissionsDetail::new(issued.identity.permissions.clone()))
            .into_iter()
            .collect(),
    })
    .into_response()
}

/// The mesh's profile endpoint: each call reaches its provider with a delegated
/// forwarded token.
async fn mcp(
    State(shared): State<Arc<Shared>>,
    Path(profile): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let mut resource = shared.base.clone();
    resource.set_path(&format!("/mcp/{profile}"));
    let bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "));
    let identity = bearer.and_then(|bearer| {
        shared
            .exchanged
            .verify(
                bearer,
                &ResourceIndicator::parse(resource.as_str()).expect("the profile resource"),
            )
            .ok()
    });
    // A Task's delegation lasts while the Task runs.
    let identity = match identity {
        Some(identity) => match &identity.task {
            Some(task)
                if live_route(&shared, &identity.acting_server.server, task.as_str())
                    .await
                    .is_none() =>
            {
                None
            }
            _ => Some(identity),
        },
        None => None,
    };
    let Some(identity) = identity else {
        return (
            StatusCode::UNAUTHORIZED,
            [(header::WWW_AUTHENTICATE, r#"Bearer error="invalid_token""#)],
        )
            .into_response();
    };
    let Ok(mut message) = serde_json::from_slice::<serde_json::Value>(&body) else {
        return (StatusCode::BAD_REQUEST, "not one JSON-RPC message").into_response();
    };
    let method = message["method"].as_str().unwrap_or_default().to_owned();
    let provider_of_permissions = identity
        .permissions
        .iter()
        .next()
        .and_then(|permission| ServerSlug::parse(permission.namespace()).ok());
    let (provider, name) = match method.as_str() {
        "tools/call" => {
            let Some((server, tool)) = message["params"]["name"]
                .as_str()
                .and_then(|name| GatewayToolName::parse(name).ok())
                .and_then(|name| name.parts().ok())
            else {
                return (StatusCode::BAD_REQUEST, "an unknown tool").into_response();
            };
            message["params"]["name"] = serde_json::json!(tool.as_str());
            (Some(server), Some(tool.to_string()))
        }
        "tasks/get" | "tasks/cancel" | "tasks/update" => {
            let task = message["params"]["taskId"].as_str().unwrap_or_default();
            (
                shared
                    .routes
                    .lock()
                    .get(task)
                    .map(|route| route.server.clone()),
                None,
            )
        }
        "resources/read" => (
            message["params"]["uri"]
                .as_str()
                .and_then(|uri| resource_owner(&shared, uri))
                .or(provider_of_permissions),
            None,
        ),
        "subscriptions/listen" => {
            let notifications = &message["params"]["notifications"];
            let task = notifications["taskIds"][0].as_str().unwrap_or_default();
            let task_server = shared
                .routes
                .lock()
                .get(task)
                .map(|route| route.server.clone());
            (
                task_server
                    .or_else(|| {
                        notifications["resourceSubscriptions"][0]
                            .as_str()
                            .and_then(|uri| resource_owner(&shared, uri))
                    })
                    .or(provider_of_permissions.clone()),
                None,
            )
        }
        _ => (provider_of_permissions, None),
    };
    let Some(provider) = provider else {
        return (
            StatusCode::NOT_FOUND,
            "no registered server serves this call",
        )
            .into_response();
    };
    let response = forward(
        &shared,
        &provider,
        &identity.caller,
        Some(Delegation {
            acting: &identity.acting_server,
            permissions: &identity.permissions,
            task: identity.task.as_ref(),
        }),
        &method,
        name.as_deref(),
        message,
    )
    .await;
    if method != "tools/call" {
        return response;
    }
    // Record a Task the call started, so later requests about it reach its server.
    let (parts, body) = response.into_parts();
    let bytes = axum::body::to_bytes(body, usize::MAX)
        .await
        .unwrap_or_default();
    let text = String::from_utf8_lossy(&bytes);
    let json = text
        .lines()
        .find_map(|line| line.strip_prefix("data:"))
        .unwrap_or(&text);
    if let Ok(answer) = serde_json::from_str::<serde_json::Value>(json.trim())
        && let Some(task) = answer["result"]["taskId"].as_str()
    {
        shared.routes.lock().insert(
            task.to_owned(),
            Route {
                server: provider,
                caller: identity.caller.clone(),
            },
        );
    }
    Response::from_parts(parts, Body::from(bytes))
}

/// A request outside the token and MCP endpoints, delivered as an ingress would: under a
/// served server's mount it reaches that server's router with the test host, without
/// Gateway authentication, so only a server's public routes answer it.
async fn ingress(State(shared): State<Arc<Shared>>, request: Request<Body>) -> Response {
    let path = request.uri().path().to_owned();
    let served = shared
        .servers
        .values()
        .filter_map(|server| server.router.as_ref());
    let Some((router, _)) = served.into_iter().find(|(_, mount)| {
        !mount.is_empty()
            && path
                .strip_prefix(mount.as_str())
                .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
    }) else {
        return (StatusCode::NOT_FOUND, "no served server owns this path").into_response();
    };
    let (mut parts, body) = request.into_parts();
    parts
        .headers
        .insert(header::HOST, header::HeaderValue::from_static(TEST_HOST));
    match router
        .clone()
        .oneshot(Request::from_parts(parts, body))
        .await
    {
        Ok(response) => response.into_response(),
        Err(never) => match never {},
    }
}
