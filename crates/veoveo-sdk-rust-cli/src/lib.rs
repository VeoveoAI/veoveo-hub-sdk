//! A command-line MCP client for a Veoveo installation's Gateway: it lists and calls
//! what one installation caller sees, and lets a chat model work on a request with
//! those tools. Tests use the same modules against a running installation.

pub mod agent;
pub mod gateway;
pub mod mcp;
pub mod model;
