//! A command-line MCP client for a Veoveo installation's Hub: it lists and calls
//! what one installation caller sees through the SDK's `HubClient`, and lets a chat
//! model work on a request with those tools. A person signs in through their browser
//! and the CLI keeps and renews that grant. Tests use the same modules against a
//! running installation.

pub mod agent;
pub mod grant;
pub mod hub;
pub mod model;
pub mod oauth;
pub mod sign_in;
