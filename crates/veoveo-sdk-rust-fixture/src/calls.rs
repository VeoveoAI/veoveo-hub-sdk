//! The contract of Ledger, the provider of the calls fixture, as a consumer such as
//! Relay depends on it: its permission vocabulary and the typed request and result of
//! each tool.
//!
//! Ledger reports the forwarded identity each call reached it with, so a test sees
//! whom a call between servers acts for and which server made it.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use veoveo_sdk_rust::{
    calls::{ProviderContract, TaskToolCall, ToolCall},
    registration::{Access, PermissionDefinition, Risk},
};
use veoveo_types::ServerSlug;

/// Ledger's permissions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, veoveo_types::Vocabulary)]
pub enum LedgerPermission {
    #[vocabulary(rename = "ledger.entries.read")]
    EntriesRead,
    /// Sensitive: the installation's member role does not hold it.
    #[vocabulary(rename = "ledger.seals.read")]
    SealsRead,
}

impl PermissionDefinition for LedgerPermission {
    fn resource(self) -> &'static str {
        match self {
            Self::EntriesRead => "ledger.entry",
            Self::SealsRead => "ledger.seal",
        }
    }
    fn access(self) -> Access {
        Access::Read
    }
    fn risks(self) -> &'static [Risk] {
        match self {
            Self::EntriesRead => &[],
            Self::SealsRead => &[Risk::Sensitive],
        }
    }
}

/// Relay's permissions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, veoveo_types::Vocabulary)]
pub enum RelayPermission {
    #[vocabulary(rename = "relay.calls.make")]
    CallsMake,
}

impl PermissionDefinition for RelayPermission {
    fn resource(self) -> &'static str {
        "relay.call"
    }
    fn access(self) -> Access {
        Access::Read
    }
}

pub struct Ledger;

impl ProviderContract for Ledger {
    fn slug() -> ServerSlug {
        ServerSlug::parse("ledger").expect("the fixture slug is valid")
    }
}

/// `inspect`: the identity the call reached Ledger with.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Inspect {}

/// What Ledger saw of a call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Inspection {
    /// The caller's principal.
    pub caller: String,
    /// The OAuth client the caller reached the Gateway with.
    pub client: String,
    /// The server that called for the caller, when another server called.
    pub acting_server: Option<String>,
    /// The permissions the call carried in Ledger's namespace.
    pub permissions: Vec<String>,
}

impl ToolCall for Inspect {
    type Provider = Ledger;
    type Output = Inspection;
    const TOOL: &'static str = "inspect";
}

/// `read_seal`: a sensitive read.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReadSeal {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Seal {
    pub value: String,
}

impl ToolCall for ReadSeal {
    type Provider = Ledger;
    type Output = Seal;
    const TOOL: &'static str = "read_seal";
}

/// `tally`: a tool that runs as a Task and counts `entries` before it completes.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Tally {
    pub entries: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Tallied {
    pub entries: u32,
    pub inspection: Inspection,
}

impl ToolCall for Tally {
    type Provider = Ledger;
    type Output = Tallied;
    const TOOL: &'static str = "tally";
}

impl TaskToolCall for Tally {}
