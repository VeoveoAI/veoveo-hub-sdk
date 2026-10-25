# Agent Instructions

These rules apply to veoveo-hub-sdk.

- The design is in [crates/veoveo-hub-sdk/DESIGN.md](crates/veoveo-hub-sdk/DESIGN.md), the
  [registration protocol](crates/veoveo-hub-sdk/src/registration/DESIGN.md), the other
  crates' and modules' `DESIGN.md` and [docs/CONVENTIONS.md](docs/CONVENTIONS.md). Change
  the design in the same change as the code.
- Use upstream runtimes and libraries unmodified and adapt through supported
  configuration. The rmcp fork pin in `Cargo.toml` records why it exists and when it
  goes.
- Every name, identifier, format, wire field and protocol element follows a published
  standard, [veoveo-core's conventions](https://github.com/VeoveoAI/veoveo-core/blob/0e4f26ab0f15b27509bdf6a3796df2f8041cbfb0/docs/CONVENTIONS.md) or this repository's
  [MCP conventions](docs/CONVENTIONS.md). Extend a standard only through its own
  extension mechanism.
- Prefer strong types: typed structs, enums and domain identifiers over raw strings and
  JSON wherever the shape is known.
- The SDK names no specific server or domain. Server-owned scopes, resources, Tasks,
  audit classes and tests belong in the server's repository.
- The `contract` feature carries declarations and wire types only.
  `tests/contract_boundary.rs` fails when it pulls in rmcp, tokio, axum, reqwest or sqlx;
  gate the dependencies themselves, not only the modules.
- `crates/veoveo-hub-sdk/src/hosting` and `crates/veoveo-hub-sdk/src/durable_tasks` are
  protected: changes to them need the repository owner's review.
- The SDK owns no queries. Tests that need a database run on a private cluster from
  core's store fixture, with PostgreSQL 18.6 from the dev image.
- A server reaches core state only through this SDK and core's runtime API, never in its
  own SQL. Every server runs `testing::assert_sql_confined` in its tests. When a server
  needs something the API lacks, add it to core or the SDK for every server instead of
  querying core's tables.
- Keep test logic in Rust and tool behavior in each tool's own configuration. Do not add
  a task runner or shell scripts that hold test logic.
- Pin exact versions. When changing a pin, verify the latest stable release at its
  source. veoveo-core is pinned to one revision for every core crate.
- Run `docker build --target check .` on macOS and on Linux before pushing.
- Commit small, coherent changes. Commits carry no AI or tool attribution.
