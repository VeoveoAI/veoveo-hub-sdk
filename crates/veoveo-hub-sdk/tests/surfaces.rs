//! The HTTP a hosted server serves besides MCP, declared in its typed routes: public
//! route prefixes and the app origin, their registration declaration, and how the
//! hosted server serves them through the in-process Hub.
use std::sync::LazyLock;

use axum::{
    body::Body,
    http::{Request, StatusCode},
    routing::{get, post},
};
use rmcp::model::{Implementation, ServerCapabilities, ServerConfig};
use serde_json::json;
use veoveo_hub_sdk::{
    calls::ProviderContract,
    docs::ServerDocs,
    hosting::{
        DomainServer, Hosted, RoutePolicy, Routes, RoutesBuilder,
        testing::{self, TestHub},
    },
    registration::{
        Access, AppOrigin, AuthorizationDeclaration, ClientMetadata, PermissionDefinition,
        RegistrationDeclaration, RouteAccess, ServerEndpoint, ServerPublicKey, ServerVersion,
    },
    server_contract::{McpServerContract, McpServerSetup},
};
use veoveo_types::{PermissionName, ResourceScheme, ServerSlug};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, veoveo_types::Vocabulary)]
#[vocabulary(scope)]
enum Scope {}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, veoveo_types::Vocabulary)]
enum Permission {
    #[vocabulary(rename = "studio.editor")]
    Editor,
}
impl PermissionDefinition for Permission {
    fn resource(self) -> &'static str {
        "studio.workflow"
    }
    fn access(self) -> Access {
        Access::Write
    }
}
fn editor() -> RoutePolicy {
    RoutePolicy::permission(Permission::Editor).unwrap()
}
fn editor_permission() -> PermissionName {
    PermissionName::parse("studio.editor").unwrap()
}

struct Studio;
impl ProviderContract for Studio {
    fn slug() -> ServerSlug {
        ServerSlug::parse("studio").unwrap()
    }
}
fn scheme() -> ResourceScheme {
    ResourceScheme::parse("studio").unwrap()
}

static DOCS: LazyLock<ServerDocs> = LazyLock::new(|| {
    ServerDocs::new(Studio::slug())
        .with_doc("agents", "Agents", "# Agents")
        .with_doc("design", "Design", "# Design")
});

struct Contract;
#[derive(Clone)]
struct Domain;
static SETUP: LazyLock<McpServerSetup<Contract>> = LazyLock::new(|| McpServerSetup::new().unwrap());
/// A webhook route, the SDK's operation callbacks and an app origin for editors.
static ROUTES: LazyLock<Routes<Domain>> = LazyLock::new(|| {
    builder()
        .public_route("hooks/", 1_048_576, 600)
        .operation_callbacks()
        .app_origin(editor())
        .build()
});
fn builder() -> RoutesBuilder<Domain> {
    Routes::builder(RoutePolicy::Authenticated)
}
impl McpServerContract for Contract {
    type Server = Domain;
    type Provider = Studio;
    type Scope = Scope;
    fn scheme() -> ResourceScheme {
        scheme()
    }
    fn scopes() -> &'static [Scope] {
        Scope::ALL
    }
    fn documents() -> &'static ServerDocs {
        &DOCS
    }
    fn server_config() -> ServerConfig {
        let mut config = ServerConfig::new(ServerCapabilities::builder().build());
        config.server_info = Implementation::new("studio", "1");
        config
    }
}
impl DomainServer for Domain {
    type Contract = Contract;
    fn setup() -> &'static McpServerSetup<Contract> {
        &SETUP
    }
    fn routes() -> &'static Routes<Self> {
        &ROUTES
    }
}

fn registration() -> RegistrationDeclaration {
    RegistrationDeclaration {
        deployment: testing::deployment_id(),
        slug: Studio::slug(),
        uri_scheme: scheme(),
        version: ServerVersion::new("1").unwrap(),
        endpoint: ServerEndpoint::new("https://studio.example/studio/mcp".parse().unwrap())
            .unwrap(),
        surfaces: Domain::routes().surfaces(),
    }
}

#[test]
fn the_routes_declare_their_surfaces_and_the_app_origin_requirement() {
    let surfaces = Domain::routes().surfaces().unwrap();
    assert_eq!(
        serde_json::to_value(&surfaces).unwrap(),
        json!({
            "publicRoutes": [
                {"prefix": "/studio/hooks/", "maxBodyBytes": 1_048_576, "requestsPerMinute": 600},
                {"prefix": "/studio/operations/", "maxBodyBytes": 1_048_576, "requestsPerMinute": 600}
            ],
            "appOrigin": true
        })
    );
    let authorization: AuthorizationDeclaration = Domain::routes().authorization();
    assert_eq!(
        authorization.requirements.app_origin,
        Some(RouteAccess::Permission(editor_permission()))
    );
    assert!(authorization.permission(&editor_permission()).is_some());
    // The registration carries both, and the Hub's checks admit them.
    let metadata = ClientMetadata::new(
        ServerPublicKey::from_bytes([3; 32]),
        registration(),
        authorization,
    )
    .unwrap();
    let wire = serde_json::to_value(&metadata).unwrap();
    assert_eq!(wire["veoveo_surfaces"]["app_origin"], true);
    assert_eq!(
        wire["veoveo_surfaces"]["public_routes"][0]["prefix"],
        "/studio/hooks/"
    );
    // Routes that serve neither declare no surfaces.
    assert!(builder().build().surfaces().is_none());
    assert!(
        builder()
            .build()
            .authorization()
            .requirements
            .app_origin
            .is_none()
    );
}

fn hub(server: veoveo_hub_sdk::hosting::HostedServer) -> TestHub {
    TestHub::new(server).granting([editor_permission()])
}

/// The studio's server with a webhook route, an undeclared public route and an
/// authenticated route, before it is built.
macro_rules! served {
    () => {
        testing::for_domain::<Domain>()
            .handler(|| Hosted::new(Domain))
            .public_routes(
                axum::Router::new()
                    .route("/hooks/github", post(|| async { "hooked" }))
                    .route("/private/leak", get(|| async { "leaked" })),
            )
            .authenticated_routes(axum::Router::new().route("/editor", get(|| async { "editor" })))
    };
}

#[tokio::test]
async fn public_routes_answer_only_under_declared_prefixes() {
    let hub = hub(served!().build());
    let (status, body) = hub
        .send(
            hub.request("/hooks/github")
                .method("POST")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!((status, body.as_str()), (StatusCode::OK, "hooked"));
    let (status, body) = hub
        .send(hub.request("/private/leak").body(Body::empty()).unwrap())
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert!(!body.contains("leaked"));
    // Authenticated routes stay behind the Hub's token under the mount.
    let (status, _) = hub
        .send(hub.request("/editor").body(Body::empty()).unwrap())
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[test]
#[should_panic(expected = "covers the server's MCP or administrative routes")]
fn a_prefix_covering_mcp_is_refused() {
    let _ = builder().public_route("mcp/", 1024, 60).build();
}

#[test]
#[should_panic(expected = "covers the server's MCP or administrative routes")]
fn a_prefix_covering_admin_is_refused() {
    let _ = builder().public_route("admin/hooks/", 1024, 60).build();
}

#[test]
#[should_panic(expected = "overlaps public route")]
fn overlapping_prefixes_are_refused() {
    let _ = builder()
        .public_route("hooks/", 1024, 60)
        .public_route("hooks/github/", 1024, 60)
        .build();
}

#[test]
#[should_panic(expected = "overlaps public route")]
fn the_sdks_prefix_declared_twice_is_refused() {
    let _ = builder()
        .operation_callbacks()
        .public_route("operations/", 1024, 60)
        .build();
}

#[test]
#[should_panic(expected = "valid public route prefix")]
fn a_prefix_without_its_final_slash_is_refused() {
    let _ = builder().public_route("hooks", 1024, 60);
}

#[test]
#[should_panic(expected = "valid public route body limit")]
fn a_body_limit_outside_its_range_is_refused() {
    let _ = builder().public_route("hooks/", 16 * 1024 * 1024 + 1, 60);
}

#[test]
#[should_panic(expected = "valid public route rate")]
fn a_rate_outside_its_range_is_refused() {
    let _ = builder().public_route("hooks/", 1024, 0);
}

const APP_HOST: &str = "studio.apps.veoveo.test";

/// The status and body of a request to `path` on `host`, with `bearer` when given.
async fn send_on(
    hub: &TestHub,
    method: &str,
    host: &str,
    path: &str,
    bearer: Option<&str>,
) -> (StatusCode, String) {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("host", host);
    if let Some(bearer) = bearer {
        request = request.header("authorization", format!("Bearer {bearer}"));
    }
    hub.send(request.body(Body::empty()).unwrap()).await
}

fn app_hub() -> TestHub {
    hub(served!()
        .app_origin(Some(
            AppOrigin::parse(&format!("https://{APP_HOST}")).unwrap(),
        ))
        .build())
}

#[tokio::test]
async fn the_app_origin_serves_the_authenticated_routes_at_its_root() {
    let hub = app_hub();
    let token = hub.token();
    let (status, body) = send_on(&hub, "GET", APP_HOST, "/editor", Some(&token)).await;
    assert_eq!((status, body.as_str()), (StatusCode::OK, "editor"));
    // The same routes stay under the mount on the deployment's host.
    let (status, body) = send_on(
        &hub,
        "GET",
        testing::TEST_HOST,
        "/studio/editor",
        Some(&token),
    )
    .await;
    assert_eq!((status, body.as_str()), (StatusCode::OK, "editor"));
    // The Hub's token is required, and the app origin's permission on every request.
    let (status, _) = send_on(&hub, "GET", APP_HOST, "/editor", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let unpermitted = hub.token_granting([]);
    let (status, _) = send_on(&hub, "GET", APP_HOST, "/editor", Some(&unpermitted)).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    // Under the mount, an authenticated route needs no app origin permission.
    let (status, _) = send_on(
        &hub,
        "GET",
        testing::TEST_HOST,
        "/studio/editor",
        Some(&unpermitted),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn the_app_origin_reaches_no_other_route() {
    let hub = app_hub();
    let token = hub.token();
    for (method, path) in [
        ("POST", "/mcp"),
        ("POST", "/studio/mcp"),
        ("GET", "/admin/docs/llms.txt"),
        ("GET", "/studio/admin/docs/llms.txt"),
        ("GET", "/studio/editor"),
        ("POST", "/hooks/github"),
        ("POST", "/studio/hooks/github"),
        ("GET", "/livez"),
    ] {
        let (status, _) = send_on(&hub, method, APP_HOST, path, Some(&token)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{method} {path}");
    }
    // Without a token, nothing on the app origin answers but the challenge.
    let (status, _) = send_on(&hub, "POST", APP_HOST, "/hooks/github", None).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    // The server's own routes keep their hosts: another app origin is misdirected.
    let (status, _) = send_on(&hub, "GET", "other.apps.veoveo.test", "/editor", None).await;
    assert_eq!(status, StatusCode::MISDIRECTED_REQUEST);
    let (status, body) = send_on(
        &hub,
        "POST",
        testing::TEST_HOST,
        "/studio/hooks/github",
        None,
    )
    .await;
    assert_eq!((status, body.as_str()), (StatusCode::OK, "hooked"));
}

#[tokio::test]
async fn the_test_mesh_registers_a_server_with_an_app_origin() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let mut mesh = testing::TestMesh::bind().await;
    // The mesh registers the app origin its requirement implies, as the Hub's rules ask.
    let _calls = mesh.register(Domain::routes().authorization());
}
