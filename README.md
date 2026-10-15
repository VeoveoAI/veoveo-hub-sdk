# veoveo-sdk-rust

The Rust SDK for Veoveo MCP servers and clients. A server built on it registers itself
with the Gateway, verifies the identity the Gateway forwards with each request, checks
permissions from one requirement table, serves its documents and contract declaration,
runs durable Tasks that wait on outside providers without holding a worker, records
their usage and publishes resource changes. Its MCP surface is written once in
typed Rust. The SDK builds on [veoveo-core](https://github.com/VeoveoAI/veoveo-core),
which holds the protocol-independent platform, and on rmcp, the official MCP Rust SDK.

## Crates

| Crate | Purpose |
|---|---|
| `veoveo-sdk-rust` | The library: the [hosted MCP server contract](crates/veoveo-sdk-rust/DESIGN.md), hosting, forwarded identity, [registration](crates/veoveo-sdk-rust/src/registration/DESIGN.md), [calls to other servers](crates/veoveo-sdk-rust/DESIGN.md#calls-to-other-servers), [MCP Apps](crates/veoveo-sdk-rust/src/apps/DESIGN.md), the MCP Tasks adapter, [Task work](crates/veoveo-sdk-rust/DESIGN.md#task-work) with steps, operations and usage, and the MCP audit vocabulary |
| `veoveo-sdk-rust-conformance` | The [conformance suite](crates/veoveo-sdk-rust-conformance/DESIGN.md) and its `conformance` and `certify` binaries, which certify a running server over MCP |
| `veoveo-sdk-rust-fixture` | The [Observatory test server](crates/veoveo-sdk-rust-fixture/DESIGN.md), Ledger and Relay for calls between servers, Foundry and its fake provider Kiln for operations and usage, and the tool-input fixtures servers use in their tests |
| `veoveo-sdk-rust-cli` | The [Gateway client](crates/veoveo-sdk-rust-cli/DESIGN.md) `veoveo-sdk-rust-cli`, which lists, reads and calls what one installation caller sees, lets a chat model work on a request with those tools, and runs use-case tests against a running installation |

## Features

| Feature of `veoveo-sdk-rust` | Gives | Pulls in |
|---|---|---|
| `contract` | Declarations and wire types a server shares with its consumers: identity, registration and authorization declarations, typed tool calls and resource reads, the token exchange, Gateway metadata, document and compliance declarations, MCP audit details, usage kinds and Task cost | veoveo-types, the audit envelope, the usage ledger's types, Serde and Schemars |
| `client` | Typed calls, Task handles and typed resource reads and waits through a Gateway profile | rmcp's client and Streamable HTTP client transport, tokio and reqwest; no rmcp server or macros |
| `server` (default) | Everything a hosted server runs, including `ctx.client()` calls to other servers for the current caller, and `TaskWorker` with `ctx.step`, `ctx.operation`, `ctx.wait` and `ctx.usage` | rmcp, tokio, axum, reqwest, core's Task runtime and usage ledger |
| `testing` | `server` plus an in-process Gateway and a mesh of in-process servers for server tests | tower |

A server's own `contract` feature depends on this crate's `contract` feature, so a
consumer of the server's types never compiles the runtime.

## Use

Depend on the SDK by Git revision, the same way components depend on veoveo-core:

```toml
[dependencies]
veoveo-sdk-rust = { git = "https://github.com/VeoveoAI/veoveo-sdk-rust", rev = "<revision>" }

[features]
contract = ["veoveo-sdk-rust/contract"]
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
docker build --target dev -t veoveo-sdk-rust:dev .
docker run -d --name vv-sdk-rust --cap-drop ALL veoveo-sdk-rust:dev sleep infinity
```

The environment runs as its non-root user with every capability dropped and holds no
GitHub credential.

## License

Apache-2.0. See [LICENSE](LICENSE).
