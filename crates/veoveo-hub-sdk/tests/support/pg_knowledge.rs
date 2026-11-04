//! A synthetic shop server whose items (UUID keys) and notes (text keys) are knowledge
//! sources, over a private PostgreSQL 18.6 cluster with pgvector, and a synthetic
//! embedder: a word-hash vector, not a model's.
#![allow(dead_code, reason = "each test uses a subset of the fixture")]
use std::{
    sync::{Arc, LazyLock},
    time::Duration,
};

use rmcp::{
    ErrorData, RoleServer,
    model::{
        CallToolRequestParams, CreateTaskResult, GetTaskParams, GetTaskResult, Implementation,
        ServerConfig, UpdateTaskParams,
    },
    service::RequestContext,
};
use serde_json::{Value, json};
use sqlx::PgPool;
use tokio_util::sync::{CancellationToken, DropGuard};
use uuid::Uuid;
use veoveo_fixtures::store::TestDb;
use veoveo_hub_sdk::{
    HubInternalIdentity,
    calls::ProviderContract,
    client::{CallError, HubClient, StaticBearer},
    docs::ServerDocs,
    durable_tasks::{
        DurableTaskService, DurableTaskSubscription, DurableTasks, TaskWorker, cancel_durable_task,
        get_durable_task, subscribe_durable_tasks, task_owner, update_durable_task,
    },
    hosting::{
        ClientContext, DomainServer, Hosted, ReadCache, ResourceBuilder, RoutePolicy, Routes,
        ToolError, hub_identity,
        testing::{self, RunningMesh, TestMesh},
    },
    hub::HubToolName,
    knowledge::{
        ContentPart, ContentParts, Embedder, EmbeddingDimensions, EmbeddingModel, EmbeddingPurpose,
        Embeddings, Indexed, IndexedRecord, KnowledgeCollection, KnowledgeHit, ModelCheckpoint,
        PgKnowledge, PgKnowledgeError, PgKnowledgeSource, PgKnowledgeTable, Snippet, WriteIndexing,
    },
    registration::{Access, PermissionDefinition},
    server_contract::{McpServerContract, McpServerSetup},
};
use veoveo_task::{Instance, TaskError, TaskFailure, TaskRuntime, TaskTransition};
use veoveo_types::{PermissionName, ResourceAddress, ResourceScheme, ResourceUri, ServerSlug};

/// The shop's migration: two knowledge tables, as the authoring guide gives them, and
/// their read rules. A record is readable in its tenant when it is public or its owner
/// calls.
const SCHEMA: &str = "CREATE TABLE shop.item (
    item_id uuid PRIMARY KEY,
    tenant uuid NOT NULL,
    owner uuid NOT NULL,
    public boolean NOT NULL,
    name text NOT NULL,
    knowledge_text tsvector GENERATED ALWAYS AS (to_tsvector('simple', name)) STORED,
    knowledge_vector _extensions.vector(768),
    knowledge_model text
);
CREATE INDEX item_knowledge_text ON shop.item USING gin (knowledge_text);
CREATE INDEX item_knowledge_vector ON shop.item USING hnsw (knowledge_vector _extensions.vector_cosine_ops);
CREATE FUNCTION shop.item_readable(r shop.item, caller jsonb) RETURNS boolean
LANGUAGE sql STABLE PARALLEL SAFE SET search_path = pg_catalog, pg_temp
RETURN r.tenant = (caller->>'tenantId')::uuid
    AND (r.public OR r.owner = (caller->>'principalId')::uuid);
CREATE TABLE shop.note (
    note_key text PRIMARY KEY,
    tenant uuid NOT NULL,
    public boolean NOT NULL,
    body text NOT NULL,
    knowledge_text tsvector GENERATED ALWAYS AS (to_tsvector('simple', body)) STORED,
    knowledge_vector _extensions.vector(768),
    knowledge_model text
);
CREATE INDEX note_knowledge_text ON shop.note USING gin (knowledge_text);
CREATE INDEX note_knowledge_vector ON shop.note USING hnsw (knowledge_vector _extensions.vector_cosine_ops);
CREATE FUNCTION shop.note_readable(r shop.note, caller jsonb) RETURNS boolean
LANGUAGE sql STABLE PARALLEL SAFE SET search_path = pg_catalog, pg_temp
RETURN r.tenant = (caller->>'tenantId')::uuid AND r.public
    AND caller->>'tenantKey' = 'tenant-a'
    AND jsonb_typeof(caller->'groups') = 'array';";

/// A synthetic embedder: each word counts in one of 700 dimensions by a fixed hash, each
/// media part in a dimension of its modality. Text holding `fail-embedding` fails.
#[derive(Debug, Clone, Copy, Default)]
pub struct Synthetic;

#[derive(Debug, thiserror::Error)]
#[error("synthetic embedding failure")]
pub struct SyntheticFailure;

impl Synthetic {
    pub const CHECKPOINT: &'static str = "synthetic/word-hash@1";

    pub fn vector(parts: &ContentParts) -> Vec<f32> {
        let mut vector = vec![0.0_f32; 768];
        for part in parts {
            match part {
                ContentPart::Text { text } => {
                    for word in text.split_whitespace() {
                        let slot = word.to_lowercase().bytes().fold(7_usize, |hash, byte| {
                            hash.wrapping_mul(31) + usize::from(byte)
                        }) % 700;
                        vector[slot] += 1.0;
                    }
                }
                ContentPart::Image(_) => vector[701] += 1.0,
                ContentPart::Audio(_) => vector[702] += 1.0,
                ContentPart::Video(_) => vector[703] += 1.0,
            }
        }
        if vector.iter().all(|value| *value == 0.0) {
            vector[767] = 1.0;
        }
        vector
    }

    /// The vector of a query of one image.
    pub fn media_vector() -> Vec<f32> {
        let mut vector = vec![0.0_f32; 768];
        vector[701] = 1.0;
        vector
    }

    /// A query of one synthetic image, which nothing fetches.
    pub fn image_query() -> Value {
        json!({"parts":[{"type":"image","url":"https://media.veoveo.test/synthetic/image?X-Amz-Signature=synthetic",
            "mediaType":"image/png","size":3}]})
    }

    fn image() -> ContentPart {
        serde_json::from_value(Self::image_query()["parts"][0].clone()).unwrap()
    }
}

impl Embedder for Synthetic {
    type Error = SyntheticFailure;
    async fn embed(
        &self,
        _client: &HubClient,
        items: &[ContentParts],
        _purpose: EmbeddingPurpose,
    ) -> Result<Embeddings, SyntheticFailure> {
        let failing = items.iter().flatten().any(
            |part| matches!(part, ContentPart::Text { text } if text.contains("fail-embedding")),
        );
        if failing {
            return Err(SyntheticFailure);
        }
        Ok(Embeddings::new(
            EmbeddingModel {
                checkpoint: ModelCheckpoint::parse(Self::CHECKPOINT).unwrap(),
                dimensions: EmbeddingDimensions::new(768).unwrap(),
            },
            items.iter().map(Self::vector).collect(),
        )
        .unwrap())
    }
}

/// How [`Interleaving`] writes its record while it embeds it.
#[derive(Debug, Clone)]
pub enum Change {
    /// Renames it during the first Embedding call only.
    RenameOnce(String),
    /// Renames it during every Embedding call.
    RenameAlways,
    /// Deletes it during the first Embedding call.
    Delete,
}

/// The synthetic embedder, which during its calls writes the record `key` as another
/// writer would, committed before it answers for the content it was given.
#[derive(Clone)]
pub struct Interleaving {
    pool: PgPool,
    key: Uuid,
    change: Change,
    texts: Arc<parking_lot::Mutex<Vec<String>>>,
}

impl Interleaving {
    pub fn new(pool: &PgPool, key: Uuid, change: Change) -> Self {
        Self {
            pool: pool.clone(),
            key,
            change,
            texts: Arc::default(),
        }
    }

    /// The text of each item of each call, in order.
    pub fn texts(&self) -> Vec<String> {
        self.texts.lock().clone()
    }
}

impl Embedder for Interleaving {
    type Error = SyntheticFailure;
    async fn embed(
        &self,
        client: &HubClient,
        items: &[ContentParts],
        purpose: EmbeddingPurpose,
    ) -> Result<Embeddings, SyntheticFailure> {
        let call = {
            let mut texts = self.texts.lock();
            let call = texts.len();
            texts.extend(items.iter().flatten().filter_map(|part| match part {
                ContentPart::Text { text } => Some(text.clone()),
                _ => None,
            }));
            call
        };
        let rename = |name: String| {
            sqlx::query("UPDATE shop.item SET name = $2 WHERE item_id = $1")
                .bind(self.key)
                .bind(name)
        };
        let write = match (&self.change, call) {
            (Change::RenameOnce(name), 0) => Some(rename(name.clone())),
            (Change::RenameAlways, call) => Some(rename(format!("renamed {call}"))),
            (Change::Delete, 0) => {
                Some(sqlx::query("DELETE FROM shop.item WHERE item_id = $1").bind(self.key))
            }
            _ => None,
        };
        if let Some(write) = write {
            write
                .execute(&self.pool)
                .await
                .expect("the interleaved write");
        }
        Synthetic.embed(client, items, purpose).await
    }
}

#[derive(Debug, thiserror::Error)]
#[error("synthetic unreadable content")]
pub struct Unreadable;

/// The shop's items: UUID keys.
#[derive(Debug, Clone, Copy)]
pub struct Items;
/// The shop's notes: text keys.
#[derive(Debug, Clone, Copy)]
pub struct Notes;

fn collection(name: &str) -> KnowledgeCollection {
    KnowledgeCollection::parse(name).unwrap()
}

impl PgKnowledgeSource<ShopDomain> for Items {
    type Key = Uuid;
    type Error = Unreadable;
    fn collection(&self) -> KnowledgeCollection {
        collection("shop.items")
    }
    fn table(&self) -> PgKnowledgeTable {
        PgKnowledgeTable::new("item", "item_id", "item_readable").unwrap()
    }
    fn pool<'a>(&self, domain: &'a ShopDomain) -> &'a PgPool {
        &domain.pool
    }
    async fn hits(
        &self,
        domain: &ShopDomain,
        _request: &RequestContext<RoleServer>,
        keys: &[Uuid],
    ) -> Result<Vec<Option<KnowledgeHit>>, ToolError> {
        let rows: Vec<(Uuid, String)> =
            sqlx::query_as("SELECT item_id, name FROM shop.item WHERE item_id = ANY ($1)")
                .bind(keys)
                .fetch_all(&domain.pool)
                .await
                .map_err(|_| ErrorData::internal_error("synthetic hits", None))?;
        Ok(keys
            .iter()
            .map(|key| {
                rows.iter()
                    .find(|(id, _)| id == key)
                    .map(|(id, name)| KnowledgeHit {
                        uri: ResourceUri::new(format!("shop://items/{id}")).unwrap(),
                        collection: collection("shop.items"),
                        title: name.clone(),
                        snippet: Snippet::new(name.clone()),
                        modified_at: None,
                    })
            })
            .collect())
    }
    async fn content(
        &self,
        domain: &ShopDomain,
        _client: &HubClient,
        key: Uuid,
    ) -> Result<Option<ContentParts>, Unreadable> {
        let name: Option<String> =
            sqlx::query_scalar("SELECT name FROM shop.item WHERE item_id = $1")
                .bind(key)
                .fetch_optional(&domain.pool)
                .await
                .map_err(|_| Unreadable)?;
        let Some(name) = name else { return Ok(None) };
        if name.starts_with("broken") {
            return Err(Unreadable);
        }
        let mut parts = vec![ContentPart::text(name.clone())];
        if name.starts_with("image") {
            parts.push(Synthetic::image());
        }
        Ok(Some(ContentParts::new(parts).unwrap()))
    }
}

impl PgKnowledgeSource<ShopDomain> for Notes {
    type Key = String;
    type Error = Unreadable;
    fn collection(&self) -> KnowledgeCollection {
        collection("shop.notes")
    }
    fn table(&self) -> PgKnowledgeTable {
        PgKnowledgeTable::new("note", "note_key", "note_readable").unwrap()
    }
    fn pool<'a>(&self, domain: &'a ShopDomain) -> &'a PgPool {
        &domain.pool
    }
    async fn hits(
        &self,
        domain: &ShopDomain,
        _request: &RequestContext<RoleServer>,
        keys: &[String],
    ) -> Result<Vec<Option<KnowledgeHit>>, ToolError> {
        let rows: Vec<(String, String)> =
            sqlx::query_as("SELECT note_key, body FROM shop.note WHERE note_key = ANY ($1)")
                .bind(keys)
                .fetch_all(&domain.pool)
                .await
                .map_err(|_| ErrorData::internal_error("synthetic hits", None))?;
        Ok(keys
            .iter()
            .map(|key| {
                rows.iter()
                    .find(|(id, _)| id == key)
                    .map(|(id, body)| KnowledgeHit {
                        uri: ResourceUri::new(format!("shop://notes/{id}")).unwrap(),
                        collection: collection("shop.notes"),
                        title: body.clone(),
                        snippet: Snippet::new(body.clone()),
                        modified_at: None,
                    })
            })
            .collect())
    }
    async fn content(
        &self,
        domain: &ShopDomain,
        _client: &HubClient,
        key: String,
    ) -> Result<Option<ContentParts>, Unreadable> {
        let body: Option<String> =
            sqlx::query_scalar("SELECT body FROM shop.note WHERE note_key = $1")
                .bind(key)
                .fetch_optional(&domain.pool)
                .await
                .map_err(|_| Unreadable)?;
        Ok(body.map(|body| ContentParts::one(ContentPart::text(body)).unwrap()))
    }
}

/// The shop's search, which embeds a query with the synthetic embedder in process.
pub static KNOWLEDGE: LazyLock<PgKnowledge<ShopDomain, Synthetic>> =
    LazyLock::new(|| PgKnowledge::new(Synthetic).source(Items).source(Notes));

/// The same sources whose writes embed through the probe server, as a server's writes
/// reach Embedding through the Hub, so a test sees who each Embedding call came from.
pub static WRITES: LazyLock<PgKnowledge<ShopDomain, Probe>> =
    LazyLock::new(|| PgKnowledge::new(Probe).source(Items).source(Notes));

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Address {
    Item(Uuid),
    Note(String),
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
        match parts.authority() {
            "items" => Uuid::parse_str(&path[0])
                .map(Self::Item)
                .map_err(|_| invalid()),
            "notes" => Ok(Self::Note(path[0].to_string())),
            _ => Err(invalid()),
        }
    }
    fn to_uri(&self) -> Result<ResourceUri, Self::Error> {
        let uri = match self {
            Self::Item(id) => format!("shop://items/{id}"),
            Self::Note(key) => format!("shop://notes/{key}"),
        };
        ResourceUri::new(uri.clone()).map_err(|_| Self::Error::new(&uri, "not a shop address"))
    }
}
#[derive(
    serde::Serialize, serde::Deserialize, schemars::JsonSchema, veoveo_hub_sdk::SingleOutput,
)]
pub struct Record {
    pub name: String,
}
#[derive(Clone)]
#[veoveo_hub_sdk::resource_read(provider=ShopProvider,address_type=Address,content=Record,
    address=Address::Item(self.id),template="shop://items/{item_id}",
    parse=|address|match address {Address::Item(id)=>Some(Self{id:*id}),_=>None})]
pub struct ReadItem {
    pub id: Uuid,
}
#[derive(Clone)]
#[veoveo_hub_sdk::resource_read(provider=ShopProvider,address_type=Address,content=Record,
    address=Address::Note(self.key.clone()),template="shop://notes/{note_key}",
    parse=|address|match address {Address::Note(key)=>Some(Self{key:key.clone()}),_=>None})]
pub struct ReadNote {
    pub key: String,
}

pub struct ShopProvider;
impl ProviderContract for ShopProvider {
    fn slug() -> ServerSlug {
        ServerSlug::parse("shop").unwrap()
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, veoveo_types::Vocabulary)]
#[vocabulary(scope)]
pub enum NoScope {}
pub struct ShopContract;
static DOCS: LazyLock<ServerDocs> = LazyLock::new(|| {
    ServerDocs::new(ShopProvider::slug())
        .with_doc("agents", "Agents", "# Synthetic shop")
        .with_doc("design", "Design", "# Synthetic tests only")
});
static SETUP: LazyLock<McpServerSetup<ShopContract>> =
    LazyLock::new(|| McpServerSetup::new().unwrap());
impl McpServerContract for ShopContract {
    type Server = ShopDomain;
    type Provider = ShopProvider;
    type Scope = NoScope;
    fn scheme() -> ResourceScheme {
        ResourceScheme::parse("shop").unwrap()
    }
    fn scopes() -> &'static [NoScope] {
        &[]
    }
    fn documents() -> &'static ServerDocs {
        &DOCS
    }
    fn server_config() -> ServerConfig {
        let mut config = ServerConfig::default();
        config.server_info = Implementation::new("shop", "1");
        config
    }
}

async fn read_record<D: Send + 'static, R: Send + 'static>(
    _: D,
    _: R,
    _: RequestContext<RoleServer>,
) -> Result<Record, ErrorData> {
    Ok(Record {
        name: "synthetic".into(),
    })
}

static ROUTES: LazyLock<Routes<ShopDomain>> = LazyLock::new(|| {
    Routes::builder(RoutePolicy::Authenticated)
        .template::<ReadItem, _, _>(
            ResourceBuilder::new("Item")
                .knowledge(collection("shop.items"))
                .reader(read_record)
                .policy(RoutePolicy::Authenticated)
                .cache(ReadCache::NoStore),
        )
        .template::<ReadNote, _, _>(
            ResourceBuilder::new("Note")
                .knowledge(collection("shop.notes"))
                .reader(read_record)
                .policy(RoutePolicy::Authenticated)
                .cache(ReadCache::NoStore),
        )
        .knowledge(RoutePolicy::Authenticated, KNOWLEDGE.clone())
        .tool::<WriteItem, _, _>(
            "Write an item",
            RoutePolicy::permission(ShopPermission::ItemsWrite).unwrap(),
            |domain: ShopDomain, request, context| async move {
                domain.write_item(request, context).await
            },
        )
        .uses(PermissionName::parse(ProbePermission::EmbeddingsCreate.as_str()).unwrap())
        .starts_tasks()
        .build()
});

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, veoveo_types::Vocabulary)]
pub enum ShopPermission {
    #[vocabulary(rename = "shop.items.write")]
    ItemsWrite,
}
impl PermissionDefinition for ShopPermission {
    fn resource(self) -> &'static str {
        "shop.item"
    }
    fn access(self) -> Access {
        Access::Write
    }
}

/// Writes a public item and has its record embedded.
#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[veoveo_hub_sdk::tool(provider=ShopProvider,output=Written,name="write_item")]
pub struct WriteItem {
    pub name: String,
}

/// A written item: its key, and the Task that embeds it or the outcome of embedding it in
/// the call.
#[derive(
    Debug,
    Clone,
    serde::Serialize,
    serde::Deserialize,
    schemars::JsonSchema,
    veoveo_hub_sdk::SingleOutput,
)]
#[serde(rename_all = "camelCase")]
pub struct Written {
    pub key: String,
    pub task: Option<String>,
    pub records: Vec<IndexedRecord>,
}

impl ShopDomain {
    async fn write_item(
        self,
        request: WriteItem,
        context: RequestContext<RoleServer>,
    ) -> Result<Written, ToolError> {
        let identity = hub_identity(&context)?;
        let key = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO shop.item (item_id, tenant, owner, public, name) VALUES ($1, $2, $3, true, $4)",
        )
        .bind(key)
        .bind(Shop::tenant())
        .bind(Shop::owner(identity.actor.id.as_str()))
        .bind(&request.name)
        .execute(&self.pool)
        .await
        .map_err(|_| ErrorData::internal_error("synthetic write", None))?;
        let indexing = WRITES
            .index_written(&self.runtime, &context, &self, &Items, &[key])
            .await?;
        Ok(match indexing {
            WriteIndexing::Task(task) => Written {
                key: key.to_string(),
                task: Some(task.to_string()),
                records: Vec::new(),
            },
            WriteIndexing::Indexed(outcomes) => Written {
                key: key.to_string(),
                task: None,
                records: outcomes
                    .iter()
                    .map(|(key, outcome)| IndexedRecord::of(key, outcome))
                    .collect(),
            },
        })
    }
}

/// The shop's Tasks, the index Tasks its writes start, for their callers.
#[derive(Clone)]
pub struct ShopTasks {
    runtime: TaskRuntime,
}

impl DurableTaskService for ShopTasks {
    type Caller = HubInternalIdentity;

    fn authenticate(
        &self,
        context: &RequestContext<RoleServer>,
    ) -> Result<Self::Caller, ErrorData> {
        hub_identity(context)
    }

    async fn start_tool_task(
        &self,
        _: &Self::Caller,
        _: CallToolRequestParams,
    ) -> Result<Option<CreateTaskResult>, ErrorData> {
        Ok(None)
    }

    async fn get_task(
        &self,
        caller: &Self::Caller,
        request: GetTaskParams,
    ) -> Result<GetTaskResult, ErrorData> {
        get_durable_task(
            &self.runtime,
            &self.runtime.for_owner(&task_owner(caller)?),
            request,
        )
        .await
    }

    async fn update_task(
        &self,
        caller: &Self::Caller,
        request: UpdateTaskParams,
    ) -> Result<(), ErrorData> {
        update_durable_task(&self.runtime.for_owner(&task_owner(caller)?), request).await
    }

    async fn cancel_task(&self, caller: &Self::Caller, task_id: String) -> Result<(), ErrorData> {
        cancel_durable_task(&self.runtime.for_owner(&task_owner(caller)?), task_id).await
    }

    async fn subscribe_tasks(
        &self,
        caller: &Self::Caller,
        task_ids: Vec<String>,
    ) -> Result<DurableTaskSubscription, ErrorData> {
        subscribe_durable_tasks(
            &self.runtime,
            &self.runtime.for_owner(&task_owner(caller)?),
            task_ids,
        )
        .await
    }
}

/// The shop's Task worker: the one handler registration an index Task needs.
async fn work(worker: TaskWorker, domain: ShopDomain, stop: CancellationToken) {
    let _ = worker
        .run(
            move |ctx| {
                let domain = domain.clone();
                async move {
                    if WRITES.runs(ctx.task()) {
                        return Ok::<_, TaskError>(WRITES.run_index(&domain, &ctx).await);
                    }
                    Ok(TaskTransition::Failed(TaskFailure::new(
                        "unknown_task",
                        "the shop runs no such Task",
                    )))
                }
            },
            stop,
        )
        .await;
}

/// A server sharing one way to declare itself in the mesh.
macro_rules! mesh_server {
    ($slug:literal, $provider:ident, $contract:ident, $domain:ty, $routes:ident) => {
        pub struct $provider;
        impl ProviderContract for $provider {
            fn slug() -> ServerSlug {
                ServerSlug::parse($slug).unwrap()
            }
        }
        pub struct $contract;
        impl McpServerContract for $contract {
            type Server = $domain;
            type Provider = $provider;
            type Scope = NoScope;
            fn scheme() -> ResourceScheme {
                ResourceScheme::parse($slug).unwrap()
            }
            fn scopes() -> &'static [NoScope] {
                &[]
            }
            fn documents() -> &'static ServerDocs {
                static DOCS: LazyLock<ServerDocs> = LazyLock::new(|| {
                    ServerDocs::new($provider::slug())
                        .with_doc("agents", "Agents", "# Synthetic")
                        .with_doc("design", "Design", "# Synthetic tests only")
                });
                &DOCS
            }
            fn server_config() -> ServerConfig {
                let mut config = ServerConfig::default();
                config.server_info = Implementation::new($slug, "1");
                config
            }
        }
        impl DomainServer for $domain {
            type Contract = $contract;
            fn setup() -> &'static McpServerSetup<$contract> {
                static SETUP: LazyLock<McpServerSetup<$contract>> =
                    LazyLock::new(|| McpServerSetup::new().unwrap());
                &SETUP
            }
            fn routes() -> &'static Routes<Self> {
                &$routes
            }
        }
    };
}

/// The probe: a stand-in for Embedding that answers synthetic vectors, refuses text
/// holding `fail-embedding`, and records who each call came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, veoveo_types::Vocabulary)]
pub enum ProbePermission {
    #[vocabulary(rename = "probe.embeddings.create")]
    EmbeddingsCreate,
}
impl PermissionDefinition for ProbePermission {
    fn resource(self) -> &'static str {
        "probe.embedding"
    }
    fn access(self) -> Access {
        Access::Write
    }
}

#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[veoveo_hub_sdk::tool(provider=ProbeProvider,output=ProbeEmbeddings,name="probe_embed")]
pub struct ProbeEmbed {
    pub items: Vec<ContentParts>,
}

#[derive(
    Debug,
    Clone,
    serde::Serialize,
    serde::Deserialize,
    schemars::JsonSchema,
    veoveo_hub_sdk::SingleOutput,
)]
pub struct ProbeEmbeddings {
    pub checkpoint: String,
    pub vectors: Vec<Vec<f32>>,
}

/// Who one Embedding call came from, and the text it embedded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seen {
    pub caller: String,
    pub acting_server: Option<String>,
    pub acting_task: Option<String>,
    pub prior_servers: Vec<String>,
    pub texts: Vec<String>,
}

#[derive(Clone, Default)]
pub struct ProbeDomain {
    seen: Arc<parking_lot::Mutex<Vec<Seen>>>,
}

static PROBE_ROUTES: LazyLock<Routes<ProbeDomain>> = LazyLock::new(|| {
    Routes::builder(RoutePolicy::Authenticated)
        .tool::<ProbeEmbed, _, _>(
            "Synthetic embeddings",
            RoutePolicy::permission(ProbePermission::EmbeddingsCreate).unwrap(),
            |domain: ProbeDomain, request, context| async move { domain.embed(request, context).await },
        )
        .build()
});
mesh_server!(
    "probe",
    ProbeProvider,
    ProbeContract,
    ProbeDomain,
    PROBE_ROUTES
);

impl ProbeDomain {
    async fn embed(
        self,
        request: ProbeEmbed,
        context: RequestContext<RoleServer>,
    ) -> Result<ProbeEmbeddings, ToolError> {
        let identity = hub_identity(&context)?;
        let texts: Vec<String> = request
            .items
            .iter()
            .flatten()
            .filter_map(|part| match part {
                ContentPart::Text { text } => Some(text.clone()),
                _ => None,
            })
            .collect();
        self.seen.lock().push(Seen {
            caller: identity.actor.id.to_string(),
            acting_server: identity.acting_server().map(ToString::to_string),
            acting_task: identity.acting_task().map(ToString::to_string),
            prior_servers: identity
                .delegation_context
                .iter()
                .flat_map(|context| context.actors.iter())
                .map(|actor| actor.server.to_string())
                .collect(),
            texts: texts.clone(),
        });
        if texts.iter().any(|text| text.contains("fail-embedding")) {
            return Err(ToolError::Rejected("synthetic embedding failure".into()));
        }
        Ok(ProbeEmbeddings {
            checkpoint: Synthetic::CHECKPOINT.into(),
            vectors: request.items.iter().map(Synthetic::vector).collect(),
        })
    }
}

/// An embedder that reaches the probe through the Hub as the client's caller.
#[derive(Debug, Clone, Copy, Default)]
pub struct Probe;

#[derive(Debug, thiserror::Error)]
pub enum ProbeFailure {
    #[error(transparent)]
    Call(#[from] CallError),
    #[error("the probe's answer is no embeddings: {0}")]
    Answer(String),
}

impl Embedder for Probe {
    type Error = ProbeFailure;
    async fn embed(
        &self,
        client: &HubClient,
        items: &[ContentParts],
        _purpose: EmbeddingPurpose,
    ) -> Result<Embeddings, ProbeFailure> {
        let answer = client
            .call(ProbeEmbed {
                items: items.to_vec(),
            })
            .await?;
        let checkpoint = ModelCheckpoint::parse(&answer.checkpoint)
            .map_err(|error| ProbeFailure::Answer(error.to_string()))?;
        Embeddings::new(
            EmbeddingModel {
                checkpoint,
                dimensions: EmbeddingDimensions::new(768).unwrap(),
            },
            answer.vectors,
        )
        .map_err(|error| ProbeFailure::Answer(error.to_string()))
    }
}

/// The desk: another server that writes items through the shop for its caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, veoveo_types::Vocabulary)]
pub enum DeskPermission {
    #[vocabulary(rename = "desk.writes.make")]
    WritesMake,
}
impl PermissionDefinition for DeskPermission {
    fn resource(self) -> &'static str {
        "desk.write"
    }
    fn access(self) -> Access {
        Access::Write
    }
}

#[derive(serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
#[veoveo_hub_sdk::tool(provider=DeskProvider,output=Relayed,name="desk_write")]
pub struct DeskWrite {
    pub name: String,
}

/// The shop's answer to the desk: what it wrote, and the related task its result named.
#[derive(
    Debug,
    Clone,
    serde::Serialize,
    serde::Deserialize,
    schemars::JsonSchema,
    veoveo_hub_sdk::SingleOutput,
)]
#[serde(rename_all = "camelCase")]
pub struct Relayed {
    pub written: Written,
    pub related_task: Option<String>,
}

#[derive(Clone, Default)]
pub struct DeskDomain;

static DESK_ROUTES: LazyLock<Routes<DeskDomain>> = LazyLock::new(|| {
    Routes::builder(RoutePolicy::Authenticated)
        .tool::<DeskWrite, _, _>(
            "Write an item through the shop",
            RoutePolicy::permission(DeskPermission::WritesMake).unwrap(),
            |_, request, context| async move { DeskDomain::write(request, context).await },
        )
        .uses(PermissionName::parse(ShopPermission::ItemsWrite.as_str()).unwrap())
        .build()
});
mesh_server!("desk", DeskProvider, DeskContract, DeskDomain, DESK_ROUTES);

impl DeskDomain {
    async fn write(
        request: DeskWrite,
        context: RequestContext<RoleServer>,
    ) -> Result<Relayed, ToolError> {
        let mut arguments = rmcp::model::JsonObject::new();
        arguments.insert("name".into(), json!(request.name));
        let result = context
            .client()?
            .call_tool(HubToolName::parse("shop__write_item").unwrap(), arguments)
            .await?;
        let related_task = result
            .meta
            .as_ref()
            .and_then(|meta| meta.get(veoveo_hub_sdk::RELATED_TASK_META_KEY))
            .and_then(|related| related.get("taskId"))
            .and_then(Value::as_str)
            .map(str::to_owned);
        let written = result
            .structured_content
            .ok_or_else(|| ToolError::Rejected("the shop answered no write".into()))
            .and_then(|written| {
                serde_json::from_value(written)
                    .map_err(|error| ToolError::Rejected(error.to_string()))
            })?;
        Ok(Relayed {
            written,
            related_task,
        })
    }
}

#[derive(Clone)]
pub struct ShopDomain {
    pub pool: PgPool,
    pub runtime: TaskRuntime,
}
impl DomainServer for ShopDomain {
    type Contract = ShopContract;
    fn setup() -> &'static McpServerSetup<ShopContract> {
        &SETUP
    }
    fn routes() -> &'static Routes<Self> {
        &ROUTES
    }
}

/// One test's shop: its database, its server login's pool, its Task worker and a mesh
/// serving it beside the probe and the desk. The worker stops first and the database goes
/// last. Removing the database waits for every backend of the cluster, so the tests run on
/// a multi-threaded runtime whose other threads finish any connection this one opened.
pub struct Shop {
    _worker: DropGuard,
    mesh: RunningMesh,
    domain: ShopDomain,
    probe: ProbeDomain,
    store: veoveo_store::PlatformStore,
    db: TestDb,
}

impl Shop {
    pub async fn start() -> Self {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let db = TestDb::new().await;
        // pgvector in `_extensions`, as the installation's bootstrap creates it.
        db.create_extensions(&["vector"]).await;
        let slug = ShopProvider::slug();
        let store = db.server_store(&slug).await;
        sqlx::raw_sql(SCHEMA)
            .execute(store.pool())
            .await
            .expect("the shop's knowledge schema");
        let instance = Arc::new(Instance::start(&store, &slug).await.unwrap());
        let runtime = TaskRuntime::new(
            store.clone(),
            slug.clone(),
            testing::deployment_id(),
            instance,
        );
        let domain = ShopDomain {
            pool: store.pool().clone(),
            runtime: runtime.clone(),
        };
        let probe = ProbeDomain::default();
        let mut mesh = TestMesh::bind().await;
        let calls = mesh.register_server(slug, ShopDomain::routes().authorization());
        mesh.register_server(ProbeProvider::slug(), ProbeDomain::routes().authorization());
        let desk_calls =
            mesh.register_server(DeskProvider::slug(), DeskDomain::routes().authorization());
        let served = domain.clone();
        let tasks = ShopTasks {
            runtime: runtime.clone(),
        };
        mesh.serve(
            testing::for_domain::<ShopDomain>()
                .calls(calls.clone())
                .handler(move || {
                    Hosted::new(served.clone()).with_tasks(DurableTasks::tasks_only(tasks.clone()))
                })
                .build(),
        );
        let probed = probe.clone();
        mesh.serve(
            testing::for_domain::<ProbeDomain>()
                .handler(move || Hosted::new(probed.clone()))
                .build(),
        );
        mesh.serve(
            testing::for_domain::<DeskDomain>()
                .calls(desk_calls)
                .handler(|| Hosted::new(DeskDomain))
                .build(),
        );
        let mesh = mesh
            .granting([
                PermissionName::parse(ShopPermission::ItemsWrite.as_str()).unwrap(),
                PermissionName::parse(ProbePermission::EmbeddingsCreate.as_str()).unwrap(),
                PermissionName::parse(DeskPermission::WritesMake.as_str()).unwrap(),
            ])
            .start();
        let stop = CancellationToken::new();
        tokio::spawn(work(
            TaskWorker::new(runtime).calls(calls),
            domain.clone(),
            stop.clone(),
        ));
        Self {
            _worker: stop.drop_guard(),
            mesh,
            domain,
            probe,
            store,
            db,
        }
    }

    pub fn pool(&self) -> &PgPool {
        &self.domain.pool
    }

    pub fn knowledge(&self) -> &'static PgKnowledge<ShopDomain, Synthetic> {
        &KNOWLEDGE
    }

    pub async fn execute(&self, statement: &'static str) {
        sqlx::raw_sql(statement)
            .execute(self.pool())
            .await
            .unwrap_or_else(|error| panic!("{statement}: {error}"));
    }

    fn tenant() -> Uuid {
        let tenant = veoveo_store::deterministic_tenant_id("tenant-a").unwrap();
        serde_json::from_value(serde_json::to_value(tenant).unwrap()).unwrap()
    }

    fn owner(principal: &str) -> Uuid {
        let owner = veoveo_store::deterministic_principal_id("tenant-a", principal).unwrap();
        serde_json::from_value(serde_json::to_value(owner).unwrap()).unwrap()
    }

    /// A record readable by the test user when `readable`: public, or else owned by
    /// someone else.
    pub async fn item(&self, name: &str, readable: bool) -> Uuid {
        let key = Uuid::now_v7();
        let owner = Self::owner("someone-else");
        sqlx::query("INSERT INTO shop.item (item_id, tenant, owner, public, name) VALUES ($1, $2, $3, $4, $5)")
            .bind(key)
            .bind(Self::tenant())
            .bind(owner)
            .bind(readable)
            .bind(name)
            .execute(self.pool())
            .await
            .unwrap();
        key
    }

    pub async fn note(&self, key: &str, body: &str, readable: bool) {
        sqlx::query(
            "INSERT INTO shop.note (note_key, tenant, public, body) VALUES ($1, $2, $3, $4)",
        )
        .bind(key)
        .bind(Self::tenant())
        .bind(readable)
        .bind(body)
        .execute(self.pool())
        .await
        .unwrap();
    }

    /// A client nothing calls: the synthetic embedder never reaches Embedding.
    fn client() -> HubClient {
        HubClient::new(
            url::Url::parse("http://127.0.0.1:9/mcp/local").unwrap(),
            reqwest::Client::new(),
            StaticBearer::new("synthetic-bearer".to_owned()),
        )
    }

    pub async fn index_items(&self, keys: &[Uuid]) -> Vec<(Uuid, Indexed)> {
        KNOWLEDGE
            .index(&self.domain, &Self::client(), &Items, keys)
            .await
            .unwrap()
    }

    /// Indexes `keys` of the items with `embedder` instead of the synthetic one.
    pub async fn index_with<E: Embedder + 'static>(
        &self,
        embedder: E,
        keys: &[Uuid],
    ) -> Vec<(Uuid, Indexed)> {
        PgKnowledge::new(embedder)
            .source(Items)
            .index(&self.domain, &Self::client(), &Items, keys)
            .await
            .unwrap()
    }

    /// An item's stored vector.
    pub async fn stored_vector(&self, key: Uuid) -> Option<Vec<f32>> {
        let text: Option<String> =
            sqlx::query_scalar("SELECT knowledge_vector::text FROM shop.item WHERE item_id = $1")
                .bind(key)
                .fetch_one(self.pool())
                .await
                .unwrap();
        text.map(|text| {
            text.trim_matches(['[', ']'])
                .split(',')
                .map(|value| value.parse().unwrap())
                .collect()
        })
    }

    pub async fn index_unknown(
        &self,
        unknown: &KnowledgeCollection,
    ) -> Result<Vec<(String, Indexed)>, PgKnowledgeError> {
        struct Unknown(KnowledgeCollection);
        impl PgKnowledgeSource<ShopDomain> for Unknown {
            type Key = String;
            type Error = Unreadable;
            fn collection(&self) -> KnowledgeCollection {
                self.0.clone()
            }
            fn table(&self) -> PgKnowledgeTable {
                PgKnowledgeTable::new("order", "order_key", "order_readable").unwrap()
            }
            fn pool<'a>(&self, domain: &'a ShopDomain) -> &'a PgPool {
                &domain.pool
            }
            async fn hits(
                &self,
                _: &ShopDomain,
                _: &RequestContext<RoleServer>,
                _: &[String],
            ) -> Result<Vec<Option<KnowledgeHit>>, ToolError> {
                Ok(vec![])
            }
            async fn content(
                &self,
                _: &ShopDomain,
                _: &HubClient,
                _: String,
            ) -> Result<Option<ContentParts>, Unreadable> {
                Ok(None)
            }
        }
        KNOWLEDGE
            .index(
                &self.domain,
                &Self::client(),
                &Unknown(unknown.clone()),
                &["a".into()],
            )
            .await
    }

    /// Embeds every item and note.
    pub async fn index_all(&self) {
        let items: Vec<Uuid> = sqlx::query_scalar("SELECT item_id FROM shop.item")
            .fetch_all(self.pool())
            .await
            .unwrap();
        for (_, outcome) in self.index_items(&items).await {
            assert_eq!(outcome, Indexed::Embedded);
        }
        let notes: Vec<String> = sqlx::query_scalar("SELECT note_key FROM shop.note")
            .fetch_all(self.pool())
            .await
            .unwrap();
        for (_, outcome) in KNOWLEDGE
            .index(&self.domain, &Self::client(), &Notes, &notes)
            .await
            .unwrap()
        {
            assert_eq!(outcome, Indexed::Embedded);
        }
    }

    /// Sets an item's vector and model directly, as another model's write would.
    pub async fn vector(&self, key: Uuid, vector: &[f32], model: &str) {
        let text = format!(
            "[{}]",
            vector
                .iter()
                .map(f32::to_string)
                .collect::<Vec<_>>()
                .join(",")
        );
        sqlx::query("UPDATE shop.item SET knowledge_vector = $1::_extensions.vector, knowledge_model = $2 WHERE item_id = $3")
            .bind(text)
            .bind(model)
            .bind(key)
            .execute(self.pool())
            .await
            .unwrap();
    }

    /// An item's stored model and vector dimensions.
    pub async fn stored(&self, key: Uuid) -> (Option<String>, Option<i32>) {
        sqlx::query_as(
            "SELECT knowledge_model, _extensions.vector_dims(knowledge_vector) FROM shop.item WHERE item_id = $1",
        )
        .bind(key)
        .fetch_one(self.pool())
        .await
        .unwrap()
    }

    pub async fn search(&self, arguments: Value) -> Value {
        self.mesh.call("shop", "search_knowledge", arguments).await
    }

    /// Writes an item as the test user, calling the shop directly.
    pub async fn write(&self, name: &str) -> Value {
        self.mesh
            .call("shop", "write_item", json!({ "name": name }))
            .await
    }

    /// Writes an item as the test user through the desk, which calls the shop.
    pub async fn write_through_desk(&self, name: &str) -> Value {
        self.mesh
            .call("desk", "desk_write", json!({ "name": name }))
            .await
    }

    /// The test user's view of the shop's Task `task` once it settles.
    pub async fn settled(&self, task: &str) -> Value {
        tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                let answer = self.mesh.task("shop", task).await;
                if answer["result"]["status"] != "working" {
                    return answer;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .expect("the Task settles within 30 seconds")
    }

    /// The Embedding calls that embedded text holding `marker`.
    pub fn seen(&self, marker: &str) -> Vec<Seen> {
        self.probe
            .seen
            .lock()
            .iter()
            .filter(|seen| seen.texts.iter().any(|text| text.contains(marker)))
            .cloned()
            .collect()
    }
}
