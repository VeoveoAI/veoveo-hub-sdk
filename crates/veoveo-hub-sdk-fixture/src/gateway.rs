//! A third independent native server: Gateway calls Relay, which calls Ledger.
use std::sync::LazyLock;

use rmcp::model::{ServerCapabilities, ServerConfig};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use veoveo_hub_sdk::{
    calls::{ProviderContract, ToolCall},
    hosting::{ClientContext, DomainServer, Hosted, RoutePolicy, Routes},
    registration::{Access, AuthorizationDeclaration, PermissionDefinition, Risk},
    server_contract::{McpServerContract, McpServerSetup},
};
use veoveo_types::{PermissionName, ResourceScheme, ServerSlug};

use crate::{
    calls::{Inspection, RelayPermission},
    calls_support::{NoScope, contract_config, docs},
};

pub struct GatewayContract;
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, veoveo_types::Vocabulary)]
enum GatewayPermission {
    #[vocabulary(rename = "gateway.calls.make")]
    CallsMake,
}
impl PermissionDefinition for GatewayPermission {
    fn resource(self) -> &'static str {
        "gateway.call"
    }
    fn access(self) -> Access {
        Access::Read
    }
    fn risks(self) -> &'static [Risk] {
        &[]
    }
}
static DOCS: LazyLock<veoveo_hub_sdk::docs::ServerDocs> = LazyLock::new(|| docs("gateway"));
static SETUP: LazyLock<McpServerSetup<GatewayContract>> =
    LazyLock::new(|| McpServerSetup::new().expect("Gateway setup"));

impl ProviderContract for GatewayContract {
    fn slug() -> ServerSlug {
        ServerSlug::parse("gateway").unwrap()
    }
}

impl McpServerContract for GatewayContract {
    type Server = Gateway;
    type Scope = NoScope;
    type Provider = GatewayContract;
    fn scheme() -> ResourceScheme {
        ResourceScheme::parse("gateway").unwrap()
    }
    fn scopes() -> &'static [NoScope] {
        NoScope::ALL
    }
    fn documents() -> &'static veoveo_hub_sdk::docs::ServerDocs {
        &DOCS
    }
    fn server_config() -> ServerConfig {
        contract_config(
            "gateway",
            ServerCapabilities::builder().enable_tools().build(),
        )
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GatewayInspect {}
impl ToolCall for GatewayInspect {
    type Provider = GatewayContract;
    type Output = Inspection;
    const TOOL: &'static str = "gateway_inspect";
}

#[derive(Clone)]
pub struct Gateway;
static ROUTES: LazyLock<Routes<Gateway>> = LazyLock::new(|| {
    Routes::builder(RoutePolicy::Authenticated)
        .uses(PermissionName::parse(RelayPermission::CallsMake.as_str()).unwrap())
        .tool::<GatewayInspect, _, _>(
            "Inspect through Relay and Ledger",
            RoutePolicy::permission(GatewayPermission::CallsMake).unwrap(),
            |_, _, context| async move {
                context
                    .client()?
                    .call(crate::relay::InspectRelay {})
                    .await
                    .map_err(Into::<veoveo_hub_sdk::hosting::ToolError>::into)
            },
        )
        .build()
});
impl DomainServer for Gateway {
    type Contract = GatewayContract;
    fn setup() -> &'static McpServerSetup<GatewayContract> {
        &SETUP
    }
    fn routes() -> &'static Routes<Self> {
        &ROUTES
    }
}
pub fn authorization() -> AuthorizationDeclaration {
    ROUTES.authorization()
}
pub fn hosted() -> Hosted<Gateway> {
    Hosted::new(Gateway)
}
