# MCP Conformance Design

`veoveo-sdk-rust-conformance` certifies a running hosted MCP server against the
[hosted MCP contract](../veoveo-sdk-rust/DESIGN.md) over MCP alone, so it can qualify a
server in any language without linking its implementation.

## Standards And Protocols

| Standard or protocol | Supported profile |
|---|---|
| Model Context Protocol | negotiated Streamable HTTP protocol plus the hosted-server requirements selected by a typed profile |
| JSON-RPC 2.0 | MCP request and response envelopes |
| Owner-generated JSON Schema Draft7 | guarded local definitions/ref and legacy dependencies/tuple subset; published MCP schemas retain their normative 2020-12 obligation |
| JSON Schema 2020-12 | bounded tool input schemas with same-document references and composition, plus generated profile/report schemas |
| OAuth 2.0 protected-resource metadata | unauthenticated Bearer rejection checks selected by the profile |
| OAuth 2.0 authorization code / RFC 7636 | separate public-client CLI login with S256 PKCE and an installation-registered client |
| RFC 8414 / RFC 8707 | SDK authorization-server discovery and explicit resource indication; requested and granted scopes must match |
| RFC 8252 | numeric loopback HTTP redirect on an explicit port; remote issuer and resource require HTTPS |
| `https://veoveo.ai/formats/mcp-conformance-profile` | domain-neutral declaration of applicable hosted-server checks |
| `https://veoveo.ai/formats/mcp-conformance-report` | machine-readable implementation identity, capabilities, requirement results, and evidence |
| `https://veoveo.ai/contracts/hosted-mcp` | Veoveo hosted-server contract for MCP `2026-07-28` |
| `ai.veoveo/gateway-discovery-degradation` | typed server/surface/failure-code catalog metadata; a degraded catalog makes naming inspection incomplete |

## Naming Inspection

C33 inspects published tool input and output schema graphs using the same per-document
structural limits as C07. One inspection permits 32 MiB of schemas and observed
values, 500,000 traversal steps and 4,096 roots within thirty seconds. Local references
use the maintained JSON Schema registry; inspection never retrieves a remote schema.
Recursive data models preserve naming context through memoized visits. A reference
cycle that makes no instance progress fails inspection. Exemptions belong to the
annotated subtree; references and composition branches cannot exempt their siblings.
Dictionary uses validate visible property and dependency names against the captured
key schema, while mapped values keep their own naming context. Definitions are checked
in each actual use context; unused definitions receive an unclassified inspection.
Controlled object constants and dependency keys are inspected as instance fields.
Examples and defaults remain annotations. A focused literal-association module
uses maintained validators with the original document dialect and local registry.
It selects successful alternatives, active conditionals and dependencies, matching
property patterns and element constraints. Maintained evaluation output identifies
which locations receive unevaluated-property or unevaluated-item constraints.
Predicate and content schemas do not grant positive payload exemptions or trigger
string decoding. A constant with no valid schema association is refused.

The inspector admits default or declared JSON Schema 2020-12 and the owner-generated Draft7 subset. Other drafts and nested dialect declarations fail before validator preparation. The Draft7 subset admits guarded definitions/references, schema and
name dependencies, tuple items and additional items under that declared dialect.
Legacy applicators in a 2020-12 document, and modern applicators in a Draft7 document,
require a consistent owner schema profile and are refused before reference preparation.
Both dialects apply the same child reference, nested-identity and naming inspection;
C07 continues to require closure separately.

Owners may supply generated schemas and deliberately safe serialized observations
through an in-memory API for nonadvertised bodies. Reports expose locations and counts,
not observed bodies. Selection, discovery identifiers and metadata, and each observed
node consume the original aggregate budget. One cached validator checks each owner's
observations, with deadline checks before and after validation. The hosted runner
shares its thirty-second discovery deadline across paged surfaces. Typed gateway
degradation on any page makes the complete selection unavailable, including when
other descriptors survive. Source-only declarations establish no remote observation. A missing selected observation
or an empty schema selection returns `Incomplete`. Successful C33 inspection records
`Mixed` coverage and `ReviewRequired`; source declarations cannot settle that review.
Gateway tool names use the typed server/local projection; prompts stay unqualified.
JWT, frozen, external and owner-specific declarations still require source review;
remote inspection validates their structure and the built-in grammar it can establish.

Adopted Apps metadata is decoded and inspected with the actual owning DTO schemas.
This includes capability declarations, tool App links and projected App dependencies. Other vendor metadata keeps
its open vocabulary while consuming the same traversal and byte limits.

## Boundary

This crate certifies a running MCP server through public HTTP and MCP surfaces. The
library accepts a typed profile and credentials supplied out of band, then returns a
typed report. The CLI reads and writes the same JSON contracts.

The hosted runner uses shared MCP protocol and Veoveo contract infrastructure without
a compiled registry of domains. Other protocol utilities can consume server-owned
contract features. Domain lifecycle smoke and database selection tests belong to the
component that owns the domain.

Certification, `info`, `tools` and `apps-check` traverse their catalog pages through
the same typed reader. Each surface permits at most 1,024 pages and 16,384 items within
30 seconds. Repeated cursors fail the read. Required tools, schemas and App links are
checked across the complete traversal, including descriptors on later pages.
`tools` reads and validates the complete tool catalog. It does not request prompts or
resource templates. Domain harnesses use it to require the tools their scenario calls;
`info` reads all three catalogs and continues to require their availability.

The `modular_server` integration test hosts the independent
[Observatory fixture](../veoveo-sdk-rust-fixture/DESIGN.md), which is a
development dependency. The fixture owns its scopes and resource family in a library
with an isolated contract feature. Its MCP feature implements the public setup trait.
The test supplies a profile to this same hosted runner, then checks typed reading
access and scope denial separately. A 60-second deadline and owned loopback listener
bound the test. It uses synthetic credentials and requires no cluster or GPU.

Repository checks read complete checked profiles for the discovered Rust, Python and
Node servers, both server templates, SUMO and independent fixtures. Each generated
manual section agrees with that profile. Hosted certification requires C18–C21 met;
other pending declarations state their qualification gaps.

Tool input schemas retain the ordinary SDK representation. Conformance permits
same-document `$ref` references and composition, and rejects external and dynamic
references without fetching them. Nested `$id` resources are outside this profile;
local references resolve within the root document. Document limits are 1 MiB, depth 64, 50,000 nodes,
4,096 references and 4,096 composition branches. Reference inspection follows schema
keywords; keys inside examples, defaults and other instance values have no reference
semantics. The byte, depth and node limits still cover that instance data.

The checker requires closure on reachable controlled object shapes, including union
branches and referenced definitions. A closed enclosing composition may supply that
closure. Typed dictionaries and opaque payload leaves keep their declared value
schemas. Predicates such as `if`, `not` and `propertyNames` constrain an instance rather
than declare another decoded object. Closure inspection permits at most 50,000 visits
and 64 nested references; cycle handling tracks whether closure came from the enclosing
instance. This prevents an outer closed object from masking an open recursive child.

## Profile

A profile names the expected implementation slug, selected contract, allowed
resource URI schemes, HTTP boundary checks, and required, optional, or forbidden MCP
surfaces. Each profile lists its required tool, resource, template, and prompt
identities; the conformance client has no compiled registry of them. A hosted-server certificate selects
exactly `https://veoveo.ai/contracts/hosted-mcp`: resources are required, and the profile must name
the administrative `llms.txt` URL. Unauthenticated Bearer rejection is required for
the MCP endpoint. C18–C21 cannot be disabled by a profile.

Credentials never enter the profile or report. The CLI receives a bearer through
`VEOVEO_CONFORMANCE_BEARER_TOKEN`, or uses the existing direct-hosted internal assertion arguments
for installation-local acceptance. The same credential authenticates the MCP endpoint
and the administrative docs projection. Profile validation requires both URLs to have
the same scheme, host, and effective port before any credential can be forwarded.
Certification also requires the index and every linked document to return HTTP 401
without that credential.

Direct-hosted assertions carry an automated service request context whose client ID
matches the selected principal subject. The context binds the tenant, Work Context,
scopes and thirty-minute expiry through the shared issuer's validation. Its source
issuer and resource are `https://conformance.veoveo.local`; it has no browser session
family or delegation. This synthetic identity supports direct protocol certification.
Public OAuth and current installation policy require separate installed acceptance.

## Report

Each result carries a stable requirement identifier, status, summary, and bounded
evidence. The report records the negotiated protocol, implementation identity,
advertised capabilities, selected contract, and execution interval. A failed
requirement produces a report and a non-zero CLI exit.

Certification reads the live contract declaration and binds it to the selection and
observation. The declaration's contract must equal the selected contract, and its
server must match both the expected slug and discovered implementation. The complete
declaration must cover the current catalog and mark C18–C21 met. Unknown
IDs, duplicate entries and omissions fail admission. Discover and the MCP lists supply the observed
capabilities. The client follows the relative document links published in `llms.txt`;
it does not synthesize document URLs from parsed identifiers.

## CLI Output

`oauth-login` obtains a token for an installation-registered public client through
browser authorization and S256 PKCE. It runs before MCP connection and does not
require an existing bearer. The issuer, client, resource, registered loopback redirect
and requested scopes are explicit inputs. The SDK owns OAuth discovery and exchange;
the CLI owns callback lifetime and private-file output. Callback shutdown closes
the listener and accepted connections within the original login deadline, reserving
its last two seconds for cleanup.
Dropping the login signals the same server handle to close its accepted connections.
Authorization URLs and tokens
are written only to new owner-private files. Operators open the authorization URL
locally without copying it into reports. The 30–900-second login deadline covers discovery, authorization, exchange and
cleanup.
Success prints only granted scopes, expiry and private-file paths; failures print a
closed phase and status without SDK error details. SDK tracing is suppressed during
the OAuth flow because authorization codes and token responses are private. Typed
OAuth getters admit bearer type, expiry and scope before the access-token secret is
exposed for its final private-file write; the complete token model is never serialized.
The library,
`certify` and ordinary MCP commands continue to receive credentials out of band.

Each CLI command reserves standard output for its requested result. Structured resources
therefore remain parseable even when the server emits notifications while the command is
running. Unsolicited progress, task-status, resource-update, and list-change notifications
are operator diagnostics on standard error.

## Distribution

The `conformance` and `certify` binaries install from this repository with
`cargo install --locked --git https://github.com/VeoveoAI/veoveo-sdk-rust veoveo-sdk-rust-conformance`.
They contain no server implementation.

## Requirement Verification Coverage

`src/requirements.rs` maps the closed catalog IDs to existing VV-MCP checks. The generated `catalog/coverage.json`, checked against the suite by a test, records runtime, review and mixed verification modes. A missing
or skipped required check produces an incomplete outcome; a failed check produces
failure. Review and mixed requirements require review even when their runtime checks
pass. C12 HTTP probes do not prove all private surfaces are authorized; C31 catalog
probes do not prove installation readiness policy. Both require review after their
selected runtime checks pass. Owner declarations do not supply results.

## Declaration And Runtime Selection

The declaration profile contains foundational identities, checked catalog/profile and embedded document models. The positive runtime feature enables hosting, signing, security services and transport mechanics; defaults preserve the ordinary runtime surface. Generic conformance accepts bearer credentials outside serialized profiles and does not issue runtime identities. Owner smoke features select domain utilities and fixture dependencies at their actual component.

## Certification Delivery

The `conformance` and `certify` binaries accept an out-of-band bearer token. Their normal and build dependency graphs contain protocol declarations and shared client mechanics. Domain schemas, installation registries, signing fixtures and provider operations compile with their owning delivery targets. `conformance schemas` writes the two certification schemas; installation schemas belong to the Gateway.
