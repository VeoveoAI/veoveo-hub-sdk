//! What an MCP client observes through the Hub: the `<slug>__<tool>` projection of
//! tool names, discovery degradation metadata and MCP App dependency metadata.
mod app_dependencies;
mod discovery;
mod tool_name;

pub use app_dependencies::*;
pub use discovery::*;
pub use tool_name::*;
