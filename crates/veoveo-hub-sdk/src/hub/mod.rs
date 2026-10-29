//! What an MCP client observes through the Hub: the `<slug>__<tool>` projection of
//! tool names and its `ai.veoveo/hub` discovery metadata.
mod discovery;
mod tool_name;

pub use discovery::*;
pub use tool_name::*;
