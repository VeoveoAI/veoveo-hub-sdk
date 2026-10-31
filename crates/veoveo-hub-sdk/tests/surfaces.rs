//! The HTTP a hosted server serves besides MCP, declared in its typed routes: public
//! route prefixes and the app origin, their registration declaration, and how the
//! hosted server serves them through the in-process Hub.
use std::sync::LazyLock;

use axum::{
    body::Body,
    http::StatusCode,
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
        Access, AuthorizationDeclaration, ClientMetadata, PermissionDefinition,
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
