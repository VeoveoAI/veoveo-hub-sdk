use crate::hub::{HUB_DISCOVERY_DEGRADATION_META_KEY, HubDiscoveryDegradation};
use rmcp::model::MetaObject;

/// MCP metadata conversion for the protocol-independent Hub declaration.
pub trait HubDiscoveryMetadata: Sized {
    fn into_meta(self) -> Option<MetaObject>;
    fn from_meta(meta: Option<&MetaObject>) -> Result<Self, serde_json::Error>;
}
impl HubDiscoveryMetadata for HubDiscoveryDegradation {
    fn into_meta(self) -> Option<MetaObject> {
        if self.is_empty() {
            return None;
        }
        let mut meta = MetaObject::new();
        meta.0.insert(
            HUB_DISCOVERY_DEGRADATION_META_KEY.to_owned(),
            serde_json::to_value(self).expect("hub discovery degradation serializes"),
        );
        Some(meta)
    }

    fn from_meta(meta: Option<&MetaObject>) -> Result<Self, serde_json::Error> {
        let Some(value) =
            meta.and_then(|meta| meta.0.get(HUB_DISCOVERY_DEGRADATION_META_KEY).cloned())
        else {
            return Ok(Self::default());
        };
        serde_json::from_value(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ServerSlug;
    use crate::hub::{HubDiscoveryFailure, HubDiscoveryFailureCode, HubDiscoverySurface};

    #[test]
    fn degradation_metadata_is_typed_sorted_and_deduplicated() {
        let resources = HubDiscoveryFailure {
            server: ServerSlug::parse("time").unwrap(),
            surface: HubDiscoverySurface::Resources,
            code: HubDiscoveryFailureCode::UpstreamUnavailable,
        };
        let tools = HubDiscoveryFailure {
            server: ServerSlug::parse("observatory").unwrap(),
            surface: HubDiscoverySurface::Tools,
            code: HubDiscoveryFailureCode::UpstreamUnavailable,
        };
        let degradation =
            HubDiscoveryDegradation::new([resources.clone(), tools.clone(), resources.clone()]);
        let meta = degradation.clone().into_meta().unwrap();
        assert_eq!(
            HubDiscoveryDegradation::from_meta(Some(&meta)).unwrap(),
            degradation
        );
        assert_eq!(degradation.failures, vec![tools, resources]);
    }
}
