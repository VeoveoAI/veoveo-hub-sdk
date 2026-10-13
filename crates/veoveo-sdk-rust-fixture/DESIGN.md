# Observatory Fixture

`veoveo-sdk-rust-fixture` is an independent hosted server, Observatory, that the Gateway
registers as its test peer and that MCP conformance certifies. It also carries the
pure tool-input fixtures that hosted servers use in their argument-schema tests.

## Standards And Protocols

The fixture implements the resource portion of MCP `2026-07-28` over stateless
Streamable HTTP. JSON carries reading values and embedded contract documents.
The foundational concrete URI profile defines `observatory://` parsing and component
construction. Its RFC 6570 declaration has a single path variable and passes the
foundational template parser before its MCP descriptor is constructed.

## Ownership And Qualification

The `contract` feature owns `ReadingId`, `ObservatoryScope`, and `ObservatoryResource`.
It depends on foundational values and Serde/Schemars. The `server` feature adds the
protocol handler and its `McpServerContract` associations. Neither veoveo-core nor the
SDK library enumerates this domain. An independent consumer disables default
features and builds only `contract` to qualify that separation.

The hosted fixture serves a fixed reading, document resources, and the contract
declaration. The conformance test owns its loopback listener and ephemeral bearer
mapping, including the `FixtureGrants` injected after authentication. The scope helper
compares those grants with the fixture's read permission. This identity profile is
synthetic and provides no installed authentication qualification.

The fixture has no external services, persistent data, or GPU work. Tests enforce a
60-second deadline and stop the owned listener when they finish or fail. Hosted
conformance checks the advertised resource and template surfaces and their well-known
documents. Domain cases check typed exact reads and rejected grants separately.

The `tool_inputs` module needs only Serde. A server's tests load its own cases and
check required members, defaults, open objects, typed dictionaries and rejected
controls against the server's generated input schemas.
