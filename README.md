# veoveo-hub-sdk

The Rust SDK for Veoveo MCP servers and clients. A server built on it registers itself
with the Hub, verifies the identity the Hub forwards with each request, checks
permissions from one requirement table, serves its documents and contract declaration,
runs durable Tasks that wait on outside providers without holding a worker, records
their usage and publishes resource changes. Its MCP surface is written once in
typed Rust. The SDK builds on [veoveo-core](https://github.com/VeoveoAI/veoveo-core),
which holds the protocol-independent platform, and on rmcp, the official MCP Rust SDK.

## Crates

| Crate | Purpose |
|---|---|
| `veoveo-hub-sdk` | The library: the [hosted MCP server contract](crates/veoveo-hub-sdk/DESIGN.md), hosting, forwarded identity, [registration](crates/veoveo-hub-sdk/src/registration/DESIGN.md), [calls to other servers](crates/veoveo-hub-sdk/DESIGN.md#calls-to-other-servers), [MCP Apps](crates/veoveo-hub-sdk/src/apps/DESIGN.md), the MCP Tasks adapter, [Task work](crates/veoveo-hub-sdk/DESIGN.md#task-work) with steps, operations and usage, and the MCP audit vocabulary |
| `veoveo-hub-sdk-fixture` | The [Observatory test server](crates/veoveo-hub-sdk-fixture/DESIGN.md), Ledger and Relay for calls between servers, Foundry and its fake provider Kiln for operations and usage, and the tool-input fixtures servers use in their tests |
| `veoveo-hub-sdk-cli` | The [Hub client](crates/veoveo-hub-sdk-cli/DESIGN.md) `veoveo-hub-sdk-cli`, which lists, reads and calls what one installation caller sees, lets a chat model work on a request with those tools, and runs use-case tests against a running installation |

Resources and tools are authored as typed declarations. `ResourceRead` binds an
address to its content, `ToolCall` binds input to a single output, and collection
routes use Core's `Collection`, `Pager` and immutable `Page<C>`. The shared route
builder supplies descriptors, authorization requirements, dispatch and schemas from
those associations. Thin attributes generate ordinary trait implementations.
The previous conformance crate, certification commands and per-rule owner profiles
have been removed. Dynamic authorization, transport and database behavior remain
ordinary implementation tests.

## Features

| Feature of `veoveo-hub-sdk` | Gives | Pulls in |
|---|---|---|
| `contract` | Declarations and wire types a server shares with its consumers: identity, registration and authorization declarations, typed tool calls and resource reads, the token exchange, Hub metadata, document and contract declarations, MCP audit details, usage kinds and Task cost | veoveo-types, the audit envelope, the usage ledger's types, Serde and Schemars |
| `client` | Typed calls, Task handles and typed resource reads and waits through a Hub profile | rmcp's client and Streamable HTTP client transport, tokio and reqwest; no rmcp server or macros |
| `client-credentials` | `client` plus a caller's own client-credentials bearer, discovered from the profile's metadata and requested with an assertion signed by the caller's key | core's OAuth client and its JWT signing |
| `server` (default) | Everything a hosted server runs, including `ctx.client()` calls to other servers for the current caller, and `TaskWorker` with `ctx.step`, `ctx.operation`, `ctx.wait` and `ctx.usage` | rmcp, tokio, axum, reqwest, core's Task runtime and usage ledger |
| `testing` | `server` plus an in-process Hub and a mesh of in-process servers for server tests | tower |

A server's own `contract` feature depends on this crate's `contract` feature, so a
consumer of the server's types never compiles the runtime.

## Use

Depend on the SDK by Git revision, the same way components depend on veoveo-core:

```toml
[dependencies]
veoveo-hub-sdk = { git = "https://github.com/VeoveoAI/veoveo-hub-sdk", rev = "<revision>" }

[features]
contract = ["veoveo-hub-sdk/contract"]
```

A new server starts from
[veoveo-template-mcp-rust](https://github.com/VeoveoAI/veoveo-template-mcp-rust).

## Gate

```sh
docker build --target check .
```

The `check` stage runs rustfmt, clippy with warnings denied, cargo-deny, cargo-hack on
every feature of every crate and the nextest `ci` profile. The operations tests start
private PostgreSQL 18.6 clusters through core's store fixture, from the packages the dev
image installs. It runs the same way on macOS and Linux.

## Development environment

```sh
docker build --target dev -t veoveo-hub-sdk:dev .
docker run -d --name vv-sdk-rust --cap-drop ALL veoveo-hub-sdk:dev sleep infinity
```

The environment runs as its non-root user with every capability dropped and holds no
GitHub credential.

## License

Apache-2.0. See [LICENSE](LICENSE).
