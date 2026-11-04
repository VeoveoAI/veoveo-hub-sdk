//! The ready-made PostgreSQL knowledge search: [`PgKnowledge`] over the
//! [`PgKnowledgeSource`]s of one server, each a table of the server's own schema with the
//! knowledge columns, their indexes and the server's read rule.
//!
//! The statements name only identifiers the sources declared, validated and quoted; every
//! value is a bind parameter. Vectors cross the driver in pgvector's text form.
use std::{collections::BTreeMap, fmt, future::Future, sync::Arc};

use chrono::Utc;
use futures::future::BoxFuture;
use rmcp::{ErrorData, RoleServer, model::CallToolResult, service::RequestContext};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sqlx::{AssertSqlSafe, PgPool};
use uuid::Uuid;
use veoveo_task::{RecoveryClass, TaskFailure, TaskRuntime, TaskSnapshot, TaskTransition};
use veoveo_types::{TaskId, TaskTypeName};

use super::{
    ContentPart, ContentParts, Embedder, EmbeddingModel, EmbeddingPurpose, KnowledgeCaller,
    KnowledgeCollection, KnowledgeHit, KnowledgeQuery, KnowledgeSearch, SearchContext, fuse,
};
use crate::{
    client::HubClient,
    durable_tasks::{CallTask, TaskContext, mcp_task_completion, start_call_task},
    hosting::{ClientContext, DomainServer, ToolError, hub_identity},
};

/// The dimensions of `knowledge_vector`, `_extensions.vector(768)`.
pub const VECTOR_DIMENSIONS: u32 = 768;
/// The most records one Embedding call of [`PgKnowledge::index`] embeds.
pub const INDEX_BATCH: usize = 8;
/// How many times [`PgKnowledge::index`] reads, embeds and writes a record that changes
/// while it is embedded, before it reports [`NotEmbedded::Changed`].
pub const INDEX_ATTEMPTS: usize = 3;
/// How deep each ranking of a search reaches: this many times the query's limit.
pub const RANK_DEPTH: u32 = 4;

/// The type of the durable Task in which [`PgKnowledge::index_written`] embeds a direct
/// write's records for its caller.
pub const INDEX_TASK: TaskTypeName = TaskTypeName::from_static("knowledge.index");
/// How long an index Task's route lives, an hour.
pub const INDEX_TASK_TTL_MS: u64 = 60 * 60 * 1_000;

/// The three columns a knowledge table carries.
pub const TEXT_COLUMN: &str = "knowledge_text";
pub const VECTOR_COLUMN: &str = "knowledge_vector";
pub const MODEL_COLUMN: &str = "knowledge_model";

/// A PostgreSQL identifier a knowledge source declares: 1 to 63 lowercase ASCII letters,
/// digits and underscores, starting with a letter or an underscore. The statements quote
/// it, so it names exactly what it spells.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PgIdentifier(String);

impl PgIdentifier {
    pub fn parse(value: &str) -> Result<Self, PgKnowledgeError> {
        let mut bytes = value.bytes();
        let admitted = value.len() <= 63
            && bytes
                .next()
                .is_some_and(|first| first.is_ascii_lowercase() || first == b'_')
            && bytes.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
        if !admitted {
            return Err(PgKnowledgeError::Identifier(value.to_owned()));
        }
        Ok(Self(value.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn quoted(&self) -> String {
        format!("\"{}\"", self.0)
    }
}

impl fmt::Display for PgIdentifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// The type of a knowledge table's key column: `uuid`, as [`Uuid`], or `text`, as
/// [`String`]. Keys cross the driver in their text form and the statements cast them to
/// the column's type.
pub trait PgKnowledgeKey: sealed::Sealed + Clone + fmt::Debug + Send + Sync + 'static {
    /// The column's PostgreSQL type.
    const COLUMN_TYPE: &'static str;
    fn to_text(&self) -> String;
    fn from_text(text: &str) -> Option<Self>;
}

mod sealed {
    pub trait Sealed {}
    impl Sealed for uuid::Uuid {}
    impl Sealed for String {}
}

impl PgKnowledgeKey for Uuid {
    const COLUMN_TYPE: &'static str = "uuid";
    fn to_text(&self) -> String {
        self.to_string()
    }
    fn from_text(text: &str) -> Option<Self> {
        Uuid::parse_str(text).ok()
    }
}

impl PgKnowledgeKey for String {
    const COLUMN_TYPE: &'static str = "text";
    fn to_text(&self) -> String {
        self.clone()
    }
    fn from_text(text: &str) -> Option<Self> {
        Some(text.to_owned())
    }
}

/// Where a source's records are, in the server's own schema: the table, its key column,
/// of the source's [`PgKnowledgeKey`] type, and the read rule, a function
/// `(r table, caller jsonb) RETURNS boolean`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PgKnowledgeTable {
    pub table: PgIdentifier,
    pub key: PgIdentifier,
    pub read_rule: PgIdentifier,
}

impl PgKnowledgeTable {
    pub fn new(table: &str, key: &str, read_rule: &str) -> Result<Self, PgKnowledgeError> {
        Ok(Self {
            table: PgIdentifier::parse(table)?,
            key: PgIdentifier::parse(key)?,
            read_rule: PgIdentifier::parse(read_rule)?,
        })
    }
}

/// One collection a server keeps in a PostgreSQL table. The server's own code loads its
/// hits and gives the content to embed for a record; the SDK ranks it.
pub trait PgKnowledgeSource<D: DomainServer>: Send + Sync + 'static {
    /// The type of the key column: [`Uuid`] for `uuid`, [`String`] for `text`.
    type Key: PgKnowledgeKey;
    /// Why a record's content could not be read. Its text reaches the record's
    /// [`NotEmbedded::Content`], so it never holds a signed URL.
    type Error: std::error::Error + Send + Sync + 'static;

    /// The collection, which a template of the server marks.
    fn collection(&self) -> KnowledgeCollection;

    /// The collection's table, key column and read rule.
    fn table(&self) -> PgKnowledgeTable;

    /// The pool of the server's database login.
    fn pool<'a>(&self, domain: &'a D) -> &'a PgPool;

    /// The hits of `keys`, ranked, for the request's caller: one entry per key in their
    /// order, `None` for a record the server no longer finds or shows.
    fn hits(
        &self,
        domain: &D,
        request: &RequestContext<RoleServer>,
        keys: &[Self::Key],
    ) -> impl Future<Output = Result<Vec<Option<KnowledgeHit>>, ToolError>> + Send;

    /// What to embed for the record `key`: its text and its media as URLs signed for the
    /// caller of `client`, or `None` when it has nothing to embed or no longer exists.
    fn content(
        &self,
        domain: &D,
        client: &HubClient,
        key: Self::Key,
    ) -> impl Future<Output = Result<Option<ContentParts>, Self::Error>> + Send;
}

/// What [`PgKnowledge::index`] did with one record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Indexed {
    /// Its vector and model were written.
    Embedded,
    /// It keeps its previous vector, if any, and is found by words until it is embedded.
    NotEmbedded(NotEmbedded),
}

/// Why a record was not embedded.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NotEmbedded {
    /// The record has nothing to embed or no longer exists.
    #[error("the record has nothing to embed")]
    NoContent,
    /// Its content could not be read: the source's reason.
    #[error("the record's content could not be read: {0}")]
    Content(String),
    /// The Embedding call failed; every record of that call reports it.
    #[error("the record could not be embedded: {0}")]
    Embedding(String),
    /// The record was gone when its vector was written.
    #[error("the record was gone when its vector was written")]
    Gone,
    /// The record changed while each of [`INDEX_ATTEMPTS`] attempts embedded it.
    #[error("the record kept changing while it was embedded")]
    Changed,
    /// The database refused the write.
    #[error("the record's vector could not be written")]
    Write,
}

/// Why a knowledge source cannot serve, or a request of it failed.
#[derive(Debug, thiserror::Error)]
pub enum PgKnowledgeError {
    #[error(
        "`{0}` is not an identifier a knowledge source names: 1 to 63 lowercase ASCII letters, digits and underscores, starting with a letter or an underscore"
    )]
    Identifier(String),
    #[error("{0} is no collection of this knowledge search")]
    UnknownCollection(KnowledgeCollection),
    #[error("{collection}: {schema}.{table} is no table")]
    Table {
        collection: KnowledgeCollection,
        schema: String,
        table: PgIdentifier,
    },
    #[error("{collection}: the column {column} is not {expected}")]
    Column {
        collection: KnowledgeCollection,
        column: String,
        expected: &'static str,
    },
    #[error("{collection}: {column} has no {expected} index")]
    Index {
        collection: KnowledgeCollection,
        column: &'static str,
        expected: &'static str,
    },
    #[error(
        "{collection}: the read rule {schema}.{function}(r {schema}.{table}, caller jsonb) RETURNS boolean is missing or not STABLE"
    )]
    ReadRule {
        collection: KnowledgeCollection,
        schema: String,
        table: PgIdentifier,
        function: PgIdentifier,
    },
    #[error("the database refused a knowledge statement")]
    Database(#[source] sqlx::Error),
}

/// The query's embedding: the one vector of its parts, made by `model`.
#[derive(Debug, Clone, PartialEq)]
pub struct QueryEmbedding {
    pub model: EmbeddingModel,
    pub vector: Vec<f32>,
}

/// Embeds `query` once as the caller of `client`, with purpose `Query`: all its parts
/// make one vector.
pub async fn embed_query<E: Embedder>(
    embedder: &E,
    client: &HubClient,
    query: &KnowledgeQuery,
) -> Result<QueryEmbedding, QueryEmbeddingError<E::Error>> {
    let embeddings = embedder
        .embed(
            client,
            std::slice::from_ref(&query.parts),
            EmbeddingPurpose::Query,
        )
        .await
        .map_err(QueryEmbeddingError::Embedder)?;
    let model = embeddings.model().clone();
    let mut vectors = embeddings.into_vectors();
    if vectors.len() != 1 {
        return Err(QueryEmbeddingError::Cardinality(vectors.len()));
    }
    Ok(QueryEmbedding {
        model,
        vector: vectors.remove(0),
    })
}

/// Why a query could not be embedded.
#[derive(Debug, thiserror::Error)]
pub enum QueryEmbeddingError<E: std::error::Error + 'static> {
    #[error(transparent)]
    Embedder(E),
    #[error("the embedder answered {0} vectors for one query")]
    Cardinality(usize),
}

/// The ready-made knowledge search over a server's PostgreSQL sources, and the vector
/// writes of their records. Clones share their sources.
pub struct PgKnowledge<D: DomainServer, E: Embedder> {
    embedder: Arc<E>,
    sources: Arc<Vec<Registered<D>>>,
}

impl<D: DomainServer, E: Embedder> Clone for PgKnowledge<D, E> {
    fn clone(&self) -> Self {
        Self {
            embedder: self.embedder.clone(),
            sources: self.sources.clone(),
        }
    }
}

struct Registered<D> {
    collection: KnowledgeCollection,
    schema: PgIdentifier,
    table: PgKnowledgeTable,
    key_type: &'static str,
    // Built only from the declared, validated and quoted identifiers and the sealed key
    // type; every value is a bind parameter.
    rank: Arc<str>,
    version: Arc<str>,
    write: Arc<str>,
    source: Arc<dyn DynSource<D>>,
}

impl<D: DomainServer, E: Embedder + 'static> PgKnowledge<D, E> {
    pub fn new(embedder: E) -> Self {
        Self {
            embedder: Arc::new(embedder),
            sources: Arc::new(Vec::new()),
        }
    }

    /// Adds `source`. Its collection is the server's own and its table lies in the
    /// server's schema; building fails for a collection added twice or of another server.
    pub fn source<S: PgKnowledgeSource<D>>(mut self, source: S) -> Self {
        let collection = source.collection();
        let slug = crate::server_contract::provider_slug::<D::Contract>();
        assert!(
            collection.is_owned_by(&slug),
            "a knowledge source's collection is the server's own"
        );
        assert!(
            self.sources
                .iter()
                .all(|registered| registered.collection != collection),
            "a knowledge collection has one source"
        );
        let schema = veoveo_store::ServerSchema::new(&slug)
            .ok()
            .and_then(|schema| PgIdentifier::parse(schema.schema()).ok())
            .expect("the server's schema is an identifier");
        let table = source.table();
        let key_type = <S::Key as PgKnowledgeKey>::COLUMN_TYPE;
        let registered = Registered {
            rank: rank_statement(&schema, &table).into(),
            version: version_statement(&schema, &table, key_type).into(),
            write: write_statement(&schema, &table, key_type).into(),
            collection,
            schema,
            table,
            key_type,
            source: Arc::new(source),
        };
        Arc::get_mut(&mut self.sources)
            .expect("sources are added before the search is shared")
            .push(registered);
        self
    }

    /// The collections of the sources.
    pub fn collections(&self) -> impl Iterator<Item = &KnowledgeCollection> {
        self.sources.iter().map(|registered| &registered.collection)
    }

    /// Checks, before the server serves, that each source's table has its key column of
    /// the type the source declares, `uuid` or `text`, the three knowledge columns with
    /// their types, a GIN index on `knowledge_text`, an HNSW
    /// index with `_extensions.vector_cosine_ops` on `knowledge_vector`, and its read rule.
    pub async fn check(&self, pool: &PgPool) -> Result<(), PgKnowledgeError> {
        for registered in self.sources.iter() {
            registered.check(pool).await?;
        }
        Ok(())
    }

    /// Embeds the records `keys` of `source` as the caller of `client`, at most 8 per
    /// Embedding call with purpose `Document`, and writes each one's vector and model on
    /// the source's pool. Returns what happened to each key, in their order; a record that
    /// fails reports why and keeps the others from failing, except that the records of one
    /// failing Embedding call all report it.
    ///
    /// A vector is written only over the row version, `xmin`, read before its content:
    /// a record written again meanwhile is read, embedded and written again, up to
    /// [`INDEX_ATTEMPTS`] times in all, so an older content's vector never replaces a
    /// newer one's.
    pub async fn index<S: PgKnowledgeSource<D>>(
        &self,
        domain: &D,
        client: &HubClient,
        source: &S,
        keys: &[S::Key],
    ) -> Result<Vec<(S::Key, Indexed)>, PgKnowledgeError> {
        let texts: Vec<String> = keys.iter().map(PgKnowledgeKey::to_text).collect();
        let outcomes = self
            .index_text(domain, client, &source.collection(), &texts)
            .await?;
        Ok(keys.iter().cloned().zip(outcomes).collect())
    }

    /// [`Self::index`] of the source of `collection`, with its keys in their text form.
    pub(crate) async fn index_text(
        &self,
        domain: &D,
        client: &HubClient,
        collection: &KnowledgeCollection,
        keys: &[String],
    ) -> Result<Vec<Indexed>, PgKnowledgeError> {
        let registered = self
            .sources
            .iter()
            .find(|registered| &registered.collection == collection)
            .ok_or_else(|| PgKnowledgeError::UnknownCollection(collection.clone()))?;
        let pool = registered.source.pool(domain);
        let mut outcomes: BTreeMap<usize, Indexed> = BTreeMap::new();
        let mut pending: Vec<usize> = (0..keys.len()).collect();
        for attempt in 1..=INDEX_ATTEMPTS {
            // Each record's row version, then its content as of that version or later.
            let mut items = Vec::new();
            for index in pending.drain(..) {
                let key = &keys[index];
                let version = match registered.version(pool, key).await {
                    Ok(Some(version)) => version,
                    Ok(None) if attempt == 1 => {
                        outcomes.insert(index, Indexed::NotEmbedded(NotEmbedded::NoContent));
                        continue;
                    }
                    Ok(None) => {
                        outcomes.insert(index, Indexed::NotEmbedded(NotEmbedded::Gone));
                        continue;
                    }
                    Err(_) => {
                        outcomes.insert(
                            index,
                            Indexed::NotEmbedded(NotEmbedded::Content(
                                "its row version could not be read".into(),
                            )),
                        );
                        continue;
                    }
                };
                match registered.source.content(domain, client, key).await {
                    Ok(Some(content)) => items.push((index, version, content)),
                    Ok(None) => {
                        outcomes.insert(index, Indexed::NotEmbedded(NotEmbedded::NoContent));
                    }
                    Err(reason) => {
                        outcomes.insert(index, Indexed::NotEmbedded(NotEmbedded::Content(reason)));
                    }
                }
            }
            for batch in items.chunks(INDEX_BATCH) {
                let contents: Vec<ContentParts> = batch
                    .iter()
                    .map(|(_, _, content)| content.clone())
                    .collect();
                let embedded = self
                    .embedder
                    .embed(client, &contents, EmbeddingPurpose::Document)
                    .await
                    .map_err(|error| error.to_string())
                    .and_then(|embeddings| written(embeddings, contents.len()));
                let (model, vectors) = match embedded {
                    Ok(written) => written,
                    Err(reason) => {
                        for (index, _, _) in batch {
                            outcomes.insert(
                                *index,
                                Indexed::NotEmbedded(NotEmbedded::Embedding(reason.clone())),
                            );
                        }
                        continue;
                    }
                };
                for ((index, version, _), vector) in batch.iter().zip(vectors) {
                    let key = &keys[*index];
                    let outcome = sqlx::query(AssertSqlSafe(registered.write.clone()))
                        .bind(vector_text(&vector))
                        .bind(&model)
                        .bind(key)
                        .bind(version)
                        .execute(pool)
                        .await;
                    let outcome = match outcome {
                        Ok(done) if done.rows_affected() == 1 => Indexed::Embedded,
                        Ok(_) => match registered.version(pool, key).await {
                            Ok(None) => Indexed::NotEmbedded(NotEmbedded::Gone),
                            // Written again since its version was read: read it again.
                            Ok(Some(_)) if attempt < INDEX_ATTEMPTS => {
                                pending.push(*index);
                                continue;
                            }
                            Ok(Some(_)) => Indexed::NotEmbedded(NotEmbedded::Changed),
                            Err(_) => Indexed::NotEmbedded(NotEmbedded::Write),
                        },
                        Err(_) => Indexed::NotEmbedded(NotEmbedded::Write),
                    };
                    outcomes.insert(*index, outcome);
                }
            }
            if pending.is_empty() {
                break;
            }
        }
        Ok((0..keys.len())
            .map(|index| outcomes.remove(&index).expect("every key has an outcome"))
            .collect())
    }
}

/// How a write's records reach their vectors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WriteIndexing<K> {
    /// The caller wrote directly: a durable Task embeds the records as the caller, and
    /// the write's result names it as its related task.
    Task(TaskId),
    /// Another server wrote for the caller: the records were embedded in the call,
    /// continuing its chain of servers, with each record's outcome.
    Indexed(Vec<(K, Indexed)>),
}

/// What an index Task embeds: a collection and its keys in their text form.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct IndexTaskRequest {
    collection: KnowledgeCollection,
    keys: Vec<String>,
}

/// One record's outcome in an index Task's result or failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct IndexedRecord {
    pub key: String,
    pub embedded: bool,
    /// Why it was not embedded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl IndexedRecord {
    /// The record `key` with its outcome, as a write's result or an index Task reports it.
    pub fn of<K: PgKnowledgeKey>(key: &K, outcome: &Indexed) -> Self {
        Self::reported(key.to_text(), outcome)
    }

    fn reported(key: String, outcome: &Indexed) -> Self {
        match outcome {
            Indexed::Embedded => Self {
                key,
                embedded: true,
                reason: None,
            },
            Indexed::NotEmbedded(reason) => Self {
                key,
                embedded: false,
                reason: Some(reason.to_string()),
            },
        }
    }
}

impl<D: DomainServer, E: Embedder + 'static> PgKnowledge<D, E> {
    /// What a write handler calls after its write commits, for the records `keys` of
    /// `source` it wrote; the server makes this one call and chooses nothing. When the
    /// caller called directly, it starts an index Task in `runtime` for the caller with
    /// [`start_call_task`], which the write's result names. When another server called for
    /// the caller, it embeds the records in the call, continuing the chain of servers,
    /// since a Task another server's call created cannot act for the caller.
    pub async fn index_written<S: PgKnowledgeSource<D>>(
        &self,
        runtime: &TaskRuntime,
        context: &RequestContext<RoleServer>,
        domain: &D,
        source: &S,
        keys: &[S::Key],
    ) -> Result<WriteIndexing<S::Key>, ErrorData> {
        let collection = source.collection();
        if !self.collections().any(|known| *known == collection) {
            return Err(ErrorData::internal_error(
                PgKnowledgeError::UnknownCollection(collection).to_string(),
                None,
            ));
        }
        let identity = hub_identity(context)?;
        if identity.acting_server().is_some() {
            let client = context.client()?;
            let outcomes = self
                .index(domain, &client, source, keys)
                .await
                .map_err(|error| ErrorData::internal_error(error.to_string(), None))?;
            return Ok(WriteIndexing::Indexed(outcomes));
        }
        let request = serde_json::to_value(IndexTaskRequest {
            collection,
            keys: keys.iter().map(PgKnowledgeKey::to_text).collect(),
        })
        .map_err(|_| ErrorData::internal_error("the index Task's request", None))?;
        let task = start_call_task(
            runtime,
            context,
            CallTask {
                task_type: INDEX_TASK,
                request,
                recovery_class: RecoveryClass::Resume,
                ttl_ms: Some(INDEX_TASK_TTL_MS),
                poll_interval_ms: Some(1_000),
            },
        )
        .await?;
        Ok(WriteIndexing::Task(task.task_id))
    }

    /// Whether `task` is an index Task, which [`Self::run_index`] runs.
    pub fn runs(&self, task: &TaskSnapshot) -> bool {
        task.task_type == INDEX_TASK
    }

    /// Runs an index Task as its caller, through `ctx.client()`: the one line a server's
    /// [`TaskWorker`](crate::durable_tasks::TaskWorker) handler gives an index Task. It
    /// succeeds with each record's outcome; when a record could not be embedded or
    /// written, it fails, naming those records and why, so the person sees it among
    /// their Tasks. A record with nothing to embed, or gone, is no failure.
    pub async fn run_index(&self, domain: &D, ctx: &TaskContext) -> TaskTransition {
        let failed = |message: String, details: Option<serde_json::Value>| {
            let mut failure = TaskFailure::new("knowledge_index", message);
            failure.details = details;
            TaskTransition::Failed(failure)
        };
        let Ok(request) = serde_json::from_value::<IndexTaskRequest>(ctx.task().request.clone())
        else {
            return failed("the index Task's request is not one".into(), None);
        };
        let client = match ctx.client() {
            Ok(client) => client,
            Err(error) => {
                return failed(format!("the Task cannot act for its caller: {error}"), None);
            }
        };
        let outcomes = match self
            .index_text(domain, &client, &request.collection, &request.keys)
            .await
        {
            Ok(outcomes) => outcomes,
            Err(error) => return failed(error.to_string(), None),
        };
        let failures: Vec<&NotEmbedded> = outcomes
            .iter()
            .filter_map(|outcome| match outcome {
                Indexed::NotEmbedded(NotEmbedded::NoContent | NotEmbedded::Gone)
                | Indexed::Embedded => None,
                Indexed::NotEmbedded(reason) => Some(reason),
            })
            .collect();
        let message = failures.first().map(|first| {
            format!(
                "{} of {} records could not be embedded: {first}",
                failures.len(),
                outcomes.len()
            )
        });
        let records: Vec<IndexedRecord> = request
            .keys
            .into_iter()
            .zip(&outcomes)
            .map(|(key, outcome)| IndexedRecord::reported(key, outcome))
            .collect();
        if let Some(message) = message {
            return failed(message, Some(serde_json::json!({ "records": records })));
        }
        let result = CallToolResult::structured(serde_json::json!({ "records": records }));
        mcp_task_completion(format!("{} records embedded", records.len()), result)
            .unwrap_or_else(|error| failed(error.to_string(), None))
    }
}

/// The model text and the vectors of `embeddings`, which answer `count` records.
fn written(embeddings: super::Embeddings, count: usize) -> Result<(String, Vec<Vec<f32>>), String> {
    let model = embeddings.model();
    if model.dimensions.get() != VECTOR_DIMENSIONS {
        return Err(format!(
            "the model's vectors have {} dimensions; knowledge_vector holds {VECTOR_DIMENSIONS}",
            model.dimensions.get()
        ));
    }
    let model = model.checkpoint.as_str().to_owned();
    let vectors = embeddings.into_vectors();
    if vectors.len() != count {
        return Err(format!(
            "the embedder answered {} vectors for {count} records",
            vectors.len()
        ));
    }
    Ok((model, vectors))
}

/// A vector in pgvector's text form, `[x,y,…]`.
fn vector_text(vector: &[f32]) -> String {
    let mut text = String::with_capacity(vector.len() * 12 + 2);
    text.push('[');
    for (index, value) in vector.iter().enumerate() {
        if index > 0 {
            text.push(',');
        }
        text.push_str(&value.to_string());
    }
    text.push(']');
    text
}

/// The text of a query: its text parts in order, joined by spaces, when it holds any.
fn query_text(parts: &ContentParts) -> Option<String> {
    let texts: Vec<&str> = parts
        .iter()
        .filter_map(|part| match part {
            ContentPart::Text { text } if !text.trim().is_empty() => Some(text.as_str()),
            _ => None,
        })
        .collect();
    (!texts.is_empty()).then(|| texts.join(" "))
}

/// One source's statement: the full-text ranking when the query holds text, `$1`, and the
/// cosine ranking over vectors of the query's model, `$4`, each over the rows the read
/// rule admits for the caller, `$2`, and each to the depth `$5`. `$3` is the query's
/// vector. Each row is one rank of one list: 0 for text, 1 for vectors.
fn rank_statement(schema: &PgIdentifier, table: &PgKnowledgeTable) -> String {
    let relation = format!("{}.{}", schema.quoted(), table.table.quoted());
    let key = table.key.quoted();
    let rule = format!("{}.{}", schema.quoted(), table.read_rule.quoted());
    format!(
        "WITH text_ranked AS MATERIALIZED (
    SELECT r.{key}::text AS key,
        pg_catalog.ts_rank_cd(r.knowledge_text, pg_catalog.websearch_to_tsquery('simple', $1)) AS score
    FROM {relation} AS r
    WHERE $1::text IS NOT NULL
        AND r.knowledge_text @@ pg_catalog.websearch_to_tsquery('simple', $1)
        AND {rule}(r, $2)
    ORDER BY score DESC, key
    LIMIT $5
), vector_ranked AS MATERIALIZED (
    SELECT r.{key}::text AS key,
        r.knowledge_vector OPERATOR(_extensions.<=>) $3::_extensions.vector AS distance
    FROM {relation} AS r
    WHERE r.knowledge_model = $4
        AND {rule}(r, $2)
    ORDER BY distance
    LIMIT $5
)
SELECT 0::int2 AS list, key, row_number() OVER (ORDER BY score DESC, key) AS rank
FROM text_ranked
UNION ALL
SELECT 1::int2 AS list, key, row_number() OVER (ORDER BY distance, key) AS rank
FROM vector_ranked"
    )
}

/// Whether a column's type, the type's schema, its modifier and its generation fit.
type ColumnTest = fn(&str, &str, i32, &str) -> bool;

/// One record's row version, `xmin`: `$1` the key's text, cast to the key column's
/// sealed type, `uuid` or `text`.
fn version_statement(schema: &PgIdentifier, table: &PgKnowledgeTable, key_type: &str) -> String {
    format!(
        "SELECT r.xmin::text FROM {}.{} AS r WHERE r.{} = $1::{key_type}",
        schema.quoted(),
        table.table.quoted(),
        table.key.quoted()
    )
}

/// One record's vector write over the row version its content was read at: `$1` the
/// vector, `$2` the model, `$3` the key's text, cast to the key column's sealed type, and
/// `$4` the version.
fn write_statement(schema: &PgIdentifier, table: &PgKnowledgeTable, key_type: &str) -> String {
    format!(
        "UPDATE {}.{} SET knowledge_vector = $1::_extensions.vector, knowledge_model = $2 WHERE {} = $3::{key_type} AND xmin::text = $4",
        schema.quoted(),
        table.table.quoted(),
        table.key.quoted()
    )
}

impl<D: DomainServer> Registered<D> {
    /// The row version of the record `key`, absent when no row holds it.
    async fn version(&self, pool: &PgPool, key: &str) -> Result<Option<String>, sqlx::Error> {
        sqlx::query_scalar(AssertSqlSafe(self.version.clone()))
            .bind(key)
            .fetch_optional(pool)
            .await
    }

    /// The source's ranking of the records the caller may read: its fused keys.
    async fn rank(
        &self,
        pool: &PgPool,
        caller: &KnowledgeCaller,
        text: Option<&str>,
        query: &QueryEmbedding,
        depth: i64,
    ) -> Result<Vec<String>, sqlx::Error> {
        let mut transaction = pool.begin().await?;
        sqlx::query("SET LOCAL hnsw.iterative_scan = relaxed_order")
            .execute(&mut *transaction)
            .await?;
        let rows: Vec<(i16, String, i64)> = sqlx::query_as(AssertSqlSafe(self.rank.clone()))
            .bind(text)
            .bind(sqlx::types::Json(caller))
            .bind(vector_text(&query.vector))
            .bind(query.model.checkpoint.as_str())
            .bind(depth)
            .fetch_all(&mut *transaction)
            .await?;
        transaction.commit().await?;
        let mut lists = [Vec::new(), Vec::new()];
        let mut ranked: Vec<(i16, i64, String)> = rows
            .into_iter()
            .map(|(list, key, rank)| (list, rank, key))
            .collect();
        ranked.sort();
        for (list, _, key) in ranked {
            lists[usize::from(list == 1)].push(key);
        }
        Ok(fuse(&lists))
    }

    async fn check(&self, pool: &PgPool) -> Result<(), PgKnowledgeError> {
        let collection = || self.collection.clone();
        let names = vec![
            self.table.key.as_str().to_owned(),
            TEXT_COLUMN.to_owned(),
            VECTOR_COLUMN.to_owned(),
            MODEL_COLUMN.to_owned(),
        ];
        let columns: Vec<(String, String, String, i32, String)> =
            sqlx::query_as(include_str!("pg/columns.sql"))
                .bind(self.schema.as_str())
                .bind(self.table.table.as_str())
                .bind(&names)
                .fetch_all(pool)
                .await
                .map_err(PgKnowledgeError::Database)?;
        if columns.is_empty() {
            return Err(PgKnowledgeError::Table {
                collection: collection(),
                schema: self.schema.to_string(),
                table: self.table.table.clone(),
            });
        }
        let key_type = self.key_type;
        let key_admits = move |kind: &str, schema: &str| kind == key_type && schema == "pg_catalog";
        let found = |column: &str| columns.iter().find(|(name, ..)| name == column);
        if !found(self.table.key.as_str())
            .is_some_and(|(_, kind, schema, ..)| key_admits(kind, schema))
        {
            return Err(PgKnowledgeError::Column {
                collection: collection(),
                column: self.table.key.to_string(),
                expected: if key_type == "uuid" { "uuid" } else { "text" },
            });
        }
        let expectations: [(&str, &'static str, ColumnTest); 3] = [
            (
                TEXT_COLUMN,
                "a tsvector generated always and stored",
                |kind, schema, _, generated| {
                    kind == "tsvector" && schema == "pg_catalog" && generated == "s"
                },
            ),
            (
                VECTOR_COLUMN,
                "_extensions.vector(768)",
                |kind, schema, modifier, _| {
                    kind == "vector"
                        && schema == "_extensions"
                        && modifier == VECTOR_DIMENSIONS as i32
                },
            ),
            (MODEL_COLUMN, "text", |kind, schema, _, _| {
                kind == "text" && schema == "pg_catalog"
            }),
        ];
        for (column, expected, admits) in expectations {
            let admitted = found(column).is_some_and(|(_, kind, schema, modifier, generated)| {
                admits(kind, schema, *modifier, generated)
            });
            if !admitted {
                return Err(PgKnowledgeError::Column {
                    collection: collection(),
                    column: column.to_owned(),
                    expected,
                });
            }
        }
        let indexes: Vec<(String, String, String, String)> =
            sqlx::query_as(include_str!("pg/indexes.sql"))
                .bind(self.schema.as_str())
                .bind(self.table.table.as_str())
                .fetch_all(pool)
                .await
                .map_err(PgKnowledgeError::Database)?;
        let has = |method: &str, column: &str, class: Option<(&str, &str)>| {
            indexes.iter().any(|(found, on, opclass, opclass_schema)| {
                found == method
                    && on == column
                    && class
                        .is_none_or(|(name, schema)| opclass == name && opclass_schema == schema)
            })
        };
        if !has("gin", TEXT_COLUMN, None) {
            return Err(PgKnowledgeError::Index {
                collection: collection(),
                column: TEXT_COLUMN,
                expected: "GIN",
            });
        }
        if !has(
            "hnsw",
            VECTOR_COLUMN,
            Some(("vector_cosine_ops", "_extensions")),
        ) {
            return Err(PgKnowledgeError::Index {
                collection: collection(),
                column: VECTOR_COLUMN,
                expected: "HNSW _extensions.vector_cosine_ops",
            });
        }
        let volatility: Option<String> = sqlx::query_scalar(include_str!("pg/read_rule.sql"))
            .bind(self.schema.as_str())
            .bind(self.table.table.as_str())
            .bind(self.table.read_rule.as_str())
            .fetch_optional(pool)
            .await
            .map_err(PgKnowledgeError::Database)?;
        if volatility.as_deref() != Some("s") {
            return Err(PgKnowledgeError::ReadRule {
                collection: collection(),
                schema: self.schema.to_string(),
                table: self.table.table.clone(),
                function: self.table.read_rule.clone(),
            });
        }
        Ok(())
    }
}

impl<D: DomainServer, E: Embedder + 'static> KnowledgeSearch<D> for PgKnowledge<D, E> {
    async fn search(
        &self,
        context: SearchContext<D>,
        query: KnowledgeQuery,
    ) -> Result<Vec<KnowledgeHit>, ToolError> {
        let identity = hub_identity(&context.request)?;
        let caller = KnowledgeCaller::of(&identity, Utc::now())
            .map_err(|error| ErrorData::invalid_request(error.to_string(), None))?;
        let client = context.request.client()?;
        let embedding = embed_query(&*self.embedder, &client, &query)
            .await
            .map_err(|error| {
                ToolError::Rejected(format!("the query could not be embedded: {error}"))
            })?;
        if embedding.model.dimensions.get() != VECTOR_DIMENSIONS {
            return Err(ToolError::Rejected(format!(
                "the query's model makes vectors of {} dimensions; knowledge holds {VECTOR_DIMENSIONS}",
                embedding.model.dimensions.get()
            )));
        }
        let text = query_text(&query.parts);
        let limit = query.limit.get() as usize;
        let depth = i64::from(RANK_DEPTH * query.limit.get());
        let mut lists = Vec::new();
        for (index, registered) in self.sources.iter().enumerate() {
            if !query.searches(&registered.collection) {
                continue;
            }
            let pool = registered.source.pool(&context.domain);
            let ranked = registered
                .rank(pool, &caller, text.as_deref(), &embedding, depth)
                .await
                .map_err(|_| ErrorData::internal_error("the knowledge search failed", None))?;
            lists.push(
                ranked
                    .into_iter()
                    .map(|key| (index, key))
                    .collect::<Vec<_>>(),
            );
        }
        let mut fused = fuse(&lists);
        fused.truncate(limit);
        let mut keys: BTreeMap<usize, Vec<String>> = BTreeMap::new();
        for (index, key) in &fused {
            keys.entry(*index).or_default().push(key.clone());
        }
        let mut found: BTreeMap<(usize, String), KnowledgeHit> = BTreeMap::new();
        for (index, keys) in keys {
            let registered = &self.sources[index];
            let hits = registered
                .source
                .hits(&context.domain, &context.request, &keys)
                .await?;
            if hits.len() != keys.len() {
                return Err(ErrorData::internal_error(
                    "a knowledge source answered a different number of hits than keys",
                    None,
                )
                .into());
            }
            for (key, hit) in keys.into_iter().zip(hits) {
                if let Some(hit) = hit {
                    found.insert((index, key), hit);
                }
            }
        }
        Ok(fused
            .into_iter()
            .filter_map(|ranked| found.remove(&ranked))
            .collect())
    }
}

/// A source behind the type the search holds, with its keys in their text form.
trait DynSource<D>: Send + Sync {
    fn pool<'a>(&self, domain: &'a D) -> &'a PgPool;
    fn hits<'a>(
        &'a self,
        domain: &'a D,
        request: &'a RequestContext<RoleServer>,
        keys: &'a [String],
    ) -> BoxFuture<'a, Result<Vec<Option<KnowledgeHit>>, ToolError>>;
    fn content<'a>(
        &'a self,
        domain: &'a D,
        client: &'a HubClient,
        key: &'a str,
    ) -> BoxFuture<'a, Result<Option<ContentParts>, String>>;
}

impl<D: DomainServer, S: PgKnowledgeSource<D>> DynSource<D> for S {
    fn pool<'a>(&self, domain: &'a D) -> &'a PgPool {
        PgKnowledgeSource::pool(self, domain)
    }
    fn hits<'a>(
        &'a self,
        domain: &'a D,
        request: &'a RequestContext<RoleServer>,
        keys: &'a [String],
    ) -> BoxFuture<'a, Result<Vec<Option<KnowledgeHit>>, ToolError>> {
        Box::pin(async move {
            let keys = keys
                .iter()
                .map(|key| S::Key::from_text(key))
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| {
                    ErrorData::internal_error("a knowledge key is not of its column's type", None)
                })?;
            PgKnowledgeSource::hits(self, domain, request, &keys).await
        })
    }
    fn content<'a>(
        &'a self,
        domain: &'a D,
        client: &'a HubClient,
        key: &'a str,
    ) -> BoxFuture<'a, Result<Option<ContentParts>, String>> {
        Box::pin(async move {
            let key = S::Key::from_text(key)
                .ok_or_else(|| "the key is not of its column's type".to_owned())?;
            PgKnowledgeSource::content(self, domain, client, key)
                .await
                .map_err(|error| error.to_string())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_are_lowercase_names_postgresql_takes_as_spelled() {
        for admitted in ["artifact", "_private", "a1_b2", &"a".repeat(63)] {
            assert!(PgIdentifier::parse(admitted).is_ok(), "{admitted}");
        }
        for refused in [
            "",
            "Artifact",
            "1st",
            "a-b",
            "a.b",
            "a\"b",
            "a b",
            "artifact; DROP TABLE x",
            &"a".repeat(64),
        ] {
            assert!(PgIdentifier::parse(refused).is_err(), "{refused}");
        }
    }

    #[test]
    fn statements_name_only_the_declared_identifiers_and_bind_every_value() {
        let schema = PgIdentifier::parse("artifact").unwrap();
        let table = PgKnowledgeTable::new("artifact", "artifact_id", "knowledge_readable").unwrap();
        let rank = rank_statement(&schema, &table);
        assert!(rank.contains("FROM \"artifact\".\"artifact\" AS r"));
        assert!(rank.contains("\"artifact\".\"knowledge_readable\"(r, $2)"));
        assert!(rank.contains("SELECT r.\"artifact_id\"::text AS key"));
        assert!(rank.contains("OPERATOR(_extensions.<=>) $3::_extensions.vector"));
        for parameter in ["$1", "$2", "$3", "$4", "$5"] {
            assert!(rank.contains(parameter), "{parameter}");
        }
        assert!(!rank.contains("$6"));
        let write = write_statement(&schema, &table, "uuid");
        assert_eq!(
            write,
            "UPDATE \"artifact\".\"artifact\" SET knowledge_vector = $1::_extensions.vector, knowledge_model = $2 WHERE \"artifact_id\" = $3::uuid AND xmin::text = $4"
        );
        assert_eq!(
            version_statement(&schema, &table, "text"),
            "SELECT r.xmin::text FROM \"artifact\".\"artifact\" AS r WHERE r.\"artifact_id\" = $1::text"
        );
    }

    #[test]
    fn a_vector_crosses_the_driver_in_pgvectors_text_form_and_a_query_joins_its_text() {
        assert_eq!(vector_text(&[1.0, -0.5, 0.25]), "[1,-0.5,0.25]");
        assert_eq!(vector_text(&[]), "[]");
        let parts = ContentParts::new(vec![
            ContentPart::text("red kite"),
            ContentPart::text("  "),
            ContentPart::text("over dunes"),
        ])
        .unwrap();
        assert_eq!(query_text(&parts).as_deref(), Some("red kite over dunes"));
    }
}
