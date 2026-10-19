# Veoveo MCP Server Contract

This document is the normative contract for every hosted MCP server registered with a
Veoveo installation. It states the protocol, schema, runtime, packaging, documentation
and self-description requirements. The crate in this directory, `veoveo-hub-sdk`,
implements the shared mechanics that make most of the contract hold by construction.

The contract is identified by `https://veoveo.ai/contracts/hosted-mcp`, exported as
`veoveo_hub_sdk::docs::ContractId::HostedMcp` and as the string
`veoveo_hub_sdk::HOSTED_MCP_CONTRACT`. A server names the contract it implements in its
crate documents and in its contract resource.

## Standards And Protocols

| Standard or protocol | Supported profile |
|---|---|
| Model Context Protocol | protocol version `2026-07-28`; Discover is mandatory and Initialize is excluded from the hosted profile |
| MCP Streamable HTTP | stateless POST requests with JSON terminal responses capped at 8 MiB after serialization; SSE is used only by methods whose final flow requires a stream; protocol sessions, reconnect GET, DELETE, and replay are excluded |
| MCP Tasks, SEP-2663 | official `tasks/get`, `tasks/update`, and `tasks/cancel`, task status notifications through `subscriptions/listen`, opaque task IDs, and typed terminal payloads |
| MCP multi-round requests, SEP-2322 | `input_required`, protected opaque `requestState`, and retry `inputResponses`; server-initiated elicitation is excluded |
| MCP subscriptions | request-scoped `subscriptions/listen` with an authorized accepted filter; resource subscribe and unsubscribe are excluded |
| rmcp 3.5.0 | the official Rust SDK, pinned to the `VeoveoAI/rust-sdk` fork at `917e791` for Task status subscriptions and typed HTTP rejections, with default features off so each SDK feature enables only the rmcp features it uses; the pin's comment states the removal condition |
| Requirement catalog `https://veoveo.ai/formats/hosted-mcp-requirements` | C01–C31 and C33 document architectural requirements; the catalog is generated from native types. No owner compliance status or runtime certification is recorded. |
| JSON Schema 2020-12 | ordinary rmcp and Schemars generation with bounded references and composition; each published tool argument declares one `type`, an optional argument admits no `null`, and every subschema declares `type` wherever references, composition, `const` or `enum` restrict it; `Hosted` explains a refused call's arguments against the published schema with jsonschema 0.58.6 |
| RFC 6570 URI Templates | checked MCP setup uses the foundational ASCII-template profile and the iri-string 0.7.14 parser; domain tests qualify expansion against typed resource addresses |
| W3C Trace Context and Baggage | `traceparent`, `tracestate`, and `baggage` in MCP request metadata with the authenticated HTTP boundary as the trust gate |
| OAuth 2.0 Dynamic Client Registration, RFC 7591 and RFC 7592 | servers register, update and remove themselves through the profile in [the server registration protocol](src/registration/DESIGN.md) |
| JWT access tokens, RFC 9068, with Rich Authorization Requests, RFC 9396 | the Hub-to-server identity: an EdDSA `at+jwt` token whose `iss` is the Hub's issuer URL, `aud` the server's resource URL (RFC 8707), `client_id` the caller's OAuth client and `jti` a UUIDv7, with the Hub in the RFC 8693 `act` claim and the calling server as its prior actor when another server made the call, granted permissions in one `https://veoveo.ai/oauth/permissions` authorization detail, and the actor, authority and request context in the `https://veoveo.ai/claims/invocation` claim; a hosted server verifies it with its registration's keys and refreshes them from the issuer's RFC 8414 `jwks_uri` when a token names an unknown key; specified in [the server registration protocol](src/registration/DESIGN.md#forwarded-identity) |
| OAuth 2.0 client credentials, RFC 6749 section 4.4, with RFC 7523 client authentication, RFC 9728 protected resource metadata and RFC 8414 authorization server metadata | `ClientCredentialsBearer` in the `client-credentials` feature: a caller's own token for one profile, with `resource` naming the profile's MCP endpoint, RFC 8707 |
| OAuth 2.0 Token Exchange, RFC 8693 | calls from a server to other servers for its caller: the profile of [calls between servers](src/registration/DESIGN.md#calls-between-servers), sent by `HubClient` over MCP Streamable HTTP to the Hub profile's endpoint |
| Veoveo access-token `session_family` | signed UUIDv7 refresh-family binding for browser tokens; the Hub enforces current family revocation, and absence supplies no renewable session authority |
| Veoveo `request_context` | signed source principal, verified access-token metadata and required audit request correlation, including the OAuth client and optional session family; it contains no bearer value and grants no independent renewal permission |
| MCP Apps, SEP-1865 | `ui://` App resources and tool links, specified in [MCP Apps](src/apps/DESIGN.md) |
| [Veoveo audit envelope](https://github.com/VeoveoAI/veoveo-core/blob/9a6b3ff95469ff19f9ccb513421a57ba6751aa9e/platform/audit/contract/DESIGN.md), after NIST SP 800-53 Rev. 5 AU-3 and mapped to OCSF 1.9 | the classes `mcp.discovery` and `mcp.read` (OCSF API Activity Read) and `mcp.tool_admission` and `mcp.tool_completion` (API Activity Other), which an MCP producer declares under the owner `mcp`; targets are `veoveo://` URIs built by the shared resource URI builder; the acting server and Task travel in the envelope's `via`, and the OAuth client, delegating principal, policy revision, scopes and data labels in each detail's `invocation` |
| [Veoveo durable Task runtime](https://github.com/VeoveoAI/veoveo-core/blob/9a6b3ff95469ff19f9ccb513421a57ba6751aa9e/platform/task/DESIGN.md) | durable steps, and operations whose outcome arrives through provider events or a bounded check schedule, run through `TaskWorker` and `TaskContext` |
| Provider webhooks over HTTP | `POST {mount}/operations/{operation_id}/{token}` and `POST {mount}/providers/{provider}/events` on a server's public routes; the server's parser checks the provider's signature, and the route answers 204 only after the Task runtime stored the event |
| PostgreSQL SQL and PL/pgSQL, through libpg_query 17-6.2.5 (`pg_query` 6.2.1) | the `testing` feature's SQL confinement check parses a server's query files, migrations, sqlx macro literals and function bodies with PostgreSQL 17's grammar; SQL only PostgreSQL 18 accepts fails the check instead of passing unread |
| FOCUS 1.4 and ISO 4217, through [core's usage ledger](https://github.com/VeoveoAI/veoveo-core/blob/9a6b3ff95469ff19f9ccb513421a57ba6751aa9e/platform/usage/DESIGN.md) | usage kinds a server declares under its slug, estimates and actual usage of a Task, and the Task's cost under the `ai.veoveo/usage` `_meta` key of its result and of `tasks/get` |

A hosted server declares its MCP endpoint when it registers with the Hub and keeps
its registration while its process holds a live kernel instance on the login the
registration grants. The Hub never uses an MCP request, an authentication failure,
or a method rejection as a health signal.

## Domain Contract Ownership

A server's library owns its public models and addresses and exposes them through an
isolated `contract` feature that builds with default features disabled. Consumers
import that feature. The SDK owns no domain models and depends on no server library.

The contract governs every server that registers with a Veoveo Hub, in any
implementation language. Checks are generic over the Hub's registration catalog
and never enumerate servers by hand. A server is checked once it registers, and no
conformance manifest, Console page or documentation index changes when a server is
added.

A server that uses another server declares the permissions it uses when it registers
and calls that server through the Hub for the forwarded caller. It never reaches
another server's storage, private HTTP or credentials.

## Typed Scopes And Resource Identities

Each owning contract declares a closed enum for the scopes its code understands. The
enum maps each variant to one OAuth wire spelling, and domain authorization helpers
accept that enum. A generic policy engine accepts validated `ScopeName` values because
installations and external providers can define additional scopes. `ScopeName`
validates syntax; it does not establish membership in a domain's scope vocabulary.

Server libraries own their scope enums, IDs, resource address variants and Task
operation enums. Operations implement `veoveo_types::TaskTypeDefinition`, and the
`Vocabulary` derive with the `task_type` hook checks complete, distinct names at
compilation. Shared Task admission and snapshots carry `TaskTypeName`, and MCP adapters
parse incoming operation names into their own enums for exhaustive Task dispatch.

Domain IDs keep their specific types in internal requests, cursors and query APIs.
Resource URI constructors require the corresponding ID types, and parsers validate the
wire URI and return a typed resource variant before dispatch. The shared resource URI
abstraction delegates component parsing and encoding to a maintained URI library.
Domain builders own route shapes and accepted parameter names, reject duplicate or
unsupported parameters and validate each parent ID before dispatch. Code does not use
interpolation, concatenation or delimiter chains to assemble or parse dynamic resource
URIs, and fixed discovery literals and URI templates require agreement tests with the
builders.

Task-backed resource addresses may implement the foundational `TaskResourceAddress`
trait. The [Task runtime](https://github.com/VeoveoAI/veoveo-core/blob/9a6b3ff95469ff19f9ccb513421a57ba6751aa9e/platform/task/DESIGN.md) can then deliver
requested Task status and resource invalidations from one authorized watch.

`McpServerSetup` checks implementation and document identity, required document
coverage, resource capability and duplicate declarations before handlers serve
discovery. It provides sorted resources and templates and compares typed permissions
against an authenticated grant set. `McpResourceTemplate` binds metadata to the
foundational `ResourceTemplateUri`, which uses the RFC 6570 parser, and its builder
rejects a descriptor that changes the admitted template. Setup establishes API and
declaration consistency; ordinary implementation tests establish dynamic behavior.

### Library Features

| Feature | Contains | Excludes |
|---|---|---|
| `contract` | the forwarded identity types, registration and authorization declarations, the typed tool calls and resource reads a provider publishes, the token exchange wire types, the Hub's client-visible metadata, the document and contract declarations, the MCP audit vocabulary and the usage kinds and Task cost types | rmcp, tokio, axum, reqwest and sqlx; `tests/contract_boundary.rs` fails when the resolved graph contains them |
| `client` | `HubClient`: typed calls, Task handles and typed resource reads and waits through a Hub profile over MCP Streamable HTTP, with rmcp's client and Streamable HTTP client transport | rmcp's server, server transport and macros, axum, jsonwebtoken, OpenTelemetry, sqlx and core's Task runtime; `tests/contract_boundary.rs` fails when the resolved graph or rmcp's features contain them |
| `client-credentials` | `client` plus `ClientCredentialsBearer`, a caller's own RFC 6749 client-credentials token for a profile, requested with an RFC 7523 assertion that core's OAuth client signs with the caller's key, and `caller_signer`, which reads an installation's caller key files | rmcp's server, server transport and macros, axum, OpenTelemetry, sqlx and core's Task runtime |
| `server` (default) | hosting, the Streamable HTTP profile, forwarded-token verification, the registration client, calls to other servers for the current caller, MCP Apps, telemetry, the MCP Tasks adapter over core's durable Task runtime, [Task work](#task-work) with its provider event routes, and the usage ledger | |
| `testing` | `server` plus the in-process Hub in `hosting::testing` for server tests and `TestMesh` for calls between servers, whose origin also delivers requests under a served server's mount to its public routes, as an ingress would, and `testing::assert_sql_confined`, the check that a server's SQL names only its own schema | |

The Hub attaches discovery degradation and App dependency metadata to federated
lists. Their DTOs live in the `hub` module, and `HubDiscoveryMetadata`
converts degradation to and from MCP `MetaObject`.

## Protocol Surface

Veoveo does not flatten MCP into a collection of convenience tools. Each
server uses the protocol surface that matches its domain:

| Need | Canonical MCP surface |
|---|---|
| action | tool with declared input and output JSON Schemas |
| durable action | task-augmented tool through the MCP tasks API |
| addressable state | resource or resource template |
| discovery | resource list/template plus completion |
| reusable interaction | prompt |
| live condition | `subscriptions/listen` and a resource notification filter |
| progress/result wake | task-ID filter on `subscriptions/listen`; `tasks/get` remains the correctness path |
| cross-server identity | canonical URI and resource link |

A successful terminal task that creates an addressable product returns one
top-level `resultUri` in `structuredContent`. The value is the canonical URI
owned by the producing domain. The adjacent human-readable content is a short,
identity-free status and contains one resource link for that result. Typed
provenance and product metadata remain in structured content. A task that does
not create an addressable product omits `resultUri` and does not invent a
resource identity.

The product rule also applies when an MCP tool result sets `isError` and retains
an addressable output, such as a command transcript. An explicit null address is
invalid. `task_completion::result_uri` validates the declared address and matching
link before the Task adapter stores the result and its separate typed product URI.
The Task runtime keeps the result payload opaque.

Growing domain collections are read through bounded domain-owned pages with a
stable order and opaque cursors. Exact canonical-URI reads use the owning
domain identity and do not require a full collection scan. `resources/list`
advertises stable roots and templates rather than enumerating every dynamic
record. Completion queries are bounded at their authoritative store.

### Domain Collection Paging

C04 governs growing domain collections exposed by `resources/read` and pageable
tool results. Each owner MUST document and test these observable guarantees:

- A page has a positive default and maximum item limit and a maximum serialized
  response byte size within C10's final 8 MiB JSON response cap. Owners define
  invalid-limit behavior and how an item that cannot fit is refused; continuation
  must make cursor progress and terminate on unchanged data. Empty intermediate
  pages are permitted; clients still follow a non-null continuation.
  Optional totals declare whether they are exact or estimated and their consistency
  scope; clients use `nextCursor`, not totals or page length, to detect completion.
- Requests accept an optional opaque `cursor`; JSON pages expose `nextCursor`.
  Missing or null `nextCursor` means completion; any non-null token means continue.
  A resource collection continues on the same collection URI with exactly one
  `cursor` query parameter, URI-encoded as query data. Preserve every other query
  argument. Duplicate cursor parameters are refused. A template that advertises
  continuation uses RFC 6570 query expansion; a documented root may accept cursor
  queries without advertising such a template. Tools pass continuation in their
  declared input. Clients do not decode, normalize or infer token contents.
  Opacity is a client obligation, not a requirement for encryption or a globally
  unique token spelling.
- Cursors have a documented finite byte cap and are bound to the listing and its
  effective filters, order, and applicable tenant/owner/role scope. A changed limit
  is either supported without loss or refused as documented. Malformed, oversized,
  wrong-listing and wrong-scope cursors are refused with a documented error and
  restart instruction. They never silently restart or select a different listing.
- Ordering is deterministic and total, with a unique stable tie-breaker. On
  unchanged data and authorization, following continuation terminates and returns
  every matching identity exactly once, in order. An exact page-size boundary
  terminates correctly, directly or through a documented final empty page.
- Every page applies current authorization. A cursor is a position, never an
  authority grant; possession cannot restore revoked access or cross a tenant or
  owner boundary. Unsupported cursor replicas or expired snapshots fail explicitly.
- The owner declares mutation semantics: live keyset traversal, revision-bound
  continuation, or a retained snapshot. It describes visibility of inserts,
  deletes, reorderings and authorization changes, and restart/expiry behavior.
  Request-local snapshots do not imply cross-request snapshot consistency.

Core owns one protocol-neutral `Page<C: Collection>` with private items, limit and
continuation. `Pager<C>` admits the contextual cursor, owns bounded lookahead and
checks the collection's total ordering before issuing continuation. The selected
keyset consistency policy hashes filters, tenant and limit (plus revision where
required). A received page's deserialization checks observable bounds; it does not
turn the token into contextual authority. The owner supplies the indexed query
and current authorization.

MCP catalog methods retain protocol pagination in `CatalogPage<T>`, including
following any non-null cursor token, even an empty string. Finite immutable SDK
documentation indexes keep their document-position cursors.

`CollectionRequest<C>` owns limit/cursor selection for both `List<T>` tools and
`ReadCollection<R>` resources. Resource owners map only path/filter parameters;
parent and principal identities belong in `Collection::Filters` when they affect
the listing. `collection_template` merges the shared paging query variables into
the owner's trailing optional query expression, or adds one. Discovery and
registration authorization use that one admitted template.

Owners test query ordering, multi-page traversal, exact boundaries, mutation,
current authorization and replica behavior where supported. There is no runtime
conformance inspector, evidence registry or owner self-attestation.

An agent reads resources through a governed current-profile adapter rather than an
unrestricted protocol peer. The adapter admits absolute domain resource URIs and
bounded text or JSON content. It rejects browser, local-file, network, credential,
fragment, HTML, event-stream, binary, and oversized inputs. Episode-local accounting
limits read count, resource families, bytes, wall time, and pagination depth.

A missing resource remains JSON-RPC `Invalid Params` (`-32602`) on the wire. The agent
may receive a fixed correction record containing a sanitized requested URI, a stable
code, static guidance, `automatic_retry: false`, and the remaining safe budget. The
adapter never copies upstream error text or data into model context. Authorization,
timeout, transport, and internal failures remain fixed generic failures, while a
schema-valid domain rejection remains an ordinary tool result with `isError: true`.

Compatibility helpers are allowed only when they are explicit product features
for clients that cannot use the richer MCP surfaces well. They must be
additive projections over the canonical protocol behavior and must reuse the
same typed models, policy checks, audit paths, task state, product
identities, and resource URIs. Hidden fallbacks, alternate completion paths,
unaudited content URLs, and second sources of truth are prohibited.

The hub records authorization and execution as separate audit facts. A
policy event reports whether `tools/call` was admitted. After an admitted call
returns, a tool-call event reports its outcome, the bounded result
kind, duration, and the JSON-RPC error code when the failure crossed that
boundary. A call another server made for its caller names that server, and the Task it
called from, in the record's `via`. It never records tool arguments, provider payloads, credentials, or
an upstream error message. Both records carry the same trace identity, which
keeps policy admission distinct from domain or protocol completion.

The final Tasks extension negotiates `io.modelcontextprotocol/tasks` in Discover
and each request's client capabilities. The server decides whether an admitted
`tools/call` returns a durable Task. An operation that requires Tasks rejects a
client without the extension using `-32021` and its required-capabilities payload,
before accepting work. The published
[Tasks extension](https://github.com/modelcontextprotocol/ext-tasks/blob/main/schema/2026-07-28/schema.ts)
replaces the older per-tool `execution.taskSupport` and request `task` handshake;
those fields are not part of this profile.

Durable Task ownership binds the actual actor, tenant, Work Context, profile and
invocation provenance. Current access checks apply on every read, update, cancel
and subscription. Signing in again or changing unrelated scopes does not create a
different owner. Retained data labels and current domain permissions still constrain
results and actions. The hub's persisted transition is specified in
[`Task Routes`](https://github.com/BiomaAI/veoveo/blob/0c33cf8a7e5a1d46f953702553ac6643f99c1363/platform/hub/src/state/task_routes/DESIGN.md).

A `tools_compat` registration may explicitly enable the direct task-call adapter.
That adapter supplies the negotiated capability upstream, waits on the canonical
subscription and returns the terminal result with its canonical Task ID. It shares
the original Task and authority. The typed `veoveo://tasks/{task_id}` resource exposes
current status and terminal result without introducing another task store.

## Stateless Transport And Explicit State

Every network endpoint uses MCP Streamable HTTP with stateless POST requests.
Clients call Discover and attach the selected protocol version, client identity,
and effective per-request capabilities to each ordinary request. Terminal results
use JSON. A request uses SSE only when its final protocol flow requires a stream,
including `subscriptions/listen` and in-flight progress. Initialize, protocol
session IDs, reconnect GET, DELETE, and Last-Event-ID replay are rejected.

Legacy HTTP+SSE is unsupported. Network stdio is not a transport or
registration value. Stdio may exist only between one local bridge process and
the child MCP server whose lifecycle that bridge owns. The hub always sees
the bridge as a Streamable HTTP endpoint.

Every cross-call state value is an opaque typed handle whose authority, expiry,
integrity, and replay rules are validated on each request. Domain sessions such as
UAV simulation sessions remain application state; they never derive authority or
lifetime from MCP transport locality.

The hub signs a fresh RFC 9068 access token for every upstream request. Ordinary
requests have a 60-second maximum token lifetime. A `subscriptions/listen` POST has a
15-minute maximum, because the source enforces token expiry throughout the stream.
Both lifetimes end at the source access token's expiration when it is earlier. This
permits a longer bearer lifetime only for tokens minted when opening a subscription;
those credentials stay on the private hub-to-server transport. Clients renew their
own authority and reopen listeners before that deadline. Connection reuse depends only
on validated transport security and catalog generation; request authority stays in the
token and request metadata.

Authenticated MCP and HTTP proxy requests include typed `request_context` in the
token's `https://veoveo.ai/claims/invocation` claim, which refuses unknown fields. Access-token
metadata carries checked `AuditManagedExecution` attribution rather than an owner
public claim.
Attributed requests must match automated service identity, client, tenant and Work
Context and cannot carry a browser session family. It preserves the JWT-verified principal before delegated actor
derivation and its access-token metadata. Rust and Python verify the context against
the actor, tenant, Work Context, scopes and invocation provenance. The token's
expiration cannot exceed the source access token's expiration. Every supplied context
also carries the platform audit contract's checked request, trace and span IDs and
optional source IP address. Reusing a transport
never supplies another browser family's request context.

Bootstrap fixtures and reconstructed Task owners may omit this field. Omission grants no durable or renewable request authority. A consumer
that admits renewable access or stores current-policy execution authority
must require this context and perform its own current policy/grant checks. It must
not infer client identity or family membership from an old Task owner. Accepted
background work can outlive admission tokens only under its explicit durable
execution policy, with fresh dispatch authority and separately bounded access.
Shared direct, delegated and automated fixtures qualify the Rust reader.

Hub traffic with the same validated transport-security configuration and
active catalog revision shares one process-wide HTTP connection pool and one
initialized TLS trust store. Construction is single-flight under concurrency.
A catalog revision or transport-security change selects a new pool identity.

Capability declarations name the exact signal a server can produce.
`tools.listChanged`, `prompts.listChanged`, and `resources.listChanged` are
independent claims. The hub merges and forwards those upstream claims. An
isolation-mode profile additionally declares hub-owned resource and tool
list changes because its authorized federated catalog grows as independent
discoveries complete.

Federated resource discovery and isolation-mode tool discovery start missing
servers independently. A list call gives these shared operations one two-second
settlement window across all selected servers, then returns the authorized
per-server cache entries available at that point. Warm complete catalogs return
immediately. This bounded window prevents routine cache expiry from producing an
empty first snapshot while keeping slow optional servers isolated. The hub
captures entries valid when the list request begins and includes them even if
another server consumes the settlement window before response assembly. The
response attaches a typed
`ai.veoveo/hub-discovery-degradation` result metadata document naming only
the missing server, surface, and bounded failure code. Each missing server starts
one background discovery for the exact catalog generation and invocation
authority. Repeated list calls share that single in-flight operation. An unfinished fetch reports `discovery_pending`; a failed fetch reports
`upstream_unavailable`. A healthy server commits independently and publishes its
matching MCP `listChanged` notification when content changes or a previously
reported gap recovers. An identical refresh does not generate another catalog
change. Discovery caches per-item admission decisions with the caller authority,
policy revision and catalog generation. Native catalog subscriptions invalidate those
decisions on change. Each list request commits one audit record with visible and denied
counts and the visible-set digest before returning. A warm list evaluates no policy.
A failed fetch becomes eligible on the next explicit
list call.

Prompt discovery contacts only registered servers that declare prompts. A server that fails prompt discovery is left out and named in
the same degradation metadata under the `prompts` surface.

A profile whose work requires a complete tool catalog sets
`discovery_failure_mode` to `fail_closed`; its tool and prompt lists fail until
every registered server is reachable, which prevents an autonomous client from
retaining a silently incomplete toolset. Upstream `listChanged` notifications invalidate
successful per-server entries and wake callers without polling. Direct resource
reads and tool calls remain fail closed.

A client that needs a complete surface checks the degradation metadata for the
servers it uses. Policy-filtered absence in a complete surface remains authoritative.

Request-scoped subscription delivery invalidates the matching discovery cache before
forwarding its list-change notification. RMCP routes these notifications separately
from ordinary client callbacks; both delivery paths enforce the same invalidation.
Discovery fetches carry an exact claim. Invalidation retires both cached data and
pending claims, preventing late results from reinstalling old contents or settling a
replacement fetch. A disconnected catalog subscription invalidates its cache, and
recovery establishes a new subscription before fetching the catalog again.

## Schemas And Types

Tool inputs publish JSON Schema 2020-12 generated by the ordinary rmcp/Schemars
path in Rust and the official SDK/Pydantic path in Python. Same-document `$ref`
references and composition are allowed, including recursive owner types. Dynamic
references, external references and nested `$id` resources are outside the supported profile. The shared
checker limits document depth, node count, reference count, branch count, serialized
size and work spent following references.

Some providers turn a model's tool call into JSON arguments by decoding each argument
as its declared `type`. Such a parser delivers an argument without a `type` as a JSON
string and cannot read a `type` that names several types. Schemars writes a reference
to a shared definition, and an enum as a `oneOf` of its variants, without a `type`
beside it, and an `Option` argument admits `null` beside its value's type.
`Hosted` publishes every tool input through `hosting::publish_input_schema`. An
optional argument declares its value's type and admits no `null`. A caller omits it
instead, and the server still decodes an explicit `null`. Every subschema whose
references, composition, `const` or `enum` restrict its JSON types then declares them,
as a set that contains every type the subschema admits. A required argument that admits
several types keeps them, and the shared checker rejects it, as it rejects a restricted
subschema without `type`. An unrestricted argument, such as an opaque payload, declares
no `type`.

Strong types govern every controlled shape: typed structs, enums, and explicit
domain types wherever the shape is known or owned by this contract. Raw JSON
is reserved for genuinely open-ended boundaries.

Controlled tool arguments reject undeclared fields at the root and in nested
objects. Generated schemas express the same closure, including every object variant
of a union. Dynamic dictionaries retain their typed values; provider inputs and
user-authored property or schema payloads retain their declared open shapes. Schema
annotations such as examples and defaults are instance data, not additional schemas.
The pinned RMCP router reports argument decoding failures as completed tool results
with `isError: true`. The handler does not run for a rejected input. The decoding's
message names the Rust type that refused the input, so `Hosted` appends the violations
of the tool's published input schema to such a result: at most eight, each by the
argument's JSON Pointer and the type or shape the schema expects, without the caller's
values. Authorization, the Task path and the decoding itself keep their order and
answers.

Content digests establish integrity and provenance. They do not become a
parallel public address.

## Runtime Boundary

Shared telemetry exports OTLP/HTTP protobuf logs and traces only when an endpoint
is configured. Its batch processors use OS threads, so the HTTP exporter selects
the blocking client explicitly. Cargo feature unification must not switch that
client to one requiring an ambient Tokio reactor. Standard per-signal and global
OTLP timeouts remain in milliseconds, with the upstream ten-second default.

Each request's span records its method, protocol version and `http.route`, the route
template the request matched, such as `/artifact/shares/{token}`, or `unmatched`. The
request's path and query never enter spans, logs or OpenTelemetry attributes, because a
path segment or a query parameter can be a secret, such as a share link's token.

A hosted server owns its domain models and declared schemas and consumes the
shared mechanics of `veoveo-hub-sdk` rather than reimplementing them: the
task runtime adapter, resource subscriptions, URI conventions, Work Context
propagation, and the forwarded identity.

- Durable operations run on the shared task runtime and the final task
  extension.
- A server reaches another server only through the Hub, acting for the
  forwarded caller, and holds no credential of another server.
- Administrative HTTP, when a server has it, is served only under the
  server's canonical mount and reached through the hub admin route.
- A server has no private control database. Durable state lives in the
  platform stores.
- A server has no private byte route.
- Every Rust Streamable HTTP endpoint applies the shared terminal-response
  middleware after final JSON serialization. A response through 8 MiB is
  delivered unchanged. A larger response is discarded in full and replaced
  by JSON-RPC error `-31001` with diagnostic code
  `response_budget_exceeded`, `maximumBytes`, and `actualBytes` when the
  completed byte count is available. A body collection failure uses
  `response_serialization_failed`. Neither diagnostic contains a partial
  result or an internal error detail.

## Hosting A Server

A Rust server is hosted through `veoveo_hub_sdk::hosting` and writes no
`impl ServerHandler`, router, authentication middleware, or host check. It
supplies three things:

1. **Provider identity and setup.** Implement `McpServerContract` with its
   `Provider`, `Server`, scope vocabulary, scheme, documents and native server
   metadata. The provider is the same type named by its consumer requests and
   resource reads. Force `McpServerSetup` before Store or engine initialization.
   Setup derives resources, templates, host document capabilities and route
   authorization from the ready routes. It retains owner task/subscription and
   extension metadata. Route initialization uses direct contract types and never
   calls `setup()` recursively.
2. **Ready typed routes.** Implement `DomainServer::routes` and `setup`; optional
   `complete` handles non-document completion. `Routes::<MyDomain>::builder` starts
   with an explicit document access policy. `.tool<R>` accepts that provider's
   `ToolCall`, a policy and a handler returning its associated `SingleOutput`.
   `.task<R>` additionally requires `TaskToolCall`; `.collection<T>` admits only
   the generated `List<T>` selection and the collection's immutable `Page<C>`.
   `Checked<T>` forwards the call association while its decoder admits cross-field
   invariants. No arbitrary serialized value is a single-output declaration.
   Register each singleton/member/collection reader through `ResourceBuilder<R>`
   after supplying its typed reader, access policy and cache policy. The reader
   returns `ResourceRead::Content`; its own associated address parser stays bound
   through dispatch. `.collection_resource` shares `CollectionRequest` selection
   parsing and the paging-template constructor with list tools. A fixed collection
   root is optional for nested paths. `.prompt` registers one native descriptor,
   admitted name, explicit policy and render closure. Discovery and registration
   derive from these same ready entries; there are no owner catalog-list hooks or
   second descriptor/permission tables.
3. **Optional task support.** A server with durable tasks wraps its
   `DurableTaskService` in `veoveo_hub_sdk::durable_tasks::DurableTasks::tasks_only`. A server
   that also publishes resource changes implements `ResourceSubscriptions` and uses
   `DurableTasks::with_resources`. The host parses each subscribed URI into the
   server's address type, and `authorize` checks those typed addresses for the caller
   before delivery starts. A server whose subscriptions combine tasks with live or
   domain event streams implements `DurableListener` and uses
   `DurableTasks::with_listener`. A server without durable tasks that publishes
   resource changes uses `ResourcesOnly`, one whose subscriptions follow its own
   live or remote source implements `SubscriptionListener` and uses `ListenOnly`,
   and one with neither uses the default `NoTasks`.

```rust
let server = HostedServer::for_domain::<MyDomain>()
    .deployment(&public_deployment, args.allow_loopback_hosts)?
    .allowed_hosts(args.allowed_hosts.iter().cloned())
    .internal_trust(registration.access_tokens(), http.clone())?
    .handler(move || {
        Hosted::new(MyDomain::new(state.clone()))
            .with_tasks(DurableTasks::tasks_only(MyTasks::new(state.clone())))
    })
    .build();
server.serve(SocketAddr::from(([0, 0, 0, 0], args.port))).await
```

The builder starts from the domain type, which names its checked setup, and offers
`build` only after the deployment, internal trust and handler are set. `internal_trust`
takes the registration's `veoveo_access_tokens` and the `reqwest::Client` that refreshes
the Hub's keys, the one the server registered with. The handler
must be that domain's `Hosted` value: a hand-written `ServerHandler` or another
domain's handler does not compile. `authenticated_routes`, `admin_routes`,
`public_routes`, `liveness` and `readiness` add server-specific HTTP. A liveness
check covers only failures a restart repairs, such as a dead worker process.
An installation serves the internet from one HTTPS origin, `VEOVEO_PUBLIC_ORIGIN`,
such as `https://veoveo.example.com`, with each server's public routes under its own
path. A server parses it as a `PublicOrigin` and passes it to
`PublicDeployment::with_origin`, so requests that arrive for the origin's host pass
host validation, and builds the public URLs it hands out, such as callback and share
URLs, under the origin.
`internal` replaces `deployment` for a server the hub reaches at an internal
authority under `/{slug}`. `internal_root` serves a cluster-internal server whose
hub upstream and protocol paths are root-relative. Both admit only the internal
authorities they are given. `mcp_request_limit` lowers the transport's
4 MiB request-body limit, and `request_timeout` answers 504 when an authenticated
request has not started its response in time; a subscription stream continues
after its response starts. Admin routes join the
document routes under `{mount}/admin` behind hub authentication, and a server
layers any administrative scope check onto them. Public routes carry no hub authentication, so they serve
only endpoints that verify their own callers, such as a signed provider webhook.
Inside a typed handler, `hub_identity` and `forwarded_bearer` return the verified
caller. Handlers return typed outputs; the internal transport adapter serializes
resource JSON and structured tool results. `Product<T>` supplies canonical
`resultUri` and typed metadata, and the adapter derives its matching resource link
from that same value. `completion` and `rank_completions` build native completion
results; `completion` omits `total` when it cuts candidates at 100 values.

The host gives every server the same behavior:

| Behavior | Host rule |
|---|---|
| Routes | `/livez` and optional `/readyz` at the listener root, whatever the mount; `{mount}/admin/docs/*` and `{mount}/mcp` |
| Authentication | Hub RFC 9068 access token on MCP, admin and authenticated routes, addressed to the server's registered resource URL and naming its slug, with the Hub's keys refreshed as [key refresh](src/registration/DESIGN.md#key-refresh) describes; a rejection is 401 with `WWW-Authenticate: Bearer` |
| Host validation | 400 without a Host authority, 421 for an authority outside the deployment's allowed hosts: the deployment host, the installation's public origin when the deployment names one, and loopback only when asked |
| Discovery | Authenticated `resources/list`, `resources/templates/list`, `tools/list` and `prompts/list` from ready routes filtered by current route policy, sorted, 100 per page, private, five-second cache |
| Well-known reads | `{scheme}://docs` routes and `{scheme}://contract` |
| Completion | `doc_id` on the document template; other references go to the domain |
| Address admission | An unparseable read or subscribed URI is Invalid Params (-32602) before domain code runs |
| Read cache | The explicit `ResourceBuilder` cache policy: one second private, or no reuse; protocol continuation reads are never reused |
| Tools | The registered route policy is checked before Task admission; remaining calls dispatch through their typed request/output adapter |
| Shutdown | SIGTERM or Ctrl-C, cancelling in-flight MCP work |

A new server starts from the
[`veoveo-template-mcp-rust`](https://github.com/VeoveoAI/veoveo-template-mcp-rust)
template repository.

The crate's `testing` feature provides `hosting::testing` for a server's
dev-dependencies. `testing::for_domain` starts the builder on a fixed test deployment
that trusts a published test key. `TestHub` signs hub access tokens for a test
user, carrying the permissions `TestHub::granting` sets, and sends MCP and HTTP
requests through the complete router, so discovery, reads, tools and prompts are
testable without a deployment. `HostedServerBuilder::calls` gives a server's handlers
the [calls to other servers](#calls-to-other-servers) that `ctx.client()` makes.

### Core State Through The SDK

A server reaches core state only through this SDK and core's runtime API: its Tasks
through `TaskRuntime` and `OwnerTaskQuery`, its usage through the usage ledger, its audit
through the audit writer. Its own SQL names only its own schema, never `tasks`,
`kernel`, `usage`, `audit`, `identity`, `hub`, another server's schema or an unknown
one. When the API lacks something a server needs, the feature is added to core or this
SDK for every server. A server keeps its database grants on core's tables, which core's
libraries use on its login, so the rule is a check in its tests, not a database
boundary.

Each server runs the check as an ordinary test in its Docker check:

```rust
#[test]
fn sql_names_only_the_servers_schema() {
    veoveo_hub_sdk::testing::assert_sql_confined(env!("CARGO_MANIFEST_DIR"), "media");
}
```

`check_sql_confined` returns the findings that `assert_sql_confined` reports. The check
reads every `.sql` file under the crate root, which holds the query files and
migrations, the SQL literal of every sqlx query macro, of every `sqlx` query function
and of every executor method called with a string, and the file every `query_file`
macro names. PostgreSQL's parser from libpg_query reads each statement, so comments and
string literals never count. The check reads PL/pgSQL function bodies and `DO` blocks
with libpg_query's PL/pgSQL parser and SQL-language bodies as SQL. That parser compiles
every function in its input, and libpg_query aborts the process on a function without a
string body, such as a SQL-standard `RETURN` body, so the check hands it one PL/pgSQL
`CREATE FUNCTION` at a time, cut at the statement's own bounds. It skips `target` and
every hidden directory, such as the `.sqlx` offline metadata that holds core's own
queries.

A statement passes when every relation, type, function and object it names lies in the
server's schema, in `pg_catalog` or in `pg_temp`, and every `search_path` it sets names
only those. The one core function a server names is `kernel.notify_change`, and only as
the function a trigger executes. The check fails closed: a statement the parser refuses,
SQL built at run time, a dynamic `EXECUTE` and a function in another language each fail
it with the file and line. It reads names as written. An unqualified name resolves
through PostgreSQL's default `search_path`, which names none of core's schemas, and a
statement that sets `search_path` to another schema fails the check.

## Calls To Other Servers

A provider publishes each tool's request and each resource consumers read in its
`contract` feature. `ProviderContract` names its slug once, and each request type
implements `ToolCall`: the tool's name on its server and its structured result type. A
tool that runs as a Task also implements `TaskToolCall`. Each read type implements
`ResourceRead`: the provider's typed resource address, which builds the URI, and the
content type it decodes from the resource's one `application/json` text content. A
consumer depends on the provider's `contract` feature by Git revision and declares the
permissions it uses with the ready route builder's `.uses`, passing the provider's
typed permission name, for example `.uses(ArtifactPermission::ArtifactsRead.name())`.

Calls are builders that run when awaited:

```rust
// In a tool handler: for this request's caller.
let client = ctx.client()?;
let inspection = client.call(Inspect {}).await?;
let tally = client.start(Tally { entries: 3 }).await?;
let tallied = tally.timeout(Duration::from_secs(60)).await?;

// In a Task worker: for the caller that created the Task, while it runs.
let client = calls.for_task(&snapshot)?;
client.call(Inspect {}).await?;

// Reads: once, until the content reaches a state, or each time it changes.
let entry = client.read(ReadEntry { entry: name.clone() }).await?;
let closed = client
    .read(ReadEntry { entry: name.clone() })
    .until(|entry| entry.state == EntryState::Closed)
    .timeout(Duration::from_secs(60))
    .await?;
let mut updates = client.read(ReadEntry { entry: name }).updates();
while let Some(entry) = updates.next().await { /* ... */ }
```

`HubClient::call` resolves with the tool's structured result and, for a tool that
started a Task, waits for that Task. `HubClient::start` resolves with a
`TaskHandle` once the Task has started; the handle resolves when the Task completes,
from the Task status subscription of the rmcp fork, which first delivers the Task's
current status. `TaskHandle::state` reads it once, `cancel` requests cancellation, and
`HubClient::task` restores a handle from a recorded Task identifier. `Call::timeout`
and `TaskHandle::timeout` bound the wait.

`HubClient::read` resolves with the resource's content. `Read::until` subscribes to
the resource through `subscriptions/listen` before it reads, reads again on each
`notifications/resources/updated` for the resource, and resolves with the first content
that satisfies its predicate, so it waits without polling. `Read::updates` yields the
content when the subscription opens and each time a read after an update finds it
changed. An ended subscription is opened again after cancellable exponential backoff
from 100 milliseconds to five seconds, each time with a fresh read. Backoff resets
only after five seconds observed awaiting notifications on that acknowledged stream;
acknowledgments and snapshot reads alone do not reset it. The caller's existing
timeout or dropping the observation bounds recovery; an untimed observation has no
implicit lifetime limit. Task-result observations use the same policy and reconcile
the current status on each new subscription, without resubmitting the tool call.
Protocol, admission, acknowledgment, read and decode errors remain terminal. A
subscription the Hub does not acknowledge for the resource fails
with `Unsubscribed`. The provider must advertise resource subscriptions and announce
each change to the resource.

`CallError` separates the outcomes. `Refused` carries the token endpoint's OAuth error
and description, such as `invalid_target` naming a permission no registered server
provides. `Undeclared` names a provider the server declared no use of, refused before
any request. `Rejected` carries a JSON-RPC error from the Hub or the provider,
`Tool` a tool's error result, `Task` a failed or cancelled Task, `Content` a read
without the one JSON text content, and `Decode`, `Timeout` and `Transport` the remaining
failures. A request binding lasts as long as the
forwarded token of that request, at most 60 seconds, so a wait longer than that belongs
in a Task of the calling server.

A program that calls a profile in its own name builds `HubClient::new` with a
`StaticBearer` for a token it already holds, or with a `ClientCredentialsBearer` from
the `client-credentials` feature. That bearer reads the profile endpoint's RFC 9728
metadata for its authorization server and that server's RFC 8414 metadata for the
token endpoint, and requests a new token once a third of the current one's lifetime
remains. Such a client also calls without a provider contract:
`HubClient::call_tool` calls a tool by its Hub name with JSON arguments, waits
for its Task like a typed call and resolves with the tool result, error results
included. `read_resource` reads an address, and `tools`, `resources`,
`resource_templates` and `prompts` list the profile's complete catalog through
`client::catalog`, which follows at most 1,024 pages and 16,384 items within 30 seconds
and reports the discovery failures the Hub attached to any page. These profile-wide
requests carry the caller's own bearer, which a server's calls for its caller do not
have. `hosting::testing::TestMesh` runs several hosted servers in process
behind a loopback token endpoint and profile, with the Hub's exchange and
narrowing, so a server tests its calls to another without a Hub.

## Task Work

`durable_tasks::TaskWorker` claims a server's Tasks and runs a handler for each with a
`TaskContext`, the `ctx` of that Task's work. The worker carries the server's calls to
other servers, the callback base of its operations and the usage ledger:

```rust
let worker = TaskWorker::new(runtime.clone())
    .calls(calls)
    .callbacks(CallbackUrls::new(public_base, slug)?)
    .usage(UsageLedger::new(&runtime));
worker.run(|ctx| async move { cast(ctx).await }, stop).await?;

async fn cast(ctx: TaskContext) -> anyhow::Result<TaskTransition> {
    let spec = OperationSpec::events(OperationKey::parse("cast")?, ProviderName::parse("kiln")?, timeout);
    ctx.usage()?.estimate(Usage::Casts, Quantity::whole(1)).money(estimate).operation(&spec.key).await?;
    ctx.operation(&spec)
        .dispatch(|dispatch| async move {
            let job = match dispatch.dispatch {
                StepDispatch::First => provider.submit(dispatch.operation_id, dispatch.callback_url).await?,
                StepDispatch::Unsettled { .. } => provider.find(dispatch.operation_id).await?,
            };
            Ok(Submission::job(job))
        })
        .await?;
    match ctx.wait::<ProviderOutcome, _>(&spec.key, &provider).await? {
        WaitOutcome::Settled(outcome) => { /* record actual usage, complete */ }
        WaitOutcome::Expired => { /* the outcome is unknown, never a failure by itself */ }
        WaitOutcome::Cancelled { .. } => Ok(TaskTransition::Cancelled),
    }
}
```

`ctx.step` runs a durable step: a finished step returns its stored result, and a step
an earlier run dispatched without a result receives an unsettled attempt with the same
operation identity. `ctx.operation(..).dispatch` records an operation and runs its
closure once, with the operation identity as the provider's idempotency key and, on the
first attempt, the callback URL `{base}/{slug}/operations/{operation_id}/{token}` under
the worker's callback base; an unsettled attempt reconciles with the provider and never
resubmits blindly. `ctx.wait` returns the outcome of a settled operation, runs a due
status check, carries a cancellation to the provider through the server's
`OperationCalls`, or parks the Task. A parked wait fails with `TaskError::Parked`, the
handler propagates it with `?`, and the worker releases the Task without settling it: a
waiting Task holds no worker and survives restarts. A provider event, a due check or the
deadline queues the Task, and the handler runs again from its start, so its finished
steps and its dispatched operation return what they recorded and it resumes at its wait.
A Task whose cancellation was requested while it was parked runs to its wait, which
cancels the operation and returns the provider's receipt.

The worker settles the transition a handler returns, a `resume` Task through resumable
settlement. By default a pending cancellation turns a `resume` Task's success into
`Cancelled` and keeps a genuine failure. `TaskWorker::cancellation(task_type, policy)`
names another policy for a kind: `CancellationWins` for one whose outcome a cancellation
always replaces, `PreserveOutcome` for one whose finished result a later cancellation
must not undo, such as a verification that completed. Under `PreserveOutcome` a success
or a genuine failure stands when the request arrives after the handler finished. The
worker withholds a success only while its process stops; a cancellation reaches the
outcome through the Task's state, where the policy decides what it keeps. A
handler error that is a parked Task releases it, one that reports the Task lost to
another instance leaves it alone, a requested cancellation settles the Task as
`Cancelled`, and any other error fails it with `task_error`. An error that a lost or
unavailable database connection caused says nothing about the Task: the worker releases
the Task to its recovery, which queues a `resume` Task to run again from its finished
steps and ends any other as interrupted. `TaskWorkError` reads these from `TaskError`,
`TaskStop` and `anyhow::Error`, whose whole chain it searches for a connection error of
the runtime, the store or the driver. An outcome that cannot be recorded while the
connection is down is tried again for up to 60 seconds, waiting 100 milliseconds and
doubling to at most 5 seconds, before the Task goes to its recovery the same way.
`ctx.update` records progress through the same settlement.

A worker keeps running while the database connection is down. Its instance
[reconnects](https://github.com/VeoveoAI/veoveo-core/blob/9a6b3ff95469ff19f9ccb513421a57ba6751aa9e/docs/DATABASE.md#process-instances)
and keeps the Tasks it owns, or continues as a new instance once a reaper moved them to
recovery, and its work stream claims again once the connection is back. `run` returns
an error only when the instance ended, as it does once the database refuses the
server's login after the Hub removed the registration.

A process stops its worker with the token `run` takes. The worker claims nothing more
and waits up to its drain time, 30 seconds by default, for its running Tasks. The stop
reaches each handler through `ctx.cancelled()`, which also resolves when the Task's
cancellation reaches this worker, and through `ctx.checkpoint()`, which fails with
`TaskStop::Stopping` while the process stops and with `TaskStop::Cancelled` when the
Task's cancellation was requested on any replica. A stopping process publishes no
outcome of a `resume` Task, neither a success nor a failure, unless its cancellation is
pending: the Task stays with its instance, whose recovery queues it when the process
ends, and another worker runs it to its outcome. A handler of another recovery class
that finishes during the drain settles its outcome, and one that stops with an error
leaves its Task to that class's recovery. A parked Task holds no worker, so stopping
leaves it parked. A Task that dispatched an operation reaches `ctx.wait` before any
checkpoint, so the wait carries a cancellation to the provider.
`ctx.client()` calls other servers for the Task's caller, and `ctx.audit_request()`
names the Task in the records its work writes. A Task the store reports waiting is
`working` over MCP unless it holds outstanding input requests.

`hosting::operations::operation_callbacks` serves the callback route on a server's
public routes, and `provider_events` serves `POST /providers/{provider}/events` for a
provider that sends every event to one fixed URL and names its own job. Both hand the
request to the server's `ProviderEvents` parser, which checks the provider's signature
and reads the event's identifier, payload and finality, and then store the event through
the Task runtime. A stored event answers 204 whether it was new, a repeat or late. An
unknown operation, a malformed path and a wrong token answer the same 404, a refused
signature 401, an unreadable event 400 and an unavailable database 503. A body holds at
most 1 MiB, and the request span records the route template, never the token.

A server declares its usage kinds from a closed enum that implements `usage::UsageKinds`
through `UsageLedger::declare`, each under its slug with its FOCUS service category and
unit. `ctx.usage()` records an estimate before billed work, which is the work's
reservation, and the actual usage the provider reports. Each record is for the Task's
caller and names the Task and its operation. Its source identifier defaults to
`task:<task>:<operation key>` and its charge period to the Task's creation, so a
replayed run writes an identical record, which the ledger accepts as a repeat. A
completed Task's result and every `tasks/get` answer carry the Task's cost under
`ai.veoveo/usage`: per kind and currency, the actual records when a kind has any and its
estimates otherwise.

## Deployment Identity

Ordinary hosted MCP endpoints and the hub run behind load-balanced replicas.
Durable Tasks, explicit handles, and shared event sources preserve correctness
across replica changes. A workload remains singleton only when it owns real
exclusive state or hardware, such as DuckDB or a GPU simulation runtime; protocol
session locality is never a reason for singleton deployment.

## Packaging And Registration

A server ships as an OCI image and registers itself with the Hub at startup
through the [server registration protocol](src/registration/DESIGN.md), an
RFC 7591 and RFC 7592 profile. Its client metadata carries two declarations:
`ai.veoveo/registration` states the slug, URI scheme, version and endpoint, and
`ai.veoveo/authorization` lists the server's permissions and the permission every
tool, resource template and prompt requires. The registration response grants the
server a PostgreSQL login for its own schema, where the server applies its own
migrations. The server holds a kernel instance on that login while it serves and
removes the registration on shutdown; the Hub removes the registration of a process
that ended without removing it.

The installation configuration names no server. Its `authorization` section declares
roles, grants and overrides. A role selects declared permissions by access kind and
risk, a grant gives a role to a group or principal, optionally within one tenant, and
an override adds risks to a named permission or denies it. Its `registration` section
names the Hub-owned registration secret reference. A configuration without roles,
grants or registered servers validates.

```json
{
  "authorization": {
    "roles": [
      { "name": "member", "access": ["read", "write"] },
      { "name": "admin", "access": ["read", "write"], "risks": ["sensitive", "destructive", "billed", "physical", "administer"] }
    ],
    "grants": [
      { "role": "member", "subject": { "group": "tenant-members" } },
      { "role": "admin", "subject": { "group": "installation-admins" } }
    ],
    "overrides": [{ "kind": "deny", "permission": "time.admin" }]
  },
  "registration": {
    "secret": "registration-secret"
  }
}
```

## Well-Known Surface

Every server is self-describing. Under its canonical URI scheme it serves:

| Resource | Content |
|---|---|
| `{scheme}://docs` | stable page of document entries with `id`, `title`, and concrete `uri`, in `items`; optional `nextCursor` names the last entry and continues through `?cursor=` |
| `{scheme}://docs/{doc_id}` | a document body: at minimum `agents` (the crate `AGENTS.md`) and `design` (the crate `DESIGN.md`) |
| `{scheme}://contract` | typed contract and provider identity; Discover and list methods own the observed runtime surface |

On its administrative mount the server serves the same material as a read-only
HTTP projection at `{mount}/admin/docs/llms.txt` (an index in llms.txt form) and
`{mount}/admin/docs/{doc_id}`. Links in `llms.txt` are relative to that directory:
`agents` and `design`, not paths that repeat `docs/`. The projection requires the
same hub-issued internal identity as the server's MCP endpoint. It is not a
public exception to refused-by-default authentication and does not establish an
alternate domain administration API.

Documents are embedded at build time from the crate, so a running server
serves the manual for exactly the version deployed, including in offline
installations. The `veoveo_hub_sdk::docs` module provides the embedding,
declaration, and rendering machinery; consuming it is the intended way to
comply. `server_docs!` selects the consuming crate's `AGENTS.md` and `DESIGN.md`,
then calls the shared `embedded_document!` proc macro. The document identity is an
admitted `ServerSlug`, and setup compares it with the provider identity. Its
manifest-relative selection expands in the consuming crate. No adjacent compliance
profile or generated per-rule manual section is loaded.

The Console renders these resources generically; the hub generates an
installation llms.txt from the catalog. Neither requires per-server work.

The Rust embedding macro computes document digests during compilation. Each body is
immutable for the running image. Checked server setup requires the `{scheme}://docs`
index, a resource per document, the `{scheme}://docs/{doc_id}` template and the contract
resource. The shared read adapter requires the Hub-authenticated profile before
serving a body, and an independently hosted adapter may call the authorized-read helper
after enforcing its own profile policy. Document indexes contain at most 32 entries per
page. Unknown cursors, duplicate
IDs and unsupported document query parameters fail admission.

## Crate Documents

Documentation lives beside the code it governs, written for agents first and
readable by humans:

- `DESIGN.md` — the server's domain contract, including its standards and
  protocols profile.
- `AGENTS.md` — the agent work manual, delta-only over the repository root
  `AGENTS.md`, with required sections `Purpose`, `Invariants`,
  and `Build And Test`.

A server's crate, binary and image take its repository name, `veoveo-mcp-<name>`.

## Typed Construction

`McpServerContract::Provider` is the same provider used by every registered request
and read. `Routes<D>` accepts only that provider's tool and resource associations.
Single outputs explicitly implement `SingleOutput`; no blanket `Serialize`
implementation admits top-level collections. `CollectionTool` registers `List<T>`
inputs and only `Page<T::Collection>` results. `.task<R>` requires `TaskToolCall` and
derives the native tasks capability extension. Task-specific authorization and
lifecycle behavior remain in `TaskSupport`.

`ResourceBuilder<R>` must receive its typed reader, explicit access policy and cache
policy before registration as a singleton, member template or collection. Each read
keeps its own `ResourceRead::Address` parser through dispatch. Schemas, descriptors
and the existing authorization requirements maps derive from the ready routes.
`RouteAccess::Authenticated` means a verified caller without an additional
permission; it never admits an anonymous request. Missing entries remain undeclared.
Every route checks current authorization, including before Task start. Catalog
resources and templates use the same route policies. Native prompts register a
descriptor, admitted name, explicit policy and render closure in the same routes;
registration, prompt discovery and rendering share that entry.

`Product<T>` owns a checked result URI and typed metadata. The transport adapter
produces structured `resultUri` and a matching resource link together. Handlers
return typed outputs rather than raw MCP results. `tool`, `collection_tool`,
`resource_read` and `collection_resource` attributes generate ordinary public traits;
contract-only consumers need no rmcp runtime. Type arguments are Rust types.

The requirement catalog below remains architectural documentation. The previous
conformance runner, certification commands, evidence/coverage formats and per-rule
compliance profiles are removed.

## Architectural Requirements

<!-- veoveo:requirement-catalog:start -->
| ID | Level | Requirement |
|---|---|---|
| C01 | MUST | Each capability uses the canonical MCP surface for its need per the Protocol Surface table. |
| C02 | MUST | Every tool declares input and output JSON Schemas; an addressable terminal product has one top-level canonical `resultUri`, while a no-product task omits it. |
| C03 | MUST | Durable operations are task-augmented tools on the shared task runtime. |
| C04 | MUST | Addressable state is exposed as resources or resource templates under the server's canonical scheme. Growing resource collections and every collection-returning tool, including bounded or fixed listings, use the collection request/page contract: bounded pages, opaque scoped continuation, deterministic complete traversal on unchanged data, current authorization and explicit mutation semantics. Embedded arrays and batch action results remain parts of single output records. Exact reads do not scan the full collection. |
| C05 | MUST | The server is not flattened to a tool-only convenience surface. |
| C06 | MUST | Compatibility helpers are additive projections reusing canonical models, policy, audit, tasks, and URIs. |
| C07 | MUST | Tool input schemas use the supported JSON Schema 2020-12 profile, close controlled argument objects, give each typed argument one JSON `type` with optional arguments omitted rather than `null`, declare `type` on every subschema whose references, composition, `const` or `enum` restrict its JSON types, and pass the shared document and reference-expansion bounds. |
| C08 | MUST | Schemas are generated through ordinary rmcp/Schemars or official SDK/Pydantic machinery. |
| C09 | MUST | Controlled shapes use strong domain types; raw JSON only at open boundaries. |
| C10 | MUST | Shared mechanics, including the final 8 MiB serialized JSON response cap, come from `veoveo_hub_sdk`, not reimplementation. |
| C11 | MUST | A server reaches another server only through the Hub, acting for the forwarded caller, and holds no credential of another server. |
| C12 | MUST | Administrative HTTP exists only under the canonical mount. |
| C13 | MUST | No private control database. |
| C14 | MUST | No private byte route. |
| C15 | MUST | The server ships as an OCI image with a versioned Helm chart. |
| C16 | MUST | The server registers with the Hub through `ai.veoveo/registration` and declares the permission every tool, resource template and prompt requires through `ai.veoveo/authorization`. |
| C17 | MUST | The registration and crate documents name the contract the server implements. |
| C18 | MUST | Docs resources are served under `{scheme}://docs`. |
| C19 | MUST | The contract declaration resource is served at `{scheme}://contract`. |
| C20 | MUST | The admin mount serves `docs/llms.txt` and document bodies. |
| C21 | MUST | Served documents are embedded at build time from the crate. |
| C22 | MUST | `DESIGN.md` exists beside the crate and pins the domain profile. |
| C23 | MUST | `AGENTS.md` exists beside the crate with the required sections. |
| C24 | MUST | The crate, binary and image take the repository name, `veoveo-mcp-<name>`. |
| C25 | MUST | Every network endpoint uses Streamable HTTP; stdio exists only inside a local bridge that owns its child. |
| C26 | MUST | Streamable HTTP is stateless, requires final per-request protocol metadata, returns JSON terminal responses, and rejects session, reconnect, DELETE, and replay surfaces. |
| C27 | MUST | `subscriptions/listen` uses an authorized accepted filter, a request-scoped sink, bounded backpressure, and a shared restart-safe event source; legacy subscribe and unsubscribe are absent. |
| C28 | MUST | Tool, prompt, and resource list-change capabilities are declared independently and match emitted notifications. |
| C29 | MUST | Ordinary hosted servers and the hub tolerate load-balanced replica changes; singleton workloads name the real exclusive state or GPU owner. |
| C30 | MUST | Requests with equivalent upstream transport security share one catalog-revision-scoped HTTP connection pool and TLS trust store without sharing request authority. |
| C31 | MUST | Readiness calls Discover and required list methods, compares the observed surface with installation allow/require policy, and fails closed on mismatch. |
| C33 | MUST | Discovery identifiers, controlled DTO fields and values, and adopted extension metadata derive from typed naming declarations; narrow owner, JWT, frozen and external declarations require source review. |
<!-- veoveo:requirement-catalog:end -->

## Implementation Boundaries

Construction supplies typed request/output associations, required route policies,
shared descriptors, schemas and immutable bounded pages. Ordinary component and
integration tests exercise current authorization, indexed queries, mutation,
response byte/time limits, forwarded-token rejection, readiness and transport.
Dynamic properties are not certified by marker traits or owner declarations.
Core state remains behind the SDK/runtime APIs; each server keeps
`testing::assert_sql_confined` in its tests.

Installation roles and server route policies own exposure. Discover and list
methods own runtime observations. Readiness compares those observations with its
installation selection without copying them into the contract declaration.

### Resource source recovery

`SubscriptionHub` carries either an exact changed URI or a reconciliation signal.
A listener projects reconciliation only onto its accepted resource identities.
The shared resource listener sends initial invalidations after registering its
receivers. Its initial list invalidation allows a catalog consumer to wait for
observation setup before listing. Task-runtime resource fan-out also sends initial
resource and requested catalog invalidations. The SDK filter acknowledgement alone
precedes asynchronous source setup.
Content-only reconciliation does not emit a resource-list change. Servers emit list
changes when discovery membership or descriptor metadata changes, independently of
updates to an existing resource's contents.
Broadcast lag requests reconciliation instead of silently discarding the gap.
Resource-list lag likewise emits an invalidation. These notifications contain no
resource content and never grant read authority. A server shares its database change
sources across its local listeners. Source reconnection invalidates
accepted identities after a delivery gap. Idle sources do not emit timer-generated
resource changes: declared resource subscriptions can wake an agent. Client-side
catalog reconciliation reads authoritative state without invoking a model.

Task-backed resource subscriptions share the Task runtime's current-owner watch.
On a new database listener connection, the runtime rereads admitted identities under SQL
visibility predicates. That baseline restores resource invalidations after expired
event history without forwarding denied Task payloads or emitting idle timer changes.

## Cursor Admission

The low-level offset helper keeps its published optional String cursor fields and
`v1:` numeric-offset spelling. Its owner codec retains usize parser aliases and
existing error behavior. It binds no listing, filters or authority and retains no
snapshot; callers must maintain stable membership and order across requests and
apply their own bounds and authorization. It is not by itself an implementation
of the Domain Collection Paging contract for mutable collections.

## Forwarded Identity Types

`Principal`, `GroupMembership`, `AccessTokenSubject` and the request context reject
unknown fields during decoding. Raw external JWT claims keep their own admission
profile. `Principal::group_memberships` resolves each group the caller belongs to with
its explicit role, or read when the token names none.
