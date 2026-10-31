//! MCP Apps declared in typed routes: an App's listing, read, registration requirement
//! and capability, and the `_meta.ui` links of the tools it may invoke, through the
//! in-process Hub.
use std::{path::Path, sync::LazyLock};

use rmcp::{
    ServerHandler,
    model::{Icon, Implementation, ServerCapabilities, ServerConfig},
};
use serde_json::{Value, json};
use veoveo_hub_sdk::{
    apps::{
        APP_MIME_TYPE, AppHtml, AppUri, EXTENSION_ID, ResourceUiMeta, UI_META_KEY, UiCsp,
        UiVisibility,
    },
    calls::ProviderContract,
    docs::ServerDocs,
    hosting::{
        AppBuilder, DomainServer, Hosted, RoutePolicy, Routes,
        testing::{self, TestHub},
    },
    registration::{Access, PermissionDefinition, RouteAccess},
    server_contract::{McpServerContract, McpServerSetup},
};
use veoveo_types::{PermissionName, ResourceScheme, ResourceUri, ServerSlug};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, veoveo_types::Vocabulary)]
#[vocabulary(scope)]
enum Scope {}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, veoveo_types::Vocabulary)]
enum Permission {
    #[vocabulary(rename = "viewer.read")]
    Read,
}
impl PermissionDefinition for Permission {
    fn resource(self) -> &'static str {
        "viewer.board"
    }
    fn access(self) -> Access {
        Access::Read
    }
}
fn read() -> RoutePolicy {
    RoutePolicy::permission(Permission::Read).unwrap()
}
fn read_permission() -> PermissionName {
    PermissionName::parse("viewer.read").unwrap()
}

struct Viewer;
impl ProviderContract for Viewer {
    fn slug() -> ServerSlug {
        ServerSlug::parse("viewer").unwrap()
    }
}

#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[veoveo_hub_sdk::tool(provider=Viewer,output=Shown,name="show")]
struct Show {}
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[veoveo_hub_sdk::tool(provider=Viewer,output=Shown,name="convert")]
struct Convert {}
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[veoveo_hub_sdk::tool(provider=Viewer,output=Shown,name="administer")]
struct Administer {}
#[derive(
    serde::Serialize, serde::Deserialize, schemars::JsonSchema, veoveo_hub_sdk::SingleOutput,
)]
struct Shown {
    shown: bool,
}

const BOARD: &str = "ui://viewer/board";
const ICON: &str =
    "data:image/svg+xml;base64,PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciLz4=";

fn board() -> AppUri {
    AppUri::parse(BOARD).unwrap()
}
fn ui() -> ResourceUiMeta {
    ResourceUiMeta {
        csp: Some(UiCsp {
            connect_domains: vec!["https://live.viewer.example".to_owned()],
            ..UiCsp::default()
        }),
        prefers_border: Some(false),
    }
}
/// The App document, read once as a server reads its packaged asset at startup.
static HTML: LazyLock<AppHtml> = LazyLock::new(|| {
    AppHtml::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/app-view.html")).unwrap()
});
static DOCS: LazyLock<ServerDocs> = LazyLock::new(|| {
    ServerDocs::new(Viewer::slug())
        .with_doc("agents", "Agents", "# Agents")
        .with_doc("design", "Design", "# Design")
});

/// The tools every fixture serves; only the App fixture links any.
fn tools<D: DomainServer>() -> veoveo_hub_sdk::hosting::RoutesBuilder<D>
where
    D::Contract: McpServerContract<Provider = Viewer>,
{
    Routes::builder(RoutePolicy::Authenticated)
        .tool::<Show, _, _>(
            "Show the board",
            RoutePolicy::Authenticated,
            |_, _, _| async { Ok(Shown { shown: true }) },
        )
        .tool::<Convert, _, _>(
            "Convert a value",
            RoutePolicy::Authenticated,
            |_, _, _| async { Ok(Shown { shown: true }) },
        )
        .tool::<Administer, _, _>("Administer the board", read(), |_, _, _| async {
            Ok(Shown { shown: true })
        })
}

macro_rules! fixture {
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
                type Provider = Viewer;
                type Scope = Scope;
                fn scheme() -> ResourceScheme {
                    ResourceScheme::parse("viewer").unwrap()
                }
                fn scopes() -> &'static [Scope] {
                    Scope::ALL
                }
                fn documents() -> &'static ServerDocs {
                    &DOCS
                }
                fn server_config() -> ServerConfig {
                    let mut capabilities = ServerCapabilities::builder().enable_tools().build();
                    // A server's own claim of the UI extension does not survive setup.
                    veoveo_hub_sdk::apps::extend_capabilities(&mut capabilities);
                    let mut config = ServerConfig::new(capabilities);
                    config.server_info = Implementation::new("viewer", "1");
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
            pub(super) fn hub() -> TestHub {
                TestHub::new(
                    testing::for_domain::<Domain>()
                        .handler(|| Hosted::new(Domain))
                        .build(),
                )
                .granting([read_permission()])
            }
        }
    };
}

// Each fixture's routes expand inside its module, where `Domain` is its domain.
fixture!(with_app, || {
    tools::<Domain>()
        .app(
            AppBuilder::new(board(), "board")
                .title("Board")
                .description("The viewer's board")
                .icon(Icon::new(ICON).with_mime_type("image/svg+xml"))
                .ui(ui())
                .policy(read())
                .reader(|_, _| async { Ok(HTML.clone()) }),
        )
        .app_tools(
            &board(),
            &["show", "convert"],
            &[UiVisibility::Model, UiVisibility::App],
        )
        .build()
});
fixture!(without_app, || tools::<Domain>().build());

fn listed<'a>(list: &'a Value, member: &str, key: &str, value: &str) -> Option<&'a Value> {
    list["result"][member]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry[key] == value)
}

#[tokio::test]
async fn discovery_lists_the_app_with_its_mime_type_and_fields() {
    let hub = with_app::hub();
    let resources = hub.rpc("resources/list", json!({})).await;
    let app = listed(&resources, "resources", "uri", BOARD).expect("the App is listed");
    assert_eq!(
        *app,
        json!({
            "uri": BOARD,
            "name": "board",
            "title": "Board",
            "description": "The viewer's board",
            "mimeType": APP_MIME_TYPE,
            "icons": [{"src": ICON, "mimeType": "image/svg+xml"}],
            "_meta": {"ui": {"csp": {"connectDomains": ["https://live.viewer.example"]}, "prefersBorder": false}}
        })
    );
    // The App's policy admits its listing as it admits its read.
    let denied = hub.granting([]).rpc("resources/list", json!({})).await;
    assert!(listed(&denied, "resources", "uri", BOARD).is_none());
}

#[tokio::test]
async fn a_read_returns_the_document_under_the_app_mime_type() {
    let hub = with_app::hub();
    let read = hub.rpc("resources/read", json!({"uri": BOARD})).await;
    assert_eq!(
        read["result"]["contents"],
        json!([{"uri": BOARD, "mimeType": APP_MIME_TYPE, "text": HTML.as_str()}])
    );
    assert_eq!(read["result"]["cacheScope"], "private");
    let denied = with_app::hub()
        .granting([])
        .rpc("resources/read", json!({"uri": BOARD}))
        .await;
    assert_eq!(denied["error"]["code"], -32600, "{denied}");
    let unknown = hub
        .rpc("resources/read", json!({"uri": "ui://viewer/other"}))
        .await;
    assert_eq!(unknown["error"]["code"], -32602, "{unknown}");
}

#[tokio::test]
async fn linked_tools_carry_the_app_link_and_unlinked_tools_none() {
    let hub = with_app::hub();
    let tools = hub.rpc("tools/list", json!({})).await;
    let link = json!({"resourceUri": BOARD, "visibility": ["model", "app"]});
    for name in ["show", "convert"] {
        let tool = listed(&tools, "tools", "name", name).unwrap();
        assert_eq!(tool["_meta"][UI_META_KEY], link, "{name}");
    }
    let unlinked = listed(&tools, "tools", "name", "administer").unwrap();
    assert!(unlinked.get("_meta").is_none(), "{unlinked}");
    // Tool lookup serves the same descriptors.
    let handler = Hosted::new(with_app::Domain);
    let looked_up = handler.get_tool("show").unwrap();
    assert_eq!(
        serde_json::to_value(looked_up.meta.unwrap()).unwrap()[UI_META_KEY],
        link
    );
    assert!(handler.get_tool("administer").unwrap().meta.is_none());
    // A server without Apps links nothing.
    let plain = without_app::hub().rpc("tools/list", json!({})).await;
    for tool in plain["result"]["tools"].as_array().unwrap() {
        assert!(tool.get("_meta").is_none(), "{tool}");
    }
}

#[test]
fn the_ui_extension_is_declared_only_with_an_app() {
    let declared = |capabilities: &ServerCapabilities| {
        capabilities
            .extensions
            .as_ref()
            .and_then(|extensions| extensions.get(EXTENSION_ID))
            .map(|declaration| Value::Object(declaration.clone()))
    };
    assert_eq!(
        declared(&with_app::Domain::setup().server_config().capabilities),
        Some(json!({"mimeTypes": [APP_MIME_TYPE]}))
    );
    assert_eq!(
        declared(&without_app::Domain::setup().server_config().capabilities),
        None
    );
}

#[test]
fn registration_requirements_cover_the_app_address() {
    let authorization = with_app::Domain::routes().authorization();
    assert_eq!(
        authorization
            .requirements
            .resources
            .get(&ResourceUri::new(BOARD).unwrap()),
        Some(&RouteAccess::Permission(read_permission()))
    );
    authorization
        .check(&Viewer::slug(), &ResourceScheme::parse("viewer").unwrap())
        .unwrap();
    let plain = without_app::Domain::routes().authorization();
    assert!(
        plain
            .requirements
            .resources
            .keys()
            .all(|uri| !uri.as_str().starts_with("ui://"))
    );
}

/// A ready App at `$uri` that serves the fixture's document.
macro_rules! app {
    ($uri:expr) => {
        AppBuilder::new(AppUri::parse($uri).unwrap(), "board")
            .policy(read())
            .reader(|_, _| async { Ok(HTML.clone()) })
    };
}

#[test]
#[should_panic(expected = "App address lies outside the server's ui://{slug}/")]
fn an_app_outside_the_servers_slug_is_refused() {
    let _ = tools::<with_app::Domain>().app(app!("ui://other/board"));
}

#[test]
#[should_panic(expected = "App icons are data: URIs of images")]
fn an_icon_that_is_not_an_image_data_uri_is_refused() {
    let _ = tools::<with_app::Domain>()
        .app(app!(BOARD).icon(Icon::new("https://viewer.example/icon.svg")));
}

#[test]
#[should_panic(expected = "duplicate typed fixed resource route")]
fn an_app_declared_twice_is_refused() {
    let _ = tools::<with_app::Domain>()
        .app(app!(BOARD))
        .app(app!(BOARD));
}

#[test]
#[should_panic(expected = "App tool links name a tool the routes do not declare")]
fn a_link_to_an_unknown_tool_is_refused() {
    let _ = tools::<with_app::Domain>()
        .app(app!(BOARD))
        .app_tools(&board(), &["show", "missing"], &[UiVisibility::App])
        .build();
}

#[test]
#[should_panic(expected = "App tool links name an App the routes do not declare")]
fn a_link_to_an_unknown_app_is_refused() {
    let _ = tools::<with_app::Domain>()
        .app(app!(BOARD))
        .app_tools(
            &AppUri::parse("ui://viewer/elsewhere").unwrap(),
            &["show"],
            &[UiVisibility::App],
        )
        .build();
}

#[test]
#[should_panic(expected = "a tool links to one App")]
fn a_tool_linked_twice_is_refused() {
    let _ = tools::<with_app::Domain>()
        .app(app!(BOARD))
        .app(app!("ui://viewer/other"))
        .app_tools(&board(), &["show"], &[UiVisibility::App])
        .app_tools(
            &AppUri::parse("ui://viewer/other").unwrap(),
            &["show"],
            &[UiVisibility::App],
        )
        .build();
}
