//! Bulletin, a server whose one tool's description changes while it runs: a test sets
//! [`DESCRIPTIONS`] and sees the server, and a Hub in front of it, tell their listeners
//! that the tool list changed.
use std::sync::LazyLock;

use rmcp::model::{ServerCapabilities, ServerConfig};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use veoveo_hub_sdk::{
    calls::{ProviderContract, ToolCall},
    hosting::{DomainServer, Hosted, RoutePolicy, Routes, ToolDescriptions},
    registration::AuthorizationDeclaration,
    server_contract::{McpServerContract, McpServerSetup},
};
use veoveo_types::{ResourceScheme, ServerSlug};

use crate::calls_support::{NoScope, contract_config, docs};

/// The description `read_bulletin` registers, served until a test sets another.
pub const DESCRIPTION: &str = "Read the bulletin";

/// Bulletin's changing descriptions.
pub static DESCRIPTIONS: LazyLock<ToolDescriptions> = LazyLock::new(ToolDescriptions::new);

pub struct BulletinContract;
static DOCS: LazyLock<veoveo_hub_sdk::docs::ServerDocs> = LazyLock::new(|| docs("bulletin"));
static SETUP: LazyLock<McpServerSetup<BulletinContract>> =
    LazyLock::new(|| McpServerSetup::new().expect("Bulletin setup"));

impl ProviderContract for BulletinContract {
    fn slug() -> ServerSlug {
        ServerSlug::parse("bulletin").unwrap()
    }
}

impl McpServerContract for BulletinContract {
    type Server = Bulletin;
    type Scope = NoScope;
    type Provider = BulletinContract;
    fn scheme() -> ResourceScheme {
        ResourceScheme::parse("bulletin").unwrap()
    }
    fn scopes() -> &'static [NoScope] {
        NoScope::ALL
    }
    fn documents() -> &'static veoveo_hub_sdk::docs::ServerDocs {
        &DOCS
    }
    fn server_config() -> ServerConfig {
        contract_config(
            "bulletin",
            ServerCapabilities::builder().enable_tools().build(),
        )
    }
}

/// The arguments of `read_bulletin`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReadBulletin {}
impl ToolCall for ReadBulletin {
    type Provider = BulletinContract;
    type Output = Note;
    const TOOL: &'static str = "read_bulletin";
}

/// The bulletin's text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Note {
    pub text: String,
}
impl veoveo_hub_sdk::definition::SingleOutput for Note {}

#[derive(Clone)]
pub struct Bulletin;
static ROUTES: LazyLock<Routes<Bulletin>> = LazyLock::new(|| {
    Routes::builder(RoutePolicy::Authenticated)
        .tool::<ReadBulletin, _, _>(
            DESCRIPTION,
            RoutePolicy::Authenticated,
            |_, _, _| async move {
                Ok(Note {
                    text: "Nothing new".into(),
                })
            },
        )
        .descriptions(DESCRIPTIONS.clone())
        .build()
});
impl DomainServer for Bulletin {
    type Contract = BulletinContract;
    fn setup() -> &'static McpServerSetup<BulletinContract> {
        &SETUP
    }
    fn routes() -> &'static Routes<Self> {
        &ROUTES
    }
}
pub fn authorization() -> AuthorizationDeclaration {
    ROUTES.authorization()
}
pub fn hosted() -> Hosted<Bulletin> {
    Hosted::new(Bulletin)
}
