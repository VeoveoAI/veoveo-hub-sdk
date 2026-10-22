# Authoring a hosted MCP server

Implement the domain contract and handlers; let the SDK own the MCP host. Start
from [veoveo-template-mcp-rust](https://github.com/VeoveoAI/veoveo-template-mcp-rust).
The [Observatory contract](../crates/veoveo-hub-sdk-fixture/src/contract.rs) and
[host](../crates/veoveo-hub-sdk-fixture/src/mcp.rs) are a compact working example,
including a collection exposed as both a resource and a tool.

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
4. Use the registration client and the Hub's issued `AccessTokenTrust`. Construct
   the Task runtime with that trust's deployment ID. Use the SDK's request/Task
   call clients for downstream calls so they retain the authenticated caller and
   captured execution context. A server that calls the Hub's own routes for its
   caller declares `.calls_hub()` and sends `HubClient::hub_bearer`.

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

## Deployment identity and environment routing

The logical slug names the API. `DeploymentId` names one admitted deployment of
that API. Several deployments can share a slug while serving different execution
environments. Keep the deployment ID and key directory across ordinary worker
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
startup. Component/template adoption and deployed environment qualification are
tracked in the [execution plan](https://github.com/VeoveoAI/veoveo-docs/blob/main/OPENSHELL_HARDCUT_PLAN.md);
the SDK API alone does not establish that every deployed server has adopted it.

A client selects an environment using the Veoveo HTTP header
`veoveo-environment`. Hub authenticates the caller, admits the selection and
captures `ExecutionContext { environment, composition }`. The signed per-hop
context separately identifies the recipient deployment. Servers use the SDK's
verified context and downstream clients; they do not add environment arguments
to every tool or resolve the component map themselves. Task execution and reads
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
- OAuth registration and registration management follow RFC 7591/7592. Our
  `veoveo_*` client metadata members use OAuth's extension mechanism. Forwarded
  JWT invocation claims and RFC 8693 delegation have their own typed SDK models.
- `veoveo-environment` and the composition model are Veoveo routing contracts,
  not standard MCP tool arguments.
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
