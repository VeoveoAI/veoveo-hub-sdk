use rmcp::model::{Implementation, ServerConfig};
use std::{collections::BTreeSet, sync::LazyLock};
use veoveo_hub_sdk::{
    calls::{ProviderContract, ResourceRead},
    definition::{Product, SingleOutput, TemplateRead},
    docs::ServerDocs,
    hosting::{DomainServer, ReadCache, ResourceBuilder, RoutePolicy, Routes},
    server_contract::{McpServerContract, McpServerSetup, McpSetupError},
};
use veoveo_types::{ResourceAddress, ResourceScheme, ResourceUri, ScopeName, ServerSlug};
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, veoveo_types::Vocabulary)]
#[vocabulary(scope)]
enum Scope {
    #[vocabulary(rename = "independent:read")]
    Read,
    #[vocabulary(rename = "independent:write")]
    Write,
}
struct Independent;
impl ProviderContract for Independent {
    fn slug() -> ServerSlug {
        ServerSlug::parse("independent").unwrap()
    }
}
#[derive(Clone, PartialEq, Eq)]
enum Address {
    Product { id: String },
}
#[derive(
    serde::Serialize, serde::Deserialize, schemars::JsonSchema, veoveo_hub_sdk::SingleOutput,
)]
struct Metadata {
    label: String,
}
#[derive(Clone)]
#[veoveo_hub_sdk::resource_read(provider=Independent,address_type=Address,content=Metadata,
    address=Address::Product{id:self.id.clone()},template="independent://products/{id}",
    parse=|address|match address {Address::Product{id}=>Some(Self{id:id.clone()})})]
struct ReadProduct {
    id: String,
}
impl ResourceAddress for Address {
    type Error = veoveo_types::IdentifierError;
    fn parse(uri: &ResourceUri) -> Result<Self, Self::Error> {
        let invalid = || Self::Error::new(uri.as_str(), "invalid product address");
        let parts = uri.components().map_err(|_| invalid())?;
        let path: Vec<_> = parts.path_segments().collect();
        if parts.scheme() != "independent"
            || parts.authority() != "products"
            || parts.has_query()
            || path.len() != 1
        {
            return Err(invalid());
        }
        Ok(Self::Product {
            id: path[0].to_string(),
        })
    }
    fn to_uri(&self) -> Result<ResourceUri, Self::Error> {
        let Self::Product { id } = self;
        let invalid = || Self::Error::new("product", "invalid product address");
        veoveo_types::ResourceUriBuilder::from_components(
            &ResourceScheme::parse("independent").unwrap(),
            veoveo_types::UriAuthority::new("products").unwrap(),
        )
        .map_err(|_| invalid())?
        .segment(veoveo_types::UriSegment::new(id).map_err(|_| invalid())?)
        .build()
        .map_err(|_| invalid())
    }
}
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[veoveo_hub_sdk::tool(provider=Independent,output=Product<Metadata>,name="produce_product")]
struct Produce {}
const SERIALIZATION_SENTINEL: &str = "fixture-private-serialization-payload";
#[derive(serde::Deserialize, schemars::JsonSchema, veoveo_hub_sdk::SingleOutput)]
struct Unserializable {}
impl serde::Serialize for Unserializable {
    fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
        Err(serde::ser::Error::custom(SERIALIZATION_SENTINEL))
    }
}
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[veoveo_hub_sdk::tool(provider=Independent,output=Unserializable,name="fail_serialization")]
struct FailSerialization {}
fn documents(owner: &str) -> ServerDocs {
    ServerDocs::new(ServerSlug::parse(owner).unwrap())
        .with_doc("agents", "Agents", "# Agents")
        .with_doc("design", "Design", "# Design")
}
static DOCUMENTS: LazyLock<ServerDocs> = LazyLock::new(|| documents("independent"));
static FOREIGN: LazyLock<ServerDocs> = LazyLock::new(|| documents("foreign"));
static EMPTY: LazyLock<ServerDocs> = LazyLock::new(|| ServerDocs::new(Independent::slug()));
static BLANK: LazyLock<ServerDocs> =
    LazyLock::new(|| documents("independent").with_doc("extra", "Extra", " "));
fn routes<D: DomainServer>() -> Routes<D>
where
    D::Contract: McpServerContract<Provider = Independent>,
{
    Routes::builder(RoutePolicy::Authenticated)
        .tool::<Produce, _, _>(
            "Create a product",
            RoutePolicy::Authenticated,
            |_, _, _| async {
                Product::new(
                    &Address::Product { id: "one".into() },
                    Metadata {
                        label: "ready".into(),
                    },
                )
                .map_err(|_| {
                    rmcp::ErrorData::internal_error("fixture product construction", None).into()
                })
            },
        )
        .tool::<FailSerialization, _, _>(
            "Fail while serializing typed output",
            RoutePolicy::Authenticated,
            |_, _, _| async { Ok(Unserializable {}) },
        )
        .template::<ReadProduct, _, _>(
            ResourceBuilder::new("Product")
                .reader(|_, _: ReadProduct, _| async {
                    Ok(Metadata {
                        label: "ready".into(),
                    })
                })
                .policy(RoutePolicy::Authenticated)
                .cache(ReadCache::Private),
        )
        .build()
}
macro_rules! fixture {
    ($module:ident,$docs:ident,$name:literal,$scopes:expr) => {
        fixture!($module, $docs, $name, $scopes, false);
    };
    ($module:ident,$docs:ident,$name:literal,$scopes:expr,$registered:expr) => {
        mod $module {
            use super::*;
            pub(super) struct Contract;
            #[derive(Clone)]
            pub(super) struct Domain;
            static SETUP: LazyLock<McpServerSetup<Contract>> =
                LazyLock::new(|| McpServerSetup::new().unwrap());
            static ROUTES: LazyLock<Routes<Domain>> = LazyLock::new(routes);
            impl McpServerContract for Contract {
                type Server = Domain;
                type Provider = Independent;
                type Scope = Scope;
                fn scheme() -> ResourceScheme {
                    ResourceScheme::parse("independent").unwrap()
                }
                fn scopes() -> &'static [Scope] {
                    $scopes
                }
                fn documents() -> &'static ServerDocs {
                    &$docs
                }
                fn server_config() -> ServerConfig {
                    let mut config = ServerConfig::default();
                    config.server_info = Implementation::new($name, "1");
                    config
                        .capabilities
                        .extensions
                        .get_or_insert_default()
                        .insert(
                            veoveo_hub_sdk::registration::AUTHORIZATION_EXTENSION_ID.to_owned(),
                            rmcp::model::JsonObject::new(),
                        );
                    if $registered {
                        use veoveo_hub_sdk::registration::{
                            AuthorizationDeclaration, RegistrationDeclaration, Requirements,
                            ServerEndpoint, ServerVersion, mcp,
                        };
                        let declaration = RegistrationDeclaration {
                            slug: Independent::slug(),
                            uri_scheme: Self::scheme(),
                            version: ServerVersion::new("1").unwrap(),
                            endpoint: ServerEndpoint::new(
                                "https://independent.example/mcp".parse().unwrap(),
                            )
                            .unwrap(),
                        };
                        mcp::advertise(
                            &mut config.capabilities,
                            &declaration,
                            &AuthorizationDeclaration {
                                permissions: vec![],
                                requirements: Requirements::default(),
                                uses: vec![],
                            },
                        );
                    }
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
        }
    };
}
fixture!(valid, DOCUMENTS, "independent", &[Scope::Read]);
fixture!(registered, DOCUMENTS, "independent", &[Scope::Read], true);
fixture!(wrong_name, DOCUMENTS, "foreign", &[Scope::Read]);
fixture!(wrong_owner, FOREIGN, "independent", &[Scope::Read]);
fixture!(missing_docs, EMPTY, "independent", &[Scope::Read]);
fixture!(blank_doc, BLANK, "independent", &[Scope::Read]);
fixture!(
    duplicate_scope,
    DOCUMENTS,
    "independent",
    &[Scope::Read, Scope::Read]
);
fixture!(empty_scope, DOCUMENTS, "independent", &[]);
#[test]
fn scope_admission_preserves_unknown_grants_and_empty_vocabularies() {
    let setup = McpServerSetup::<valid::Contract>::new().unwrap();
    let grants = BTreeSet::from([
        Scope::Read.into(),
        Scope::Write.into(),
        ScopeName::parse("other:read").unwrap(),
    ]);
    assert!(setup.has_scope(&grants, Scope::Read));
    assert!(!setup.has_scope(&grants, Scope::Write));
    assert!(!setup.has_scope(&BTreeSet::new(), Scope::Read));
    assert!(
        McpServerSetup::<empty_scope::Contract>::new()
            .unwrap()
            .scope_names()
            .is_empty()
    );
    assert_eq!(
        McpServerSetup::<duplicate_scope::Contract>::new().err(),
        Some(McpSetupError::DuplicateScope)
    );
}
#[test]
fn host_supplies_capabilities_and_document_descriptors_from_construction() {
    let setup = McpServerSetup::<valid::Contract>::new().unwrap();
    assert!(setup.server_config().capabilities.resources.is_some());
    assert!(setup.server_config().capabilities.completions.is_some());
    assert!(setup.server_config().capabilities.tools.is_some());
    assert_eq!(setup.resources().len(), 4);
    assert!(
        setup
            .resources()
            .windows(2)
            .all(|pair| pair[0].address() < pair[1].address())
    );
    assert!(
        setup
            .resource_templates()
            .iter()
            .any(|template| template.template().as_str() == ReadProduct::TEMPLATE)
    );
    assert!(
        veoveo_hub_sdk::registration::mcp::advertised(&setup.server_config().capabilities)
            .unwrap()
            .is_none()
    );
    let registered = McpServerSetup::<registered::Contract>::new().unwrap();
    let advertised =
        veoveo_hub_sdk::registration::mcp::advertised(&registered.server_config().capabilities)
            .unwrap()
            .unwrap();
    assert_eq!(advertised.registration.slug, Independent::slug());
    assert_eq!(
        advertised.authorization,
        registered::Domain::routes().authorization()
    );
}
#[test]
fn semantic_identity_and_document_errors_are_still_refused() {
    assert_eq!(
        McpServerSetup::<wrong_name::Contract>::new().err(),
        Some(McpSetupError::IdentityMismatch)
    );
    assert_eq!(
        McpServerSetup::<wrong_owner::Contract>::new().err(),
        Some(McpSetupError::IdentityMismatch)
    );
    assert_eq!(
        McpServerSetup::<missing_docs::Contract>::new().err(),
        Some(McpSetupError::MissingDocument)
    );
    assert_eq!(
        McpServerSetup::<blank_doc::Contract>::new().err(),
        Some(McpSetupError::MissingDocument)
    );
}
#[test]
fn product_construction_and_received_uri_admission_share_one_shape() {
    let read = ReadProduct { id: "one".into() };
    let product = Product::new(
        &read.address(),
        Metadata {
            label: "ready".into(),
        },
    )
    .unwrap();
    assert_eq!(
        SingleOutput::product_uri(&product),
        Some(&read.uri().unwrap())
    );
    let wire = serde_json::to_value(product).unwrap();
    assert_eq!(wire["resultUri"], "independent://products/one");
    assert_eq!(wire["metadata"]["label"], "ready");
    for invalid in [
        serde_json::json!({"metadata":{"label":"ready"}}),
        serde_json::json!({"resultUri":"not a URI","metadata":{"label":"ready"}}),
    ] {
        assert!(serde_json::from_value::<Product<Metadata>>(invalid).is_err());
    }
}
#[cfg(feature = "testing")]
#[tokio::test]
async fn hosted_product_output_text_and_link_come_from_the_same_value() {
    use veoveo_hub_sdk::hosting::{
        Hosted,
        testing::{self, TestHub},
    };
    let hub = TestHub::new(
        testing::for_domain::<valid::Domain>()
            .handler(|| Hosted::new(valid::Domain))
            .build(),
    );
    let response = hub
        .rpc(
            "tools/call",
            serde_json::json!({"name":"produce_product","arguments":{}}),
        )
        .await;
    let output = &response["result"]["structuredContent"];
    assert_eq!(
        output["resultUri"], "independent://products/one",
        "{response}"
    );
    let content = response["result"]["content"].as_array().unwrap();
    let links: Vec<_> = content
        .iter()
        .filter(|block| block["type"] == "resource_link")
        .collect();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0]["uri"], output["resultUri"]);
    let text = content
        .iter()
        .find_map(|block| block["text"].as_str())
        .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(text).unwrap(),
        *output
    );
}

#[cfg(feature = "testing")]
mod security {
    use super::*;
    use std::{
        io::Write,
        sync::{Arc, Mutex},
        time::Duration,
    };
    use tracing::instrument::WithSubscriber;
    use veoveo_hub_sdk::hosting::{
        Hosted,
        testing::{self, TestHub},
    };

    struct TraceWriter(Arc<Mutex<Vec<u8>>>);
    impl Write for TraceWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn traces_preserve_routing_without_query_or_credentials() {
        tokio::time::timeout(Duration::from_secs(20), async {
            let captured = Arc::new(Mutex::new(Vec::new()));
            let writer = captured.clone();
            let subscriber = tracing_subscriber::fmt()
                .with_max_level(tracing::Level::TRACE)
                .with_ansi(false)
                .without_time()
                .with_span_events(tracing_subscriber::fmt::format::FmtSpan::FULL)
                .with_writer(move || TraceWriter(writer.clone()))
                .finish();
            let hub = TestHub::new(
                testing::for_domain::<valid::Domain>()
                    .handler(|| Hosted::new(valid::Domain))
                    .public_routes(axum::Router::new().route(
                        "/trace-query",
                        axum::routing::get(|uri: axum::http::Uri| async move {
                            uri.query().unwrap_or_default().to_owned()
                        }),
                    ))
                    .build(),
            );
            let query = "query-sentinel=query-value&access_token=synthetic-query-token&signature=synthetic-signature%2Fvalue";
            let request = hub
                .request(&format!("/trace-query?{query}"))
                .method("GET")
                .version(axum::http::Version::HTTP_11)
                .header("authorization", "Bearer synthetic-header-token")
                .header("cookie", "fixture=synthetic-cookie")
                .body(axum::body::Body::empty())
                .unwrap();
            let expected_route = request.uri().path().to_owned();
            let (status, body) = hub.send(request).with_subscriber(subscriber).await;
            assert_eq!(status.as_u16(), 200);
            assert_eq!(body, query, "the handler must receive the original query");
            let trace = String::from_utf8(captured.lock().unwrap().clone()).unwrap();
            assert!(trace.contains("method=GET"), "{trace}");
            assert!(trace.contains(&format!("http.route={expected_route}")), "{trace}");
            assert!(trace.contains("version=HTTP/1.1"), "{trace}");
            for sentinel in [
                "query-sentinel", "query-value", "access_token", "synthetic-query-token",
                "synthetic-signature", "synthetic-header-token", "synthetic-cookie",
            ] {
                assert!(!trace.contains(sentinel), "trace leaked {sentinel}: {trace}");
            }
        })
        .await
        .expect("trace redaction test exceeded twenty seconds");
    }

    #[tokio::test]
    async fn traces_record_templates_never_secret_or_unmatched_paths() {
        tokio::time::timeout(Duration::from_secs(20), async {
            let captured = Arc::new(Mutex::new(Vec::new()));
            let writer = captured.clone();
            let subscriber = tracing_subscriber::fmt()
                .with_max_level(tracing::Level::TRACE)
                .with_ansi(false)
                .without_time()
                .with_span_events(tracing_subscriber::fmt::format::FmtSpan::FULL)
                .with_writer(move || TraceWriter(writer.clone()))
                .finish();
            let hub = TestHub::new(
                testing::for_domain::<valid::Domain>()
                    .handler(|| Hosted::new(valid::Domain))
                    .public_routes(
                        axum::Router::new()
                            .route("/shares/{token}", axum::routing::get(|| async { "shared" })),
                    )
                    .build(),
            );
            let secret = "synthetic-share-token";
            let shared = hub
                .request(&format!("/shares/{secret}?signature=synthetic-share-query"))
                .method("GET")
                .body(axum::body::Body::empty())
                .unwrap();
            let template = format!(
                "{}/shares/{{token}}",
                shared
                    .uri()
                    .path()
                    .trim_end_matches(&format!("/shares/{secret}"))
            );
            let unmatched = hub
                .request("/no-such-route/synthetic-unmatched-path?q=synthetic-unmatched-query")
                .method("GET")
                .body(axum::body::Body::empty())
                .unwrap();
            async {
                let (status, body) = hub.send(shared).await;
                assert_eq!((status.as_u16(), body.as_str()), (200, "shared"));
                assert_eq!(hub.send(unmatched).await.0.as_u16(), 404);
            }
            .with_subscriber(subscriber)
            .await;
            let trace = String::from_utf8(captured.lock().unwrap().clone()).unwrap();
            assert!(trace.contains(&format!("http.route={template}")), "{trace}");
            assert!(trace.contains("http.route=unmatched"), "{trace}");
            for sentinel in [
                secret,
                "synthetic-share-query",
                "synthetic-unmatched-path",
                "synthetic-unmatched-query",
            ] {
                assert!(
                    !trace.contains(sentinel),
                    "trace leaked {sentinel}: {trace}"
                );
            }
        })
        .await
        .expect("path redaction test exceeded twenty seconds");
    }

    #[tokio::test]
    async fn typed_output_serialization_failure_keeps_diagnostics_payload_free() {
        tokio::time::timeout(Duration::from_secs(20), async {
            let hub = TestHub::new(
                testing::for_domain::<valid::Domain>()
                    .handler(|| Hosted::new(valid::Domain))
                    .build(),
            );
            let response = hub
                .rpc(
                    "tools/call",
                    serde_json::json!({"name":"fail_serialization","arguments":{}}),
                )
                .await;
            assert_eq!(response["error"]["code"], -32603);
            assert_eq!(
                response["error"]["message"],
                "typed output serialization failed"
            );
            assert!(response["error"].get("data").is_none());
            assert!(response.get("result").is_none());
            assert!(!response.to_string().contains(SERIALIZATION_SENTINEL));
        })
        .await
        .expect("serialization redaction test exceeded twenty seconds");
    }
}
