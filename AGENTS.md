# Agent Instructions

These rules apply to veoveo-sdk-rust.

- Use upstream runtimes and libraries unmodified and adapt through supported
  configuration. The rmcp fork pin in `Cargo.toml` records why it exists and when it
  goes.
- Every name, identifier, format, wire field and protocol element follows a published
  standard, [veoveo-core's conventions](https://github.com/VeoveoAI/veoveo-core/blob/7a6c210ff1f9ee9f5150c0a5d101afeb98d659da/docs/CONVENTIONS.md) or this repository's
  [MCP conventions](docs/CONVENTIONS.md). Extend a standard only through its own
  extension mechanism.
- Prefer strong types: typed structs, enums and domain identifiers over raw strings and
  JSON wherever the shape is known.
- The SDK names no specific server or domain. Server-owned scopes, resources, Tasks,
  audit classes and tests belong in the server's repository.
- The `contract` feature carries declarations and wire types only.
  `tests/contract_boundary.rs` fails when it pulls in rmcp, tokio, axum, reqwest or sqlx;
  gate the dependencies themselves, not only the modules.
- `crates/veoveo-sdk-rust/src/hosting` and `crates/veoveo-sdk-rust/src/durable_tasks` are
  protected: changes to them need the repository owner's review.
- Keep test logic in Rust and tool behavior in each tool's own configuration. Do not add
  a task runner or shell scripts that hold test logic.
- Pin exact versions. When changing a pin, verify the latest stable release at its
  source. veoveo-core is pinned to one revision for every core crate.
- Run `docker build --target check .` on macOS and on Linux before pushing.
- Commit small, coherent changes. Commits carry no AI or tool attribution.
