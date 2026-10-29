use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use veoveo_types::ServerSlug;

/// MCP result metadata the Hub attaches to what it federates: a [`HubMeta`].
pub const HUB_META_KEY: &str = "ai.veoveo/hub";

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum HubDiscoverySurface {
    Resources,
    ResourceTemplates,
    Tools,
    Prompts,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum HubDiscoveryFailureCode {
    DiscoveryPending,
    UpstreamUnavailable,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HubDiscoveryFailure {
    pub server: ServerSlug,
    pub surface: HubDiscoverySurface,
    pub code: HubDiscoveryFailureCode,
}

/// `_meta["ai.veoveo/hub"]`: what the Hub reports about a federated result.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HubMeta {
    pub discovery: HubDiscovery,
}

/// The servers a federated catalog lacks, and why.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HubDiscovery {
    pub failures: Vec<HubDiscoveryFailure>,
}

impl HubDiscovery {
    pub fn new(failures: impl IntoIterator<Item = HubDiscoveryFailure>) -> Self {
        let mut failures: Vec<_> = failures.into_iter().collect();
        failures.sort();
        failures.dedup();
        Self { failures }
    }

    pub fn is_empty(&self) -> bool {
        self.failures.is_empty()
    }

    pub fn merge(&mut self, other: Self) {
        self.failures.extend(other.failures);
        self.failures.sort();
        self.failures.dedup();
    }
}
