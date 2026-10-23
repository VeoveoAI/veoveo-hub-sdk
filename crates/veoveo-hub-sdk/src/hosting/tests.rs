//! In-process tests of typed hosting with real synthetic Hub tokens.
use super::{
    DomainServer, Hosted, ReadCache, ResourceBuilder, RoutePolicy, Routes, hub_identity,
    testing::{self, TestHub},
};
use crate::{
    ServerSlug,
    docs::ServerDocs,
    server_contract::{McpServerContract, McpServerSetup},
};
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use rmcp::model::{ServerCapabilities, ServerConfig};
use std::sync::LazyLock;
use veoveo_types::{ResourceAddress, ResourceScheme, ResourceUri};
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, veoveo_types::Vocabulary)]
#[vocabulary(scope)]
enum FixtureScope {}
#[derive(Debug, Clone, PartialEq, Eq)]
enum FixtureResource {
    Docs,
    Items,
    Item,
    NestedItem,
    Volatile,
}
impl ResourceAddress for FixtureResource {
    type Error = veoveo_types::IdentifierError;
    fn parse(uri: &ResourceUri) -> Result<Self, Self::Error> {
        match uri.as_str() {
            "fixture://docs" => Ok(Self::Docs),
            "fixture://items" => Ok(Self::Items),
            "fixture://items/1" => Ok(Self::Item),
            "fixture://items/1/member" => Ok(Self::NestedItem),
            "fixture://volatile" => Ok(Self::Volatile),
            _ => Err(Self::Error::new(uri.as_str(), "unknown fixture address")),
        }
    }
    fn to_uri(&self) -> Result<ResourceUri, Self::Error> {
        ResourceUri::new(match self {
            Self::Docs => "fixture://docs",
            Self::Items => "fixture://items",
            Self::Item => "fixture://items/1",
            Self::NestedItem => "fixture://items/1/member",
            Self::Volatile => "fixture://volatile",
        })
        .map_err(|_| Self::Error::new("fixture", "invalid fixture address"))
    }
}
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, veoveo_types::Vocabulary)]
enum FixturePermission {
    #[vocabulary(rename = "fixture.docs.read")]
    Docs,
    #[vocabulary(rename = "fixture.admin.read")]
    Admin,
}
impl crate::registration::PermissionDefinition for FixturePermission {
    fn resource(self) -> &'static str {
        "fixture.item"
    }
    fn access(self) -> crate::registration::Access {
        crate::registration::Access::Read
    }
    fn risks(self) -> &'static [crate::registration::Risk] {
        &[]
    }
}
static DOCS: LazyLock<ServerDocs> = LazyLock::new(|| {
    ServerDocs::new(ServerSlug::parse("fixture").unwrap())
        .with_doc(
            "agents",
            "Agent manual",
            include_str!("../../testdata/hosting-agents.md"),
        )
        .with_doc("design", "Design", "# Design")
});
struct FixtureContract;
impl crate::calls::ProviderContract for FixtureContract {
    fn slug() -> ServerSlug {
        ServerSlug::parse("fixture").unwrap()
    }
}
impl McpServerContract for FixtureContract {
    type Server = FixtureDomain;
    type Provider = Self;
    type Scope = FixtureScope;
    fn scheme() -> ResourceScheme {
        ResourceScheme::parse("fixture").unwrap()
    }
    fn scopes() -> &'static [FixtureScope] {
        FixtureScope::ALL
    }
    fn documents() -> &'static ServerDocs {
        &DOCS
    }
    fn server_config() -> ServerConfig {
        let mut config = ServerConfig::new(ServerCapabilities::builder().enable_tools().build());
        config.server_info = rmcp::model::Implementation::new("fixture", "1");
        config
    }
}
static SETUP: LazyLock<McpServerSetup<FixtureContract>> =
    LazyLock::new(|| McpServerSetup::new().expect("fixture setup"));
#[derive(Clone)]
struct FixtureDomain;
impl FixtureDomain {
    fn new() -> Self {
        Self
    }
}
#[derive(Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[crate::tool(provider=FixtureContract,output=Echo,name="echo")]
struct EchoRequest {
    text: String,
}
#[derive(Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[crate::tool(provider=FixtureContract,output=Echo,name="admin_echo")]
struct AdminRequest {
    text: String,
}
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
struct Echo {
    text: String,
}
impl crate::definition::SingleOutput for Echo {}
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
struct State {
    count: usize,
}
impl crate::definition::SingleOutput for State {}
#[derive(Clone)]
struct ReadState(FixtureResource);
impl crate::calls::ResourceRead for ReadState {
    type Provider = FixtureContract;
    type Address = FixtureResource;
    type Content = State;
    fn address(&self) -> FixtureResource {
        self.0.clone()
    }
}
struct ReadItem;
impl crate::calls::ResourceRead for ReadItem {
    type Provider = FixtureContract;
    type Address = FixtureResource;
    type Content = State;
    fn address(&self) -> FixtureResource {
        FixtureResource::Item
    }
}
impl crate::definition::TemplateRead for ReadItem {
    const TEMPLATE: &'static str = "fixture://items/{item_id}";
    fn from_address(address: &FixtureResource) -> Option<Self> {
        (*address == FixtureResource::Item).then_some(Self)
    }
}
#[crate::resource_read(provider = FixtureContract, address_type = FixtureResource, content = State,
    template = "fixture://items/{item_id}/member", address = FixtureResource::NestedItem,
    parse = |address| matches!(address, FixtureResource::NestedItem).then_some(Self))]
struct ReadNestedItem;

/// The fixture's changing descriptions. Only
/// `a_changed_description_is_listed_and_told_to_listeners` changes one, of `admin_echo`.
static DESCRIPTIONS: LazyLock<super::ToolDescriptions> =
    LazyLock::new(super::ToolDescriptions::new);

static ROUTES: LazyLock<Routes<FixtureDomain>> = LazyLock::new(|| {
    Routes::builder(RoutePolicy::permission(FixturePermission::Docs).unwrap())
        .tool::<EchoRequest, _, _>(
            "Echo text back to the verified caller",
            RoutePolicy::Authenticated,
            |_, request, context| async move {
                Ok(Echo {
                    text: format!(
                        "{} from {}",
                        request.text,
                        hub_identity(&context)?.actor.subject
                    ),
                })
            },
        )
        .tool::<AdminRequest, _, _>(
            "Echo for administrators",
            RoutePolicy::permission(FixturePermission::Admin).unwrap(),
            |_, request, _| async move { Ok(Echo { text: request.text }) },
        )
        .singleton(
            ReadState(FixtureResource::Items),
            ResourceBuilder::new("items")
                .reader(|_, _, _| async { Ok(State { count: 0 }) })
                .policy(RoutePolicy::Authenticated)
                .cache(ReadCache::Private),
        )
        .singleton(
            ReadState(FixtureResource::Item),
            ResourceBuilder::new("item-1")
                .reader(|_, _, _| async { Ok(State { count: 1 }) })
                .policy(RoutePolicy::Authenticated)
                .cache(ReadCache::Private),
        )
        .singleton(
            ReadState(FixtureResource::Volatile),
            ResourceBuilder::new("volatile")
                .reader(|_, _, _| async { Ok(State { count: 0 }) })
                .policy(RoutePolicy::Authenticated)
                .cache(ReadCache::NoStore),
        )
        .template::<ReadItem, _, _>(
            ResourceBuilder::new("item")
                .reader(|_, _, _| async { Ok(State { count: 1 }) })
                .policy(RoutePolicy::Authenticated)
                .cache(ReadCache::Private),
        )
        .prompt(
            rmcp::model::Prompt::new("summarize", Some("Summarize items"), None),
            RoutePolicy::Authenticated,
            |_, _, _| async {
                Ok(rmcp::model::GetPromptResult::new(vec![
                    rmcp::model::PromptMessage::new_text(
                        rmcp::model::Role::User,
                        "Summarize fixture://items.",
                    ),
                ]))
            },
        )
        .prompt(
            rmcp::model::Prompt::new("admin-summary", Some("Administrator summary"), None),
            RoutePolicy::permission(FixturePermission::Admin).unwrap(),
            |_, _, _| async { Ok(rmcp::model::GetPromptResult::new(vec![])) },
        )
        .descriptions(DESCRIPTIONS.clone())
        .build()
});
impl DomainServer for FixtureDomain {
    type Contract = FixtureContract;
    fn setup() -> &'static McpServerSetup<FixtureContract> {
        &SETUP
    }
    fn routes() -> &'static Routes<Self> {
        &ROUTES
    }
}

fn hub() -> TestHub {
    TestHub::new(
        testing::for_domain::<FixtureDomain>()
            .handler(|| Hosted::new(FixtureDomain::new()))
            .admin_routes(
                axum::Router::new().route("/status", axum::routing::get(|| async { "admin ok" })),
            )
            .readiness(|| async { true })
            .build(),
    )
    .granting([veoveo_types::PermissionName::parse("fixture.docs.read").unwrap()])
}

#[test]
fn test_hub_tokens_carry_the_permissions_it_grants() {
    let read = veoveo_types::PermissionName::parse("fixture.read").unwrap();
    let hub = hub().granting([read.clone()]);
    let verifier = crate::HubInternalTokenVerifier::from_access_tokens(
        &testing::access_token_trust(),
        ServerSlug::parse("fixture").unwrap(),
    )
    .unwrap();
    let identity = verifier.verify(&hub.token()).unwrap();
    assert!(identity.grants(&read));
    assert_eq!(identity.client.as_str(), testing::TEST_CLIENT);
    assert_eq!(identity.issuer.as_str(), testing::TEST_ISSUER);
    assert_eq!(identity.audience.as_str(), testing::TEST_RESOURCE);
    let narrowed = verifier.verify(&hub.token_granting([])).unwrap();
    assert!(narrowed.permissions.is_empty());
}

#[tokio::test]
async fn tokens_signed_by_a_rotated_hub_key_pass_after_a_key_refresh() {
    use crate::internal_auth::forwarded_tests::{KeyServer, generated_key, http};
    let keys = KeyServer::start().await;
    let (registered, rotated) = (generated_key(), generated_key());
    keys.publish(&[&rotated]);
    let deployment =
        crate::PublicDeployment::new(format!("https://{}", testing::TEST_HOST)).unwrap();
    let hub = TestHub::new(
        super::HostedServer::for_domain::<FixtureDomain>()
            .deployment(&deployment, false)
            .unwrap()
            .internal_trust(&keys.trust(&[&registered], "fixture"), http())
            .unwrap()
            .handler(|| Hosted::new(FixtureDomain::new()))
            .build(),
    );
    for key in [&registered, &rotated] {
        let (status, body) = hub
            .rpc_with(
                "tools/list",
                serde_json::json!({}),
                Some(&keys.token(key, "fixture")),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["result"]["tools"][0]["name"], "echo");
    }
    assert_eq!(keys.jwks_requests(), 1);
    // The refreshed set retired the registration's key.
    let (status, _) = hub
        .rpc_with(
            "tools/list",
            serde_json::json!({}),
            Some(&keys.token(&registered, "fixture")),
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

/// The status and body of a GET to `path` at the root of `hub`'s listener.
async fn get_root(hub: &TestHub, path: &str) -> (StatusCode, String) {
    hub.send(TestHub::root_request(path).body(Body::empty()).unwrap())
        .await
}

#[tokio::test]
async fn health_and_readiness_need_no_authentication() {
    let hub = hub();
    let (status, body) = get_root(&hub, "/livez").await;
    assert_eq!((status, body.as_str()), (StatusCode::OK, "ok"));
    let (status, body) = get_root(&hub, "/readyz").await;
    assert_eq!((status, body.as_str()), (StatusCode::OK, "ready"));
}

#[tokio::test]
async fn health_routes_sit_only_at_the_listener_root() {
    for server in [
        testing::for_domain::<FixtureDomain>(),
        testing::internal_for_domain::<FixtureDomain>(),
    ] {
        let hub = TestHub::new(
            server
                .handler(|| Hosted::new(FixtureDomain::new()))
                .readiness(|| async { true })
                .build(),
        );
        for path in ["/livez", "/readyz"] {
            assert_eq!(get_root(&hub, path).await.0, StatusCode::OK, "{path}");
            let (status, _) = hub
                .send(hub.request(path).body(Body::empty()).unwrap())
                .await;
            assert_eq!(status, StatusCode::NOT_FOUND, "{path} under the mount");
        }
    }
    // Without a readiness check, `/readyz` does not exist.
    let hub = TestHub::new(
        testing::for_domain::<FixtureDomain>()
            .handler(|| Hosted::new(FixtureDomain::new()))
            .build(),
    );
    assert_eq!(get_root(&hub, "/readyz").await.0, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn liveness_and_readiness_report_their_checks() {
    let hub = TestHub::new(
        testing::for_domain::<FixtureDomain>()
            .handler(|| Hosted::new(FixtureDomain::new()))
            .liveness(|| async { false })
            .readiness(|| async { false })
            .build(),
    );
    for (path, body) in [("/livez", "not alive"), ("/readyz", "not ready")] {
        let (status, text) = get_root(&hub, path).await;
        assert_eq!(
            (status, text.as_str()),
            (StatusCode::SERVICE_UNAVAILABLE, body)
        );
    }
}

#[tokio::test]
async fn internal_root_servers_route_from_the_listener_root() {
    let hub = TestHub::new(
        testing::internal_root_for_domain::<FixtureDomain>()
            .handler(|| Hosted::new(FixtureDomain::new()))
            .build(),
    );
    assert_eq!(get_root(&hub, "/livez").await.0, StatusCode::OK);
    let body = hub.rpc("tools/list", serde_json::json!({})).await;
    assert_eq!(body["result"]["tools"][0]["name"], "echo");
}

#[tokio::test]
async fn internal_servers_mount_at_their_slug_for_internal_hosts() {
    let hub = TestHub::new(
        testing::internal_for_domain::<FixtureDomain>()
            .handler(|| Hosted::new(FixtureDomain::new()))
            .build(),
    );
    assert_eq!(get_root(&hub, "/livez").await.0, StatusCode::OK);
    let body = hub.rpc("tools/list", serde_json::json!({})).await;
    assert_eq!(body["result"]["tools"][0]["name"], "echo");
    let (status, _) = hub
        .send(
            Request::builder()
                .uri("/livez")
                .header("host", "fixture.example")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(status, StatusCode::MISDIRECTED_REQUEST);
}

#[tokio::test]
async fn request_limits_bound_mcp_bodies_and_authenticated_responses() {
    let hub = TestHub::new(
        testing::for_domain::<FixtureDomain>()
            .handler(|| Hosted::new(FixtureDomain::new()))
            .admin_routes(axum::Router::new().route(
                "/slow",
                axum::routing::get(|| async {
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    "late"
                }),
            ))
            .mcp_request_limit(512)
            .request_timeout(std::time::Duration::from_millis(50))
            .build(),
    );
    let (status, _) = hub
        .rpc_with(
            "tools/call",
            serde_json::json!({"name": "echo", "arguments": {"text": "x".repeat(1024)}}),
            Some(&hub.token()),
        )
        .await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    let (status, _) = hub
        .send(
            hub.request("/admin/slow")
                .header("authorization", format!("Bearer {}", hub.token()))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(status, StatusCode::GATEWAY_TIMEOUT);
    let body = hub.rpc("tools/list", serde_json::json!({})).await;
    assert_eq!(body["result"]["tools"][0]["name"], "echo");
}

#[tokio::test]
async fn hosts_are_validated_before_routing() {
    let hub = hub();
    let (status, _) = hub
        .send(
            Request::builder()
                .uri("/livez")
                .header("host", "attacker.example")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(status, StatusCode::MISDIRECTED_REQUEST);
    let (status, _) = hub
        .send(
            Request::builder()
                .uri("/livez")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn the_installation_origins_host_is_admitted_beside_the_deployment_host() {
    let origin = crate::PublicOrigin::parse("https://veoveo.example.com").unwrap();
    let deployment = crate::PublicDeployment::new(format!("https://{}", testing::TEST_HOST))
        .unwrap()
        .with_origin(Some(origin));
    let hub = TestHub::new(
        super::HostedServer::for_domain::<FixtureDomain>()
            .deployment(&deployment, false)
            .unwrap()
            .with_verifier(testing::test_verifier::<FixtureDomain>())
            .handler(|| Hosted::new(FixtureDomain::new()))
            .build(),
    );
    let livez = |host: &str| {
        Request::builder()
            .uri("/livez")
            .header("host", host)
            .body(Body::empty())
            .unwrap()
    };
    assert_eq!(hub.send(livez(testing::TEST_HOST)).await.0, StatusCode::OK);
    assert_eq!(
        hub.send(livez("veoveo.example.com")).await.0,
        StatusCode::OK
    );
    assert_eq!(
        hub.send(livez("attacker.example")).await.0,
        StatusCode::MISDIRECTED_REQUEST
    );
}

#[tokio::test]
async fn rejected_requests_name_the_bearer_scheme() {
    let server = testing::for_domain::<FixtureDomain>()
        .handler(|| Hosted::new(FixtureDomain::new()))
        .build();
    let challenge = |bearer: Option<&str>| {
        let mut request = Request::builder()
            .method("POST")
            .uri(format!("{}/mcp", server.mount.prefix()))
            .header("host", testing::TEST_HOST);
        if let Some(bearer) = bearer {
            request = request.header("authorization", format!("Bearer {bearer}"));
        }
        let router = server.router.clone();
        async move {
            let response = tower::ServiceExt::oneshot(router, request.body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
            response.headers()["www-authenticate"]
                .to_str()
                .unwrap()
                .to_owned()
        }
    };
    assert_eq!(challenge(None).await, "Bearer");
    assert_eq!(
        challenge(Some("not-a-token")).await,
        r#"Bearer error="invalid_token""#
    );
}

#[tokio::test]
async fn mcp_and_admin_routes_require_a_hub_token() {
    let hub = hub();
    let (status, _) = hub
        .rpc_with("resources/list", serde_json::json!({}), None)
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = hub
        .rpc_with("resources/list", serde_json::json!({}), Some("not-a-token"))
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, _) = hub
        .send(
            hub.request("/admin/docs/llms.txt")
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, body) = hub
        .send(
            hub.request("/admin/docs/llms.txt")
                .header("authorization", format!("Bearer {}", hub.token()))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.contains("(agents)"));

    // Server admin routes sit beside the documents, behind the same check.
    let (status, _) = hub
        .send(hub.request("/admin/status").body(Body::empty()).unwrap())
        .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (status, body) = hub
        .send(
            hub.request("/admin/status")
                .header("authorization", format!("Bearer {}", hub.token()))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!((status, body.as_str()), (StatusCode::OK, "admin ok"));
}

#[tokio::test]
async fn discovery_comes_from_the_checked_setup() {
    let hub = hub();
    let body = hub.rpc("resources/list", serde_json::json!({})).await;
    let uris: Vec<_> = body["result"]["resources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|resource| resource["uri"].as_str().unwrap().to_owned())
        .collect();
    assert!(uris.contains(&"fixture://items".to_owned()));
    assert!(uris.contains(&"fixture://contract".to_owned()));
    // The domain adds instance resources; the host sorts the combined list.
    assert!(uris.contains(&"fixture://items/1".to_owned()));
    assert!(uris.is_sorted());
    assert_eq!(body["result"]["cacheScope"], "private");

    let body = hub
        .rpc("resources/templates/list", serde_json::json!({}))
        .await;
    assert_eq!(
        body["result"]["resourceTemplates"]
            .as_array()
            .unwrap()
            .len(),
        2
    );

    // The domain hides tools this caller lacks the scope for.
    let body = hub.rpc("tools/list", serde_json::json!({})).await;
    let tools = body["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["name"], "echo");
}

#[tokio::test]
async fn the_host_serves_contract_and_admits_domain_addresses() {
    let hub = hub();
    let body = hub
        .rpc(
            "resources/read",
            serde_json::json!({"uri": "fixture://contract"}),
        )
        .await;
    let contract: serde_json::Value =
        serde_json::from_str(body["result"]["contents"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(contract["server"], "fixture");

    let body = hub
        .rpc(
            "resources/read",
            serde_json::json!({"uri": "fixture://items"}),
        )
        .await;
    assert_eq!(body["result"]["contents"][0]["text"], r#"{"count":0}"#);
    assert_eq!(body["result"]["cacheScope"], "private");

    let body = hub
        .rpc(
            "resources/read",
            serde_json::json!({"uri": "fixture://unknown"}),
        )
        .await;
    assert_eq!(body["error"]["code"], -32602);
}

#[tokio::test]
async fn tools_receive_the_verified_caller() {
    let body = hub()
        .rpc(
            "tools/call",
            serde_json::json!({"name": "echo", "arguments": {"text": "hello"}}),
        )
        .await;
    assert_eq!(
        body["result"]["structuredContent"]["text"],
        "hello from user-1"
    );
}

#[tokio::test]
async fn a_refused_call_names_each_argument_by_its_pointer_and_type() {
    let body = hub()
        .rpc(
            "tools/call",
            serde_json::json!({"name": "echo", "arguments": {"text": 5}}),
        )
        .await;
    assert_eq!(body["result"]["isError"], true, "{body}");
    let content = body["result"]["content"].as_array().unwrap();
    let explained = content.last().unwrap()["text"].as_str().unwrap();
    assert!(explained.contains("input schema of `echo`"), "{body}");
    assert!(
        explained.contains(r#"`/text`: value is not of type "string""#),
        "{body}"
    );
    assert!(!body.to_string().contains("from user-1"), "{body}");
}

#[tokio::test]
async fn each_read_keeps_the_cache_policy_its_domain_chose() {
    let hub = hub();
    let body = hub
        .rpc(
            "resources/read",
            serde_json::json!({"uri": "fixture://items"}),
        )
        .await;
    assert_eq!(body["result"]["ttlMs"], crate::PRIVATE_RESOURCE_TTL_MS);
    let body = hub
        .rpc(
            "resources/read",
            serde_json::json!({"uri": "fixture://volatile"}),
        )
        .await;
    assert_eq!(body["result"]["ttlMs"], 0);
    assert_eq!(body["result"]["cacheScope"], "private");
}

#[tokio::test]
async fn prompts_come_from_registered_routes() {
    let hub = hub();
    let body = hub.rpc("prompts/list", serde_json::json!({})).await;
    assert_eq!(body["result"]["prompts"][0]["name"], "summarize");
    let body = hub
        .rpc("prompts/get", serde_json::json!({"name": "summarize"}))
        .await;
    assert_eq!(
        body["result"]["messages"][0]["content"]["text"],
        "Summarize fixture://items."
    );
    let body = hub
        .rpc("prompts/get", serde_json::json!({"name": "unknown"}))
        .await;
    assert_eq!(body["error"]["code"], -32602);
}

#[tokio::test]
async fn the_host_completes_document_ids() {
    let body = hub()
        .rpc(
            "completion/complete",
            serde_json::json!({
                "ref": {"type": "ref/resource", "uri": "fixture://docs/{doc_id}"},
                "argument": {"name": "doc_id", "value": "de"}
            }),
        )
        .await;
    assert_eq!(
        body["result"]["completion"]["values"],
        serde_json::json!(["design"])
    );
}

#[tokio::test]
async fn document_authorization_covers_mcp_reads_and_admin_routes() {
    let hub = hub();
    let allowed = hub
        .rpc(
            "resources/read",
            serde_json::json!({"uri": "fixture://docs/agents"}),
        )
        .await;
    assert!(allowed["result"]["contents"][0]["text"].is_string());
    let hub = hub.granting([]);
    let denied = hub
        .rpc(
            "resources/read",
            serde_json::json!({"uri": "fixture://docs/design"}),
        )
        .await;
    assert_eq!(denied["error"]["code"], -32600);
    let (status, _) = hub
        .send(
            hub.request("/admin/docs/design")
                .header("authorization", format!("Bearer {}", hub.token()))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn prompt_discovery_dispatch_and_registration_share_current_policy() {
    let hub = hub();
    assert_eq!(ROUTES.authorization().requirements.prompts.len(), 2);
    let listing = hub.rpc("prompts/list", serde_json::json!({})).await;
    assert_eq!(listing["result"]["prompts"].as_array().unwrap().len(), 1);
    let denied = hub
        .rpc("prompts/get", serde_json::json!({"name":"admin-summary"}))
        .await;
    assert_eq!(denied["error"]["code"], -32600);
    let allowed = hub
        .granting([veoveo_types::PermissionName::parse("fixture.admin.read").unwrap()])
        .rpc("prompts/get", serde_json::json!({"name":"admin-summary"}))
        .await;
    assert!(allowed["result"]["messages"].as_array().unwrap().is_empty());
}

#[test]
#[should_panic(expected = "duplicate typed fixed resource route")]
fn resource_construction_refuses_duplicate_fixed_identity() {
    Routes::<FixtureDomain>::builder(RoutePolicy::Authenticated)
        .singleton(
            ReadState(FixtureResource::Items),
            ResourceBuilder::new("first")
                .reader(|_, _, _| async { Ok(State { count: 0 }) })
                .policy(RoutePolicy::Authenticated)
                .cache(ReadCache::NoStore),
        )
        .singleton(
            ReadState(FixtureResource::Items),
            ResourceBuilder::new("second")
                .reader(|_, _, _| async { Ok(State { count: 1 }) })
                .policy(RoutePolicy::Authenticated)
                .cache(ReadCache::NoStore),
        );
}

#[test]
#[should_panic(expected = "duplicate typed resource template route")]
fn resource_construction_refuses_duplicate_template_identity() {
    Routes::<FixtureDomain>::builder(RoutePolicy::Authenticated)
        .template::<ReadNestedItem, _, _>(
            ResourceBuilder::new("first")
                .reader(|_, _, _| async { Ok(State { count: 0 }) })
                .policy(RoutePolicy::Authenticated)
                .cache(ReadCache::NoStore),
        )
        .template::<ReadNestedItem, _, _>(
            ResourceBuilder::new("second")
                .reader(|_, _, _| async { Ok(State { count: 1 }) })
                .policy(RoutePolicy::Authenticated)
                .cache(ReadCache::NoStore),
        );
}

#[test]
#[should_panic(expected = "collides with host-owned documents")]
fn resource_construction_reserves_host_documents() {
    Routes::<FixtureDomain>::builder(RoutePolicy::Authenticated).singleton(
        ReadState(FixtureResource::Docs),
        ResourceBuilder::new("override")
            .reader(|_, _, _| async { Ok(State { count: 0 }) })
            .policy(RoutePolicy::Authenticated)
            .cache(ReadCache::NoStore),
    );
}

#[test]
#[should_panic(expected = "conflicting typed route permission declaration")]
fn route_construction_refuses_conflicting_permission_metadata() {
    let RoutePolicy::Permission(mut conflicting) =
        RoutePolicy::permission(FixturePermission::Docs).unwrap()
    else {
        unreachable!()
    };
    conflicting.access = crate::registration::Access::Write;
    Routes::<FixtureDomain>::builder(RoutePolicy::permission(FixturePermission::Docs).unwrap())
        .tool::<EchoRequest, _, _>(
            "conflicting permission",
            RoutePolicy::Permission(conflicting),
            |_, request, _| async move { Ok(Echo { text: request.text }) },
        )
        .build();
}

/// The `data` of the next server-sent event on a stream.
async fn next_event(
    stream: &mut (impl futures::Stream<Item = Result<axum::body::Bytes, axum::Error>> + Unpin),
    buffer: &mut String,
) -> serde_json::Value {
    use futures::StreamExt;
    loop {
        if let Some(end) = buffer.find("\n\n") {
            let event = buffer[..end].to_owned();
            buffer.drain(..end + 2);
            if let Some(data) = event.lines().find_map(|line| line.strip_prefix("data:")) {
                let data = data.trim();
                if !data.is_empty() {
                    return serde_json::from_str(data).unwrap();
                }
            }
            continue;
        }
        let chunk = stream.next().await.expect("the stream stays open").unwrap();
        buffer.push_str(std::str::from_utf8(&chunk).unwrap());
    }
}

#[tokio::test]
async fn a_changed_description_is_listed_and_told_to_listeners() {
    tokio::time::timeout(std::time::Duration::from_secs(20), async {
        let tools = &SETUP.server_config().capabilities.tools;
        assert_eq!(
            tools.as_ref().and_then(|tools| tools.list_changed),
            Some(true),
            "a server with changing descriptions advertises tools.listChanged"
        );
        let hub = hub().granting([
            veoveo_types::PermissionName::parse("fixture.docs.read").unwrap(),
            veoveo_types::PermissionName::parse("fixture.admin.read").unwrap(),
        ]);
        let description = |body: serde_json::Value| {
            body["result"]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .find(|tool| tool["name"] == "admin_echo")
                .map(|tool| tool["description"].clone())
        };
        assert_eq!(
            description(hub.rpc("tools/list", serde_json::json!({})).await),
            Some("Echo for administrators".into())
        );

        let response = hub
            .open(
                "subscriptions/listen",
                serde_json::json!({"notifications": {"toolsListChanged": true}}),
            )
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        let mut events = response.into_body().into_data_stream();
        let mut buffer = String::new();
        let acknowledged = next_event(&mut events, &mut buffer).await;
        assert_eq!(
            acknowledged["method"],
            "notifications/subscriptions/acknowledged"
        );
        assert_eq!(
            acknowledged["params"]["notifications"]["toolsListChanged"],
            true
        );

        assert!(DESCRIPTIONS.set::<AdminRequest>("Echo for administrators, version 2"));
        let changed = next_event(&mut events, &mut buffer).await;
        assert_eq!(changed["method"], "notifications/tools/list_changed");
        assert_eq!(
            description(hub.rpc("tools/list", serde_json::json!({})).await),
            Some("Echo for administrators, version 2".into())
        );
        assert!(
            !DESCRIPTIONS.set::<AdminRequest>("Echo for administrators, version 2"),
            "the same description is no change"
        );
        // The unchanged tool keeps the description its route registered.
        let body = hub.rpc("tools/list", serde_json::json!({})).await;
        let echo = body["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == "echo")
            .unwrap()
            .clone();
        assert_eq!(echo["description"], "Echo text back to the verified caller");
    })
    .await
    .expect("the description fixture exceeded its bound");
}
