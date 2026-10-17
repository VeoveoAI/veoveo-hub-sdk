use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use veoveo_types::ServerSlug;

/// MCP result metadata carrying failures isolated from a federated catalog.
pub const HUB_DISCOVERY_DEGRADATION_META_KEY: &str = "ai.veoveo/hub-discovery-degradation";

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

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct HubDiscoveryDegradation {
    pub failures: Vec<HubDiscoveryFailure>,
}

impl HubDiscoveryDegradation {
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
