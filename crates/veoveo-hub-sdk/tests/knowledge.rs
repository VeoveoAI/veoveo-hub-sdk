//! The knowledge contract's wiring in a hosted server: the setup that declares or refuses
//! it, and the checks every search result passes before it leaves the server. The
//! records and the search are synthetic fixtures; no media is fetched or embedded.
use std::sync::LazyLock;

use rmcp::model::{Implementation, ServerConfig};
use serde_json::{Value, json};
use veoveo_hub_sdk::{
    calls::{ProviderContract, ResourceRead},
    docs::ServerDocs,
    hosting::{
        DomainServer, ReadCache, ResourceBuilder, RoutePolicy, Routes, ToolError,
        testing::{self, TestHub},
    },
    knowledge::{
        ContentPart, EXTENSION_ID, KnowledgeCollection, KnowledgeHit, KnowledgeQuery,
        KnowledgeSearch, MAX_SNIPPET_CHARS, SearchContext, Snippet,
    },
    registration::{Access, PermissionDefinition},
    server_contract::{McpServerContract, McpServerSetup, McpSetupError},
};
use veoveo_types::{PermissionName, ResourceAddress, ResourceScheme, ResourceUri, ServerSlug};

/// A URL whose query carries a signature, which nothing may echo.
const SIGNED: &str = "https://storage.veoveo.test/a.png?X-Amz-Signature=sig-0123456789";

struct Shop;
impl ProviderContract for Shop {
    fn slug() -> ServerSlug {
        ServerSlug::parse("shop").unwrap()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, veoveo_types::Vocabulary)]
#[vocabulary(scope)]
enum Scope {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, veoveo_types::Vocabulary)]
enum ShopPermission {
    #[vocabulary(rename = "shop.items.read")]
    ItemsRead,
}
impl PermissionDefinition for ShopPermission {
    fn resource(self) -> &'static str {
        "shop.item"
    }
    fn access(self) -> Access {
        Access::Read
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Address {
    Item(String),
    Part(String),
}
impl ResourceAddress for Address {
    type Error = veoveo_types::IdentifierError;
    fn parse(uri: &ResourceUri) -> Result<Self, Self::Error> {
        let invalid = || Self::Error::new(uri.as_str(), "not a shop address");
        let parts = uri.components().map_err(|_| invalid())?;
        let path: Vec<_> = parts.path_segments().collect();
        if parts.scheme() != "shop" || parts.has_query() || path.len() != 1 {
            return Err(invalid());
        }
        // Identifiers are lowercase; the route refuses any other.
        if !path[0].bytes().all(|byte| byte.is_ascii_lowercase()) {
            return Err(invalid());
        }
        match parts.authority() {
            "items" => Ok(Self::Item(path[0].to_string())),
            "parts" => Ok(Self::Part(path[0].to_string())),
            _ => Err(invalid()),
        }
    }
    fn to_uri(&self) -> Result<ResourceUri, Self::Error> {
        let (root, id) = match self {
            Self::Item(id) => ("items", id),
            Self::Part(id) => ("parts", id),
        };
        ResourceUri::new(format!("shop://{root}/{id}"))
            .map_err(|_| Self::Error::new(id, "not a shop address"))
    }
}

#[derive(
    serde::Serialize, serde::Deserialize, schemars::JsonSchema, veoveo_hub_sdk::SingleOutput,
)]
struct Record {
    name: String,
}

#[derive(Clone)]
#[veoveo_hub_sdk::resource_read(provider=Shop,address_type=Address,content=Record,
    address=Address::Item(self.id.clone()),template="shop://items/{item_id}",
    parse=|address|match address {Address::Item(id)=>Some(Self{id:id.clone()}),_=>None})]
struct ReadItem {
    id: String,
}

#[derive(Clone)]
#[veoveo_hub_sdk::resource_read(provider=Shop,address_type=Address,content=Record,
    address=Address::Part(self.id.clone()),template="shop://parts/{part_id}",
    parse=|address|match address {Address::Part(id)=>Some(Self{id:id.clone()}),_=>None})]
struct ReadPart {
    id: String,
}

fn collection(name: &str) -> KnowledgeCollection {
    KnowledgeCollection::parse(name).unwrap()
}

fn hit(uri: &str, name: &str) -> KnowledgeHit {
    KnowledgeHit {
        uri: ResourceUri::new(uri).unwrap(),
        collection: collection(name),
        title: format!("Record {uri}"),
        snippet: Snippet::new("A record"),
        modified_at: None,
    }
}

/// A search whose first text part chooses what it returns: a correct answer, or one
/// that breaks a check.
struct Search;
impl<D: DomainServer> KnowledgeSearch<D> for Search {
    async fn search(
        &self,
        _context: SearchContext<D>,
        query: KnowledgeQuery,
    ) -> Result<Vec<KnowledgeHit>, ToolError> {
        let text = query.parts.iter().find_map(|part| match part {
            ContentPart::Text { text } => Some(text.clone()),
            _ => None,
        });
        let item = || hit("shop://items/one", "shop.items");
        let part = || hit("shop://parts/two", "shop.parts");
        Ok(match text.as_deref() {
            Some("too many") => vec![item(); query.limit.get() as usize + 1],
            Some("unmarked collection") => vec![item(), hit("shop://items/one", "shop.orders")],
            Some("foreign address") => vec![hit("shop://orders/one", "shop.items")],
            Some("other template") => vec![hit("shop://parts/two", "shop.items")],
            Some("refused address") => vec![hit("shop://items/ONE", "shop.items")],
            Some("long snippet") => vec![KnowledgeHit {
                snippet: Snippet::new("x".repeat(400)),
                ..item()
            }],
            Some("rejected") => return Err(ToolError::Rejected("the shop is closed".into())),
            Some("ignores collections") => vec![item(), part()],
            _ => [item(), part()]
                .into_iter()
                .filter(|hit| query.searches(&hit.collection))
                .take(query.limit.get() as usize)
                .collect(),
        })
    }
}

async fn read_record<D: Send + 'static, R: Send + 'static>(
    _: D,
    _: R,
    _: rmcp::service::RequestContext<rmcp::RoleServer>,
) -> Result<Record, rmcp::ErrorData> {
    Ok(Record {
        name: "A record".into(),
    })
}

/// A ready template builder named `$name`, marked as `$collection` when one is given.
macro_rules! template {
    ($name:literal) => {
        ResourceBuilder::new($name)
            .reader(read_record)
            .policy(RoutePolicy::Authenticated)
            .cache(ReadCache::NoStore)
    };
    ($name:literal, $collection:literal) => {
        ResourceBuilder::new($name)
            .knowledge(collection($collection))
            .reader(read_record)
            .policy(RoutePolicy::Authenticated)
            .cache(ReadCache::NoStore)
    };
}

fn search_policy() -> RoutePolicy {
    RoutePolicy::permission(ShopPermission::ItemsRead).unwrap()
}

static DOCS: LazyLock<ServerDocs> = LazyLock::new(|| {
    ServerDocs::new(Shop::slug())
        .with_doc("agents", "Agents", "# Agents")
        .with_doc("design", "Design", "# Design")
});

macro_rules! shop {
    ($module:ident, $routes:expr) => {
        mod $module {
            use super::*;
            pub(super) struct Contract;
            #[derive(Clone)]
            pub(super) struct Domain;
            static SETUP: LazyLock<McpServerSetup<Contract>> =
                LazyLock::new(|| McpServerSetup::new().unwrap());
            static ROUTES: LazyLock<Routes<Domain>> = LazyLock::new($routes);
            impl McpServerContract for Contract {
                type Server = Domain;
                type Provider = Shop;
                type Scope = Scope;
                fn scheme() -> ResourceScheme {
                    ResourceScheme::parse("shop").unwrap()
                }
                fn scopes() -> &'static [Scope] {
                    &[]
                }
                fn documents() -> &'static ServerDocs {
                    &DOCS
                }
                fn server_config() -> ServerConfig {
                    let mut config = ServerConfig::default();
                    config.server_info = Implementation::new("shop", "1");
                    // A server cannot declare the extension itself; setup decides.
                    config
                        .capabilities
                        .extensions
                        .get_or_insert_default()
                        .insert(EXTENSION_ID.to_owned(), rmcp::model::JsonObject::new());
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

shop!(knowledge, || {
    Routes::builder(RoutePolicy::Authenticated)
        .template::<ReadItem, _, _>(template!("Item", "shop.items"))
        .template::<ReadPart, _, _>(template!("Part", "shop.parts"))
        .knowledge(search_policy(), Search)
        .build()
});
shop!(plain, || {
    Routes::builder(RoutePolicy::Authenticated)
        .template::<ReadItem, _, _>(template!("Item"))
        .build()
});
shop!(search_alone, || {
    Routes::builder(RoutePolicy::Authenticated)
        .template::<ReadItem, _, _>(template!("Item"))
        .knowledge(search_policy(), Search)
        .build()
});
shop!(marked_alone, || {
    Routes::builder(RoutePolicy::Authenticated)
        .template::<ReadItem, _, _>(template!("Item", "shop.items"))
        .build()
});
shop!(foreign, || {
    Routes::builder(RoutePolicy::Authenticated)
        .template::<ReadItem, _, _>(template!("Item", "market.items"))
        .knowledge(search_policy(), Search)
        .build()
});
shop!(twice, || {
    Routes::builder(RoutePolicy::Authenticated)
        .template::<ReadItem, _, _>(template!("Item", "shop.items"))
        .template::<ReadPart, _, _>(template!("Part", "shop.items"))
        .knowledge(search_policy(), Search)
        .build()
});

fn granted() -> PermissionName {
    PermissionName::parse("shop.items.read").unwrap()
}

fn hub() -> TestHub {
    TestHub::new(
        testing::for_domain::<knowledge::Domain>()
            .handler(|| veoveo_hub_sdk::hosting::Hosted::new(knowledge::Domain))
            .build(),
    )
    .granting([granted()])
}

async fn search(hub: &TestHub, arguments: Value) -> Value {
    hub.rpc(
        "tools/call",
        json!({"name": "search_knowledge", "arguments": arguments}),
    )
    .await
}

fn text(text: &str) -> Value {
    json!({"parts": [{"type": "text", "text": text}]})
}

#[test]
fn setup_declares_the_extension_exactly_when_the_routes_search_a_marked_collection() {
    let setup = McpServerSetup::<knowledge::Contract>::new().unwrap();
    let extensions = setup.server_config().capabilities.extensions.as_ref();
    assert_eq!(
        extensions.and_then(|extensions| extensions.get(EXTENSION_ID)),
        Some(&rmcp::model::JsonObject::new())
    );
    let marked: Vec<_> = setup
        .resource_templates()
        .iter()
        .filter_map(|template| template.descriptor().meta.as_ref())
        .map(|meta| meta.0[EXTENSION_ID].clone())
        .collect();
    assert_eq!(
        marked,
        [
            json!({"collection": "shop.items"}),
            json!({"collection": "shop.parts"})
        ]
    );
    let plain = McpServerSetup::<plain::Contract>::new().unwrap();
    assert!(
        plain
            .server_config()
            .capabilities
            .extensions
            .as_ref()
            .is_none_or(|extensions| !extensions.contains_key(EXTENSION_ID))
    );
}

#[test]
fn setup_refuses_a_search_or_a_collection_that_does_not_hold_together() {
    assert_eq!(
        McpServerSetup::<search_alone::Contract>::new().err(),
        Some(McpSetupError::KnowledgeSearchWithoutCollection)
    );
    assert_eq!(
        McpServerSetup::<marked_alone::Contract>::new().err(),
        Some(McpSetupError::KnowledgeCollectionWithoutSearch)
    );
    assert_eq!(
        McpServerSetup::<foreign::Contract>::new().err(),
        Some(McpSetupError::ForeignKnowledgeCollection)
    );
    assert_eq!(
        McpServerSetup::<twice::Contract>::new().err(),
        Some(McpSetupError::DuplicateKnowledgeCollection)
    );
}

#[test]
#[should_panic(expected = "the knowledge search requires a read permission")]
fn the_search_requires_a_read_permission() {
    #[derive(
        Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, veoveo_types::Vocabulary,
    )]
    enum Write {
        #[vocabulary(rename = "shop.items.write")]
        ItemsWrite,
    }
    impl PermissionDefinition for Write {
        fn resource(self) -> &'static str {
            "shop.item"
        }
        fn access(self) -> Access {
            Access::Write
        }
    }
    let _ = Routes::<knowledge::Domain>::builder(RoutePolicy::Authenticated)
        .knowledge(RoutePolicy::permission(Write::ItemsWrite).unwrap(), Search);
}

#[tokio::test]
async fn a_shop_that_searches_its_records_passes_the_knowledge_test() {
    let answers = veoveo_hub_sdk::testing::knowledge(&hub(), "anything").await;
    for results in answers.values() {
        assert_eq!(results.hits().len(), 2);
    }
}

#[tokio::test]
async fn the_search_answers_with_its_hits_and_a_resource_link_each() {
    let hub = hub();
    let answer = search(&hub, text("records")).await;
    let result = &answer["result"];
    assert_eq!(result["isError"], false, "{answer}");
    assert_eq!(
        result["structuredContent"],
        json!({"hits": [
            {"uri": "shop://items/one", "collection": "shop.items",
             "title": "Record shop://items/one", "snippet": "A record"},
            {"uri": "shop://parts/two", "collection": "shop.parts",
             "title": "Record shop://parts/two", "snippet": "A record"}
        ]})
    );
    let links: Vec<&Value> = result["content"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["type"] == "resource_link")
        .collect();
    assert_eq!(links.len(), 2);
    assert_eq!(links[0]["uri"], "shop://items/one");
    assert_eq!(links[0]["name"], "Record shop://items/one");
    assert_eq!(links[1]["uri"], "shop://parts/two");
    let long = search(&hub, text("long snippet")).await;
    let snippet = long["result"]["structuredContent"]["hits"][0]["snippet"]
        .as_str()
        .unwrap();
    assert_eq!(snippet.chars().count(), MAX_SNIPPET_CHARS);
    let rejected = search(&hub, text("rejected")).await;
    assert_eq!(rejected["result"]["isError"], true, "{rejected}");
    assert_eq!(
        rejected["result"]["content"][0]["text"],
        "the shop is closed"
    );
}

#[tokio::test]
async fn a_hit_that_fails_a_check_fails_the_call_instead_of_being_dropped() {
    let hub = hub();
    for (query, failure) in [
        (text("too many"), "more hits than the query's limit"),
        (
            json!({"parts": [{"type": "text", "text": "too many"}], "limit": 1}),
            "more hits than the query's limit",
        ),
        (
            text("unmarked collection"),
            "no template of this server marks",
        ),
        (
            text("foreign address"),
            "not one its collection's template serves",
        ),
        (
            text("other template"),
            "not one its collection's template serves",
        ),
        (
            text("refused address"),
            "not one its collection's template serves",
        ),
        (
            json!({"parts": [{"type": "text", "text": "ignores collections"}], "collections": ["shop.parts"]}),
            "not one the query names",
        ),
    ] {
        let answer = search(&hub, query.clone()).await;
        assert_eq!(answer["error"]["code"], -32603, "{query}: {answer}");
        let message = answer["error"]["message"].as_str().unwrap();
        assert!(message.contains(failure), "{query}: {message}");
        assert!(!message.contains("shop://"), "{message}");
    }
    let named = search(
        &hub,
        json!({"parts": [{"type": "text", "text": "records"}], "collections": ["shop.items", "shop.parts"]}),
    )
    .await;
    assert_eq!(named["result"]["isError"], false, "{named}");
}

#[tokio::test]
async fn arguments_that_are_not_a_query_are_invalid_params_that_never_echo_a_signed_url() {
    let hub = hub();
    let image = |media_type: &str, url: &str| json!({"parts": [{"type": "image", "url": url, "mediaType": media_type, "size": 10}]});
    for arguments in [
        json!({}),
        json!({"parts": []}),
        json!({"parts": [{"type": "text", "text": "x"}], "limit": 21}),
        json!({"parts": [{"type": "text", "text": "x"}], "collections": ["shop"]}),
        json!({"parts": [{"type": "text", "text": "x"}], "collections": (0..17).map(|index| format!("shop.c{index}")).collect::<Vec<_>>()}),
        json!({"parts": [{"type": "text", "text": "x"}], "collections": ["shop.items", "shop.items"]}),
        json!({"parts": [{"type": "text", "text": "x"}], "extra": SIGNED}),
        json!({"parts": [{"type": "image", "url": SIGNED, "mediaType": "image/png", "size": SIGNED}]}),
        image("audio/wav", SIGNED),
        image("image/png", &format!("ftp{}", &SIGNED[5..])),
        image("image/png", &format!("{SIGNED} spaced")),
    ] {
        let answer = search(&hub, arguments.clone()).await;
        assert_eq!(answer["error"]["code"], -32602, "{arguments}: {answer}");
        let text = answer.to_string();
        assert!(!text.contains("sig-0123456789"), "{text}");
        assert!(!text.contains("storage.veoveo.test"), "{text}");
    }
    let unmarked = search(
        &hub,
        json!({"parts": [{"type": "text", "text": "x"}], "collections": ["shop.orders"]}),
    )
    .await;
    assert_eq!(unmarked["error"]["code"], -32602, "{unmarked}");
    assert!(
        unmarked["error"]["message"]
            .as_str()
            .unwrap()
            .contains("shop.orders")
    );
    // A query the schema admits but whose image is audio names the rules it breaks.
    let mismatched = search(&hub, image("audio/wav", SIGNED)).await;
    assert!(
        mismatched["error"]["message"]
            .as_str()
            .unwrap()
            .contains("of its type"),
        "{mismatched}"
    );
}

#[tokio::test]
async fn the_search_runs_only_for_a_caller_its_policy_admits() {
    let hub = hub();
    let token = hub.token_granting([]);
    let (_, denied) = hub
        .rpc_with(
            "tools/call",
            json!({"name": "search_knowledge", "arguments": text("records")}),
            Some(&token),
        )
        .await;
    assert!(denied["error"].is_object(), "{denied}");
    let (_, listed) = hub.rpc_with("tools/list", json!({}), Some(&token)).await;
    assert!(
        listed["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .all(|tool| tool["name"] != "search_knowledge"),
        "{listed}"
    );
    let tools = hub.rpc("tools/list", json!({})).await;
    let tool = tools["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tool| tool["name"] == "search_knowledge")
        .cloned()
        .unwrap();
    assert_eq!(tool["annotations"]["readOnlyHint"], true);
    assert_eq!(
        ShopPermission::ItemsRead.resource(),
        "shop.item",
        "the policy is the shop's own read permission"
    );
    let item = ReadItem { id: "one".into() };
    assert_eq!(item.uri().unwrap().as_str(), "shop://items/one");
}
