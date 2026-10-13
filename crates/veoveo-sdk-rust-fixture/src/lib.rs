//! The Observatory test server, the calls fixture and the shared tool-input fixtures.
//!
//! Observatory is an independent server vocabulary that the Gateway registers as its
//! test peer and that MCP conformance certifies. The calls fixture is two servers:
//! Ledger, a provider whose `calls` contract a consumer depends on, and Relay, which
//! calls Ledger for its own caller. The tool-input fixtures drive the argument-schema
//! tests of hosted servers.
#[cfg(feature = "contract")]
pub mod calls;
#[cfg(feature = "server")]
mod calls_support;
#[cfg(feature = "contract")]
pub mod contract;
#[cfg(feature = "server")]
pub mod ledger;
#[cfg(feature = "server")]
pub mod mcp;
#[cfg(feature = "server")]
pub mod relay;
pub mod tool_inputs;

#[cfg(feature = "server")]
pub use calls_support::{CallsResource, NoScope};
