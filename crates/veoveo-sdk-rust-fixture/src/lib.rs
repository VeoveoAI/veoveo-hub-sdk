//! The Observatory test server and the shared tool-input fixtures.
//!
//! Observatory is an independent server vocabulary that the Gateway registers as its
//! test peer and that MCP conformance certifies. The tool-input fixtures drive the
//! argument-schema tests of hosted servers.
#[cfg(feature = "contract")]
pub mod contract;
#[cfg(feature = "server")]
pub mod mcp;
pub mod tool_inputs;
