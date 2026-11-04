# Authoring a hosted MCP server

Implement the domain contract and handlers; let the SDK own the MCP host. Start
from [veoveo-template-mcp-rust](https://github.com/VeoveoAI/veoveo-template-mcp-rust).
The [Observatory contract](../crates/veoveo-hub-sdk-fixture/src/contract.rs) and
[host](../crates/veoveo-hub-sdk-fixture/src/mcp.rs) are a compact working example,
including a collection exposed as both a resource and a tool, and a knowledge collection
with its search.

## What authors implement

1. Publish runtime-free request, output and address types through the server's
   `contract` feature. `ProviderContract` supplies the logical server slug;
   `ToolCall` associates a tool request with its output; `ResourceRead` associates
   a resource address with its content.
2. Implement `McpServerContract` and `DomainServer`. The former binds provider,
   scope vocabulary, documents and server information. The latter supplies the
   checked setup and ready `Routes`. `Hosted<D>` implements the MCP handler.
3. Register typed handlers in `Routes::builder(...)`, supplying each route's
   access policy. The route table derives discovery, input/output schemas,
   authorization declarations and dispatch. Do not maintain a second JSON
   authorization table alongside the routes.
4. Serve the guidance for using the server as skills, not in `instructions` or prompts:
   a `skills/` directory with one directory per skill and its `SKILL.md`, a build script
   that runs `veoveo_skills::build::embed_skills("skills")` (with `veoveo-skills` and its
   `build` feature as a build-dependency, and `veoveo-skills` as a dependency), and
   `McpServerContract::skills` returning `server_skills!("{slug}")`. Write a skill for the
   agent that uses the server: what a task needs, the tools by the names callers see,
   `{slug}__{tool}`, and the resources by their addresses. Keep `instructions` to what the
   server is and which skills it serves, and have each prompt name the skill it follows.
   The build refuses a skill a host would refuse.
5. When people search the server's records, express the knowledge contract, the MCP
   extension `ai.veoveo/knowledge`. Mark each searchable template with
   `ResourceBuilder::knowledge(collection)`, a `KnowledgeCollection` named
   `{slug}.{plural}` after the plural noun of its records, implement `KnowledgeSearch`
   over the server's own records under its own read rule, and register it with
   `RoutesBuilder::knowledge(policy, search)`, where `policy` is the server's read
   permission for those records. The SDK publishes `search_knowledge` with the contract's
   schemas, declares the extension, refuses a search without a marked template and the
   reverse, and checks every result before it leaves the server: at most the query's
   limit, each hit an address of its collection's template and of a collection the query
   named. Answer a query in any modality; a signed media part is a credential, so never
   echo its URL or headers. Test it with `veoveo_hub_sdk::testing::knowledge(&hub, text)`
   over the server's fixture, with `text` matching a fixture record, and say in the
   server's skill what its collections hold. A server whose records live in PostgreSQL
   uses the ready-made search, [below](#searching-records-kept-in-postgresql).
6. Use the registration client and the Hub's issued `AccessTokenTrust`. Construct
   the Task runtime with that trust's deployment ID. Use the SDK's request/Task
   call clients for downstream calls so they retain the authenticated caller and
   captured execution context. A server that calls the Hub's own routes for its
   caller declares `.calls_hub()` and sends `HubClient::hub_bearer`.

## Searching records kept in PostgreSQL

`knowledge::PgKnowledge` ranks a collection's rows by their words and their vectors under
the server's own read rule, and keeps their vectors current as they are written. A server
gives it, per collection, three columns and two indexes, a read-rule function and a
`PgKnowledgeSource`; the rest is one call in each write handler and one line in the Task
worker.

**The migration.** In the server's own migration, add to the collection's table the text
PostgreSQL ranks, the vector and the model that made it, and their indexes. pgvector
lives in the installation's `_extensions` schema, which the server's login may use and
nothing else outside its own schema:

```sql
ALTER TABLE artifact.artifact
    ADD COLUMN knowledge_text tsvector GENERATED ALWAYS AS
        (to_tsvector('simple', coalesce(name, '') || ' ' || coalesce(description, ''))) STORED,
    ADD COLUMN knowledge_vector _extensions.vector(768),
    ADD COLUMN knowledge_model text;
CREATE INDEX artifact_knowledge_text ON artifact.artifact USING gin (knowledge_text);
CREATE INDEX artifact_knowledge_vector ON artifact.artifact
    USING hnsw (knowledge_vector _extensions.vector_cosine_ops);
```

The key column is `uuid` or `text`. PostgreSQL keeps `knowledge_text` current with the
row; the SDK fills `knowledge_vector` and `knowledge_model`, the embedding model's
checkpoint, and compares a query only with vectors of its own model.

**The read rule.** A function in the server's schema, `(r <table>, caller jsonb) RETURNS
boolean`, `STABLE`, says whether the caller may read one row. The search applies it
before ranking and before the limit, so a page is never short for rows the caller may
not read. `caller` is the verified caller, a `KnowledgeCaller`:

```json
{"tenantId": "1b3c6f0e-…", "tenantKey": "tenant-a",
 "principalId": "8d2e41a7-…", "principalKey": "https://idp.example.com#user-1",
 "groups": [{"group": "engineering", "role": "read"}],
 "dataLabels": ["pii"], "asOf": "2026-11-03T10:00:00.123456Z"}
```

`tenantId` and `principalId` are the UUIDs core keeps, which the server's own rows name;
`tenantKey` and `principalKey` are the same as tokens name them, which a grant names.
A rule written for the search alone:

```sql
CREATE FUNCTION shop.item_readable(r shop.item, caller jsonb) RETURNS boolean
LANGUAGE sql STABLE PARALLEL SAFE SET search_path = pg_catalog, pg_temp
RETURN r.tenant = (caller->>'tenantId')::uuid
    AND (r.public OR r.owner = (caller->>'principalId')::uuid);
```

Keep one rule for listings and search: a server whose rule is already a function wraps
it, reading its arguments from `caller`:

```sql
CREATE FUNCTION artifact.knowledge_readable(a artifact.artifact, caller jsonb) RETURNS boolean
LANGUAGE sql STABLE PARALLEL SAFE SET search_path = pg_catalog, pg_temp
RETURN a.state = 'available'
    AND artifact.effective_level(
        a,
        (caller->>'tenantId')::uuid,
        (caller->>'principalId')::uuid,
        caller->>'principalKey',
        ARRAY(SELECT e.g->>'group' FROM jsonb_array_elements(caller->'groups')
            WITH ORDINALITY AS e (g, n) ORDER BY e.n),
        ARRAY(SELECT e.g->>'role' FROM jsonb_array_elements(caller->'groups')
            WITH ORDINALITY AS e (g, n) ORDER BY e.n),
        ARRAY(SELECT jsonb_array_elements_text(caller->'dataLabels')),
        (caller->>'asOf')::timestamptz
    ) >= 1;
```

**The source.** A `PgKnowledgeSource` names the collection, the table, its key column and
the function, all in the server's schema, and declares the key's type: `Uuid` for a
`uuid` column, `String` for a `text` one. It loads the hits of ranked keys through the
server's own code, in their order, and gives the content to embed for one record: its
text and its media as signed URLs.

```rust
pub struct Artifacts;

impl PgKnowledgeSource<ArtifactDomain> for Artifacts {
    type Key = Uuid;
    type Error = ContentError;
    fn collection(&self) -> KnowledgeCollection {
        KnowledgeCollection::parse("artifact.artifacts").unwrap()
    }
    fn table(&self) -> PgKnowledgeTable {
        PgKnowledgeTable::new("artifact", "artifact_id", "knowledge_readable").unwrap()
    }
    fn pool<'a>(&self, domain: &'a ArtifactDomain) -> &'a PgPool {
        domain.pool()
    }
    async fn hits(
        &self,
        domain: &ArtifactDomain,
        request: &RequestContext<RoleServer>,
        keys: &[Uuid],
    ) -> Result<Vec<Option<KnowledgeHit>>, ToolError> {
        domain.hits(request, keys).await // one per key, in order; None for one gone
    }
    async fn content(
        &self,
        domain: &ArtifactDomain,
        client: &HubClient,
        key: Uuid,
    ) -> Result<Option<ContentParts>, ContentError> {
        domain.content(client, key).await // None when there is nothing to embed
    }
}

pub static KNOWLEDGE: LazyLock<PgKnowledge<ArtifactDomain, EmbeddingClient>> =
    LazyLock::new(|| PgKnowledge::new(EmbeddingClient).source(Artifacts));
```

`EmbeddingClient` is the Embedding server's `Embedder`, from its `client` feature. Register
the search and declare what the writes need: the use of Embedding's permission, since
the server embeds as its caller, and `starts_tasks()`, since a direct write starts a Task.
Mark the template as for any search. Check the tables before serving; `check` refuses a
missing column, index or function, a key of another type and a rule that is not `STABLE`:

```rust
Routes::builder(RoutePolicy::Authenticated)
    .template::<ReadArtifact, _, _>(ResourceBuilder::new("Artifact").knowledge(collection()) /* … */)
    .knowledge(read_policy(), KNOWLEDGE.clone())
    .uses(PermissionName::parse("embedding.embeddings.create")?)
    .starts_tasks()
    // …
    .build();

KNOWLEDGE.check(&pool).await?;
```

**Writes.** After a write commits, the handler calls `index_written` once for the records
it wrote, last:

```rust
let indexing = KNOWLEDGE
    .index_written(&domain.runtime, &context, &domain, &Artifacts, &[artifact_id])
    .await?;
match indexing {
    // The caller wrote directly: an index Task embeds as the caller, and the routes name
    // it in the result as its related task.
    WriteIndexing::Task(task) => { /* say the record is being indexed */ }
    // Another server wrote for the caller: embedded in this call, each record's outcome.
    WriteIndexing::Indexed(outcomes) => { /* IndexedRecord::of(&key, &outcome) for each */ }
}
```

A record that was not embedded is still found by its words, until its next write embeds
it. Two runs for one record never leave the older content's vector: the SDK reads the
row's version, `xmin`, before `content` and writes the vector only over that version,
reading a record written meanwhile again, at most 3 times in all, before it reports that
the record kept changing. `content` therefore reads the record's current row. The index Task fails, naming the records and why, when one could not be embedded, so
the person sees it among their Tasks.

**Tasks.** The server serves `tasks/*` for its callers' index Tasks with a
`DurableTaskService` over its Task runtime, whose `start_tool_task` returns `None` when it
has no Task tool and whose other methods are the SDK's `get_durable_task`,
`update_durable_task`, `cancel_durable_task` and `subscribe_durable_tasks` for
`runtime.for_owner(&task_owner(caller)?)`, hosted with
`Hosted::new(domain).with_tasks(DurableTasks::tasks_only(tasks))`. Its worker carries the
server's calls, so an index Task calls Embedding as its caller, and runs index Tasks:

```rust
let worker = TaskWorker::new(runtime.clone()).calls(calls);
worker
    .run(
        move |ctx| {
            let domain = domain.clone();
            async move {
                if KNOWLEDGE.runs(ctx.task()) {
                    return Ok::<_, TaskError>(KNOWLEDGE.run_index(&domain, &ctx).await);
                }
                // … the server's other Task types
            }
        },
        stop,
    )
    .await?;
```

**Tests.** Test on core's store fixture with PostgreSQL 18.6 and pgvector 0.8.7 on `PATH`,
which the development image installs: `TestDb::create_extensions(&["vector"])` creates the
`_extensions` schema and pgvector as the installation's bootstrap does, before the
server's migrations. A test whose Task worker runs while its fixture drops uses a
multi-thread runtime. The SDK's own tests do this with synthetic embeddings. See [`tests/pg_knowledge.rs`](../crates/veoveo-hub-sdk/tests/pg_knowledge.rs).

## Traits enforce associations; attributes generate implementations

`#[tool(...)]`, `#[resource_read(...)]`, `#[collection_tool(...)]` and
`#[collection_resource(...)]` are Rust attribute procedural macros. They generate
ordinary trait implementations. Authors can implement those traits directly; the
route builder requires the same associations either way. The attributes are an
authoring convenience, not a separate compliance check.

For example, this excerpt from Observatory declares a collection tool:

```rust
pub struct ReadingCollection;

impl veoveo_types::Collection for ReadingCollection {
    type Item = Reading;
    type Filters = veoveo_hub_sdk::definition::NoFilters;
    type Key = String;
    type Limit = veoveo_types::PageSize<2>;
    type Policy = veoveo_types::LiveKeyset;

    const NAME: &'static str = "observatory.readings";

    fn key(item: &Reading) -> String {
        item.id.as_str().to_owned()
    }
}

#[veoveo_hub_sdk::collection_tool(
    provider = Observatory,
    collection = ReadingCollection,
    name = "readings",
    default_limit = 2
)]
pub struct ReadingsTool;
```

Its handler receives `List<ReadingsTool>` and returns
`Result<Page<ReadingCollection>, ErrorData>`. Register it with
`.collection::<ReadingsTool, _, _>(description, policy, handler)`. That builder
requires the exact page output; a handler returning `Vec<Reading>` does not fit.
The SDK produces the tool's paging input and output schemas from these types.

For a resource, declare `CollectionResource` (or use its attribute), receive
`ReadCollection<R>`, and return `Page<R::Collection>`. The SDK supplies the shared
`limit` and `cursor` URI query handling and RFC 6570 template parameters. Tools and
resources use the same `CollectionRequest<C>` and `Pager<C>` mechanics.

## Pages are constructed from an admitted query

The collection defines its item, filters, unique ordering key, bounded page size
and consistency policy. Use a key containing a unique tie-breaker and make the
database ordering match `Collection::compare`.

The handler constructs `Pager<C>` with the current tenant, effective filters,
limit, policy and incoming cursor. Use `pager.after()` and `pager.fetch_limit()`
for the ordered query; then call `pager.finish(rows)`. Fetch up to `limit + 1`
rows so the extra row determines whether continuation exists. An already ordered
in-memory collection can use `pager.select(items)`.

`Page<C>` has private fields. The pager checks ordering, cursor position and fetch
bound, truncates the lookahead row and constructs the continuation itself. There
is no page constructor taking independently chosen items and a next cursor.
`CursorToken<C>` prevents mixing collection families in Rust; decoding a wire
cursor also checks its scope against the current listing, tenant, filters, limit
and, for revision-bound paging, revision. Cursor scope is not authorization:
apply the caller's current data permissions in the query.

## Builders require complete routes

`ResourceBuilder<R, H, P, K>` uses `Missing` and `Ready<T>` states. A resource
cannot enter `Routes` until it has all three of reader, access policy and cache
policy. For example, the Observatory resource is registered as:

```rust
.collection_resource::<ReadReadings, _, _>(
    Some(ReadReadings {}),
    ResourceBuilder::new("Readings")
        .reader(read_readings)
        .policy(RoutePolicy::Authenticated)
        .cache(ReadCache::Private),
)
```

`Authenticated` is an explicit policy choice. Permission-protected routes use
the server's declared permission vocabulary. Policies also supply tool annotation
hints; the hints themselves do not grant access.

## Deployment identity and routing

The logical slug names the API. `DeploymentId` names one admitted deployment of
that API. Keep the deployment ID and key directory across ordinary worker
restarts. Admit a new deployment for a changed worker contract, endpoint or key;
do not generate a new ID every time a process starts.

`RegistrationDeclaration` requires typed `deployment`, `slug`, `uri_scheme`,
`version` and `endpoint` fields. `KeyDirectory::prepare(declaration, authorization)`
creates the public `ClientMetadata` using the durable worker key, without a
network request, database connection or admission credential. Serving uses that
same metadata and `RegistrationClient::resume_or_register(&keys)`. It persists
native RFC 7592 client information; shutdown preserves it. Explicit retirement
deletes the registration. A refused saved registration needs fresh admission;
the SDK does not silently retry with broader namespace credentials.

The uniform executable interface being adopted by components is
`--deployment-id` / `VEOVEO_DEPLOYMENT_ID` and `--describe-registration`. Describe
prints that public metadata and exits before telemetry, database and provider
startup. Component/template adoption and deployed qualification are
tracked in the [qualification checklist](https://github.com/VeoveoAI/veoveo-docs/blob/main/QUALIFICATION_CHECKLIST.md);
the SDK API alone does not establish that every deployed server has adopted it.

Hub routes each request and captures `ExecutionContext { composition }`: a slug with
factory instances, or one the caller's selection names, routes by branch; any other
slug routes to its deployment with live registrations, the newest when several have
them. The signed per-hop context separately identifies the recipient deployment.
Servers use the SDK's verified context and downstream clients; they do not add
routing arguments to every tool or resolve the component map themselves. Task execution and reads
retain their original deployment/composition. Current permissions still apply.

## Which wire fields are standard

MCP supplies tools, resources, schemas, structured tool content and metadata
extension points. The protocol revision's extension mechanism is standard; the
Veoveo vocabulary carried through it is ours. See the
[MCP metadata rules](https://modelcontextprotocol.io/specification/draft/basic#_meta)
and the SDK's [registration wire mapping](../crates/veoveo-hub-sdk/src/registration/DESIGN.md).

- `ai.veoveo/registration` and `ai.veoveo/authorization` are Veoveo declarations
  under MCP capabilities extensions, assembled from typed registration and routes.
- `ai.veoveo/usage` is Veoveo Task usage metadata, emitted by the Task adapter.
- `ai.veoveo/task` is a call's Task metadata, `TaskMeta`, such as its retention pin.
- `ai.veoveo/hub` is the Hub's result metadata, `HubMeta`, such as the servers a
  federated list lacks.
- `ai.veoveo/knowledge` is the knowledge contract: a capabilities extension a server
  declares when it searches its records, and each searchable template's `_meta`
  `CollectionMarker`.
- OAuth registration and registration management follow RFC 7591/7592. Our
  `veoveo_*` client metadata members use OAuth's extension mechanism. Forwarded
  JWT invocation claims and RFC 8693 delegation have their own typed SDK models.
- The composition model is a Veoveo routing contract, not a standard MCP tool
  argument.
- `Page<C>` is our domain collection wire contract. MCP's catalog pagination
  does not itself require arbitrary list-returning tools to use this page shape.

## Exact boundary of construction guarantees

Types enforce handler associations, supported output cardinality, collection
families, complete resource builders and required deployment fields. Constructors
and decoders admit runtime values such as identifiers, URLs, page bounds and
cursor scope. The common host owns the protocol implementation. The former
standalone conformance crate and certification commands are removed.

This does not prove every domain implementation correct. A developer can declare
a custom single-output object containing an unbounded vector; Rust cannot infer
that it semantically represents a growing collection. Declare such listings as
collections. Current permissions, signatures, database query completeness,
provider behavior and recovery are runtime facts. Keep their normal component
tests and boundary checks; do not duplicate structural checks in a second
conformance system.
