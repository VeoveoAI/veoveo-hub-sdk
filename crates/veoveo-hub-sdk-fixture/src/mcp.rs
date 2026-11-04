//! Observatory uses the same typed resource and list-tool declarations as clients.
use crate::contract::{
    ObservatoryScope, ReadReading, ReadReadings, Reading, ReadingCollection, ReadingId,
    ReadingsTool, reading_knowledge,
};
use rmcp::{
    ErrorData, RoleServer,
    model::{Implementation, ServerConfig},
    service::RequestContext,
};
use std::sync::LazyLock;
use veoveo_hub_sdk::{
    calls::ResourceRead,
    definition::{CollectionRequest, List, ReadCollection},
    hosting::{
        DomainServer, Hosted, ReadCache, ResourceBuilder, RoutePolicy, Routes, ToolError,
        hub_identity,
    },
    knowledge::{
        ContentPart, KnowledgeHit, KnowledgeQuery, KnowledgeSearch, SearchContext, Snippet,
    },
    server_contract::{McpServerContract, McpServerSetup},
};
use veoveo_types::{LiveKeyset, Page, Pager, ResourceScheme, TenantId};

pub struct ObservatoryContract;
static DOCUMENTS: LazyLock<veoveo_hub_sdk::docs::ServerDocs> =
    LazyLock::new(|| veoveo_hub_sdk::server_docs!("observatory"));
static SKILLS: LazyLock<veoveo_hub_sdk::skills::ServerSkills> =
    LazyLock::new(|| veoveo_hub_sdk::server_skills!("observatory"));
pub static SETUP: LazyLock<McpServerSetup<ObservatoryContract>> =
    LazyLock::new(|| McpServerSetup::new().expect("typed Observatory setup"));
impl McpServerContract for ObservatoryContract {
    type Server = ObservatoryMcp;
    type Scope = ObservatoryScope;
    type Provider = crate::contract::Observatory;
    fn scheme() -> ResourceScheme {
        ResourceScheme::parse("observatory").unwrap()
    }
    fn scopes() -> &'static [ObservatoryScope] {
        ObservatoryScope::ALL
    }
    fn documents() -> &'static veoveo_hub_sdk::docs::ServerDocs {
        &DOCUMENTS
    }
    fn skills() -> &'static veoveo_hub_sdk::skills::ServerSkills {
        &SKILLS
    }
    fn server_config() -> ServerConfig {
        let mut config = ServerConfig::default();
        config.server_info = Implementation::new("observatory", env!("CARGO_PKG_VERSION"));
        config
    }
}
#[derive(Clone)]
pub struct ObservatoryMcp;
fn readings() -> Vec<Reading> {
    ["sensor-a", "sensor-b", "sensor-c"]
        .into_iter()
        .enumerate()
        .map(|(i, id)| Reading {
            id: ReadingId::new(id).unwrap(),
            value: 7 + i as i32,
        })
        .collect()
}
fn page(
    selection: CollectionRequest<ReadingCollection>,
    tenant: TenantId,
) -> Result<Page<ReadingCollection>, ErrorData> {
    let (filters, limit, cursor) = selection.into_parts();
    Pager::<ReadingCollection>::new(tenant, filters, limit, LiveKeyset, cursor.as_ref())
        .and_then(|pager| pager.select(readings()))
        .map_err(|_| ErrorData::invalid_params("invalid reading continuation", None))
}
async fn read_reading(
    _: ObservatoryMcp,
    read: ReadReading,
    _: RequestContext<RoleServer>,
) -> Result<Reading, ErrorData> {
    readings()
        .into_iter()
        .find(|reading| reading.id == read.id)
        .ok_or_else(|| ErrorData::resource_not_found("unknown reading", None))
}
async fn read_readings(
    _: ObservatoryMcp,
    read: ReadCollection<ReadReadings>,
    context: RequestContext<RoleServer>,
) -> Result<Page<ReadingCollection>, ErrorData> {
    let identity = hub_identity(&context)?;
    page(read.into_selection(), identity.authority.tenant)
}
async fn list_readings(
    _: ObservatoryMcp,
    request: List<ReadingsTool>,
    context: RequestContext<RoleServer>,
) -> Result<Page<ReadingCollection>, ErrorData> {
    let identity = hub_identity(&context)?;
    page(request.into_selection(), identity.authority.tenant)
}
/// Observatory's knowledge search, over the same synthetic readings, in memory. A query
/// with text finds the readings whose identifier holds one of its words; a query of media
/// alone, which a real server embeds, answers every reading. No media is fetched.
pub struct ReadingSearch;
impl KnowledgeSearch<ObservatoryMcp> for ReadingSearch {
    async fn search(
        &self,
        context: SearchContext<ObservatoryMcp>,
        query: KnowledgeQuery,
    ) -> Result<Vec<KnowledgeHit>, ToolError> {
        hub_identity(&context.request)?;
        let words: Vec<String> = query
            .parts
            .iter()
            .filter_map(|part| match part {
                ContentPart::Text { text } => Some(text.to_lowercase()),
                _ => None,
            })
            .flat_map(|text| {
                text.split_whitespace()
                    .map(str::to_owned)
                    .collect::<Vec<_>>()
            })
            .collect();
        readings()
            .into_iter()
            .filter(|reading| {
                words.is_empty() || words.iter().any(|word| reading.id.as_str().contains(word))
            })
            .take(query.limit.get() as usize)
            .map(|reading| {
                let uri = ReadReading {
                    id: reading.id.clone(),
                }
                .uri()
                .map_err(|_| ErrorData::internal_error("a reading's address", None))?;
                Ok(KnowledgeHit {
                    uri,
                    collection: reading_knowledge(),
                    title: format!("Reading {}", reading.id.as_str()),
                    snippet: Snippet::new(format!(
                        "{} reads {}",
                        reading.id.as_str(),
                        reading.value
                    )),
                    modified_at: None,
                })
            })
            .collect()
    }
}
static ROUTES: LazyLock<Routes<ObservatoryMcp>> = LazyLock::new(|| {
    Routes::builder(RoutePolicy::Authenticated)
        .template::<ReadReading, _, _>(
            ResourceBuilder::new("Reading")
                .reader(read_reading)
                .policy(RoutePolicy::Authenticated)
                .cache(ReadCache::Private)
                .knowledge(reading_knowledge()),
        )
        .knowledge(RoutePolicy::Authenticated, ReadingSearch)
        .collection_resource::<ReadReadings, _, _>(
            Some(ReadReadings {}),
            ResourceBuilder::new("Readings")
                .reader(read_readings)
                .policy(RoutePolicy::Authenticated)
                .cache(ReadCache::Private),
        )
        .collection::<ReadingsTool, _, _>(
            "Read a page of readings",
            RoutePolicy::Authenticated,
            list_readings,
        )
        .build()
});
impl DomainServer for ObservatoryMcp {
    type Contract = ObservatoryContract;
    fn setup() -> &'static McpServerSetup<Self::Contract> {
        &SETUP
    }
    fn routes() -> &'static Routes<Self> {
        &ROUTES
    }
}
pub fn hosted() -> Hosted<ObservatoryMcp> {
    Hosted::new(ObservatoryMcp)
}
