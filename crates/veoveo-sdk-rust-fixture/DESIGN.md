# Observatory Fixture

`veoveo-sdk-rust-fixture` is an independent hosted server, Observatory, that the Gateway
registers as its test peer and that MCP conformance certifies. It also carries Ledger
and Relay for calls between servers, Foundry and its fake provider Kiln for operations
and usage, and the pure tool-input fixtures that hosted servers use in their
argument-schema tests.

## Standards And Protocols

The fixture implements the resource portion of MCP `2026-07-28` over stateless
Streamable HTTP. JSON carries reading values and embedded contract documents. Kiln's
events are JSON bodies signed with HMAC-SHA256 (RFC 2104) in a `x-kiln-signature`
header of lowercase hexadecimal, with the event's identifier in `x-kiln-event`.
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

## Calls Fixture

Ledger and Relay are two more hosted servers for calls between servers. Ledger's
`calls` contract, in the `contract` feature, publishes its permissions, the typed
requests of `inspect`, `read_seal`, `tally`, a tool that runs as a Task kept in memory,
and `open_entry`, and `ReadEntry`, the typed read of `ledger://entries/{entry}`. Each of
the first three tools answers with the forwarded identity it received: the caller, the
OAuth client, the acting server, the acting server's Task and the permissions. An entry
that `open_entry` opens closes after the given delay, and Ledger announces both changes
to the entry's subscribers. Relay declares Ledger's three permissions in `uses` and
calls Ledger through `ctx.client()`, inline and through Ledger's Task, and
`relay_follow_entry` opens an entry and follows its typed updates until it closes.
`relay_whoami` answers with the identity Relay received, so a Ledger Task that calls it
shows the Task as provenance. With core's Task runtime Relay also runs `relay_later` as
a durable Task whose worker calls Ledger for the Task's caller. The SDK's mesh tests
and the Gateway's integration tests use both.

## Operations Fixture

Foundry is a hosted server whose `foundry_cast` runs as a durable Task in core's Task
runtime, and Kiln is the fake provider it casts in, on a loopback port. A cast records
an estimate of its cost, dispatches an operation to Kiln with the operation identity as
Kiln's idempotency key and the callback URL of Foundry's public route, and parks. Kiln
calls the callback URL with an event whose body it signs with HMAC-SHA256 under a shared
secret; Foundry's parser checks the signature and reads Kiln's event identifier and
job. The resumed Task records the billed cost and completes with the piece. Kiln also
finds a job by the operation identity, which an unsettled dispatch uses to reconcile
instead of casting again, and cancels a job with a receipt. The operations tests run
Foundry and Kiln on a private PostgreSQL cluster from core's store fixture, through the
SDK's mesh, and reach no outside service. One of them ends every database session of
Foundry's HTTP process and worker while a cast is parked, then checks that MCP reads
still answer, that Kiln's callback still settles the cast, and that a new cast runs.

The worker tests run `TaskWorker` over Tasks of a test server, `anvil`, on two process
instances of a private PostgreSQL cluster. They stop a worker while its Tasks run and
check that a `resume` Task keeps no outcome and is finished by a worker of the next
instance, that a Task of the `interrupted_indeterminate` class keeps a finished outcome
and otherwise fails as indeterminate, that a cancellation requested on another replica
reaches a handler at its checkpoint, and that each Task kind settles a pending
cancellation by its own policy. They also end a worker's database sessions and check
that it claims and finishes the next Task, that work a lost connection interrupted runs
again as a `resume` Task and ends as interrupted otherwise, and that the worker ends once
its login is removed.

The `tool_inputs` module needs only Serde. A server's tests load its own cases and
check required members, defaults, open objects, typed dictionaries and rejected
controls against the server's generated input schemas.
