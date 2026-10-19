//! The contract of Ledger, the provider of the calls fixture, as a consumer such as
//! Relay depends on it: its permission vocabulary, the typed request and result of each
//! tool, and the typed read of its entries.
//!
//! Ledger reports the forwarded identity each call reached it with, so a test sees
//! whom a call between servers acts for, which server made it and from which Task. Its
//! entries change state after they open, so a test follows a resource's updates.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use veoveo_hub_sdk::{
    calls::{ProviderContract, TaskToolCall, ToolCall},
    registration::{Access, PermissionDefinition, Risk},
};
use veoveo_types::{
    ResourceAddress, ResourceUri, ResourceUriBuilder, ResourceUriParts, ServerSlug, UriSegment,
};

/// Ledger's permissions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, veoveo_types::Vocabulary)]
pub enum LedgerPermission {
    #[vocabulary(rename = "ledger.entries.read")]
    EntriesRead,
    #[vocabulary(rename = "ledger.entries.write")]
    EntriesWrite,
    /// Sensitive: the installation's member role does not hold it.
    #[vocabulary(rename = "ledger.seals.read")]
    SealsRead,
}

impl PermissionDefinition for LedgerPermission {
    fn resource(self) -> &'static str {
        match self {
            Self::EntriesRead | Self::EntriesWrite => "ledger.entry",
            Self::SealsRead => "ledger.seal",
        }
    }
    fn access(self) -> Access {
        match self {
            Self::EntriesWrite => Access::Write,
            Self::EntriesRead | Self::SealsRead => Access::Read,
        }
    }
    fn risks(self) -> &'static [Risk] {
        match self {
            Self::EntriesRead | Self::EntriesWrite => &[],
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
    /// The OAuth client the caller reached the Hub with.
    pub client: String,
    /// The server that called for the caller, when another server called.
    pub acting_server: Option<String>,
    /// That server's Task, when it called from one.
    #[serde(default)]
    pub acting_task: Option<String>,
    /// The permissions the call carried in the called server's namespace.
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

/// The name of a Ledger entry: 1 to 64 lowercase ASCII letters, digits or hyphens.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(try_from = "String", into = "String")]
pub struct EntryName(String);

/// An invalid entry name or Ledger address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("expected a Ledger entry address `ledger://entries/{{entry}}` with a valid entry name")]
pub struct LedgerAddressError;

impl EntryName {
    pub fn new(value: impl Into<String>) -> Result<Self, LedgerAddressError> {
        let value = value.into();
        if value.is_empty()
            || value.len() > 64
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            return Err(LedgerAddressError);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for EntryName {
    type Error = LedgerAddressError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<EntryName> for String {
    fn from(value: EntryName) -> Self {
        value.0
    }
}

/// The entry addresses Ledger serves: `ledger://entries/{entry}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LedgerAddress {
    Entry(EntryName),
}

/// The URI template of Ledger's entries.
pub const ENTRY_TEMPLATE: &str = "ledger://entries/{entry}";

impl ResourceAddress for LedgerAddress {
    type Error = LedgerAddressError;
    fn parse(uri: &ResourceUri) -> Result<Self, Self::Error> {
        let parts = ResourceUriParts::parse(uri.as_str()).map_err(|_| LedgerAddressError)?;
        if parts.scheme() != "ledger" || parts.authority() != "entries" || parts.has_query() {
            return Err(LedgerAddressError);
        }
        let segments: Vec<_> = parts.path_segments().collect();
        let address = match segments.as_slice() {
            [entry] => Self::Entry(EntryName::new(entry.as_ref())?),
            _ => return Err(LedgerAddressError),
        };
        if address.to_uri()? != *uri {
            return Err(LedgerAddressError);
        }
        Ok(address)
    }
    fn to_uri(&self) -> Result<ResourceUri, Self::Error> {
        let Self::Entry(entry) = self;
        ResourceUriBuilder::new("ledger://entries")
            .map_err(|_| LedgerAddressError)?
            .segment(UriSegment::new(entry.as_str()).map_err(|_| LedgerAddressError)?)
            .build()
            .map_err(|_| LedgerAddressError)
    }
}

/// Whether an entry is still open.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum EntryState {
    Open,
    Closed,
}

/// The content of `ledger://entries/{entry}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub entry: EntryName,
    pub state: EntryState,
}

/// `open_entry`: opens an entry, which closes `close_after_ms` later.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct OpenEntry {
    pub entry: EntryName,
    pub close_after_ms: u64,
}

impl ToolCall for OpenEntry {
    type Provider = Ledger;
    type Output = Entry;
    const TOOL: &'static str = "open_entry";
}

/// A read of one entry.
#[derive(Debug, Clone, PartialEq, Eq)]
#[veoveo_hub_sdk::resource_read(provider=Ledger,address_type=LedgerAddress,content=Entry,
    address=LedgerAddress::Entry(self.entry.clone()),template="ledger://entries/{entry}",
    parse=|address|match address {LedgerAddress::Entry(entry)=>Some(Self{entry:entry.clone()})})]
pub struct ReadEntry {
    pub entry: EntryName,
}

/// What Relay saw of an entry it followed: each state it observed until the entry
/// closed, the content a wait for the closed state resolved with, and a plain read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FollowedEntry {
    pub states: Vec<EntryState>,
    pub waited: Entry,
    pub read: Entry,
}

impl veoveo_hub_sdk::definition::SingleOutput for Inspection {}
impl veoveo_hub_sdk::definition::SingleOutput for Seal {}
impl veoveo_hub_sdk::definition::SingleOutput for Tallied {}
impl veoveo_hub_sdk::definition::SingleOutput for Entry {}
impl veoveo_hub_sdk::definition::SingleOutput for FollowedEntry {}
