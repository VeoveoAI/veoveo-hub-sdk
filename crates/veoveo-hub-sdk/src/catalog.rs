use crate::hub::{HUB_META_KEY, HubDiscovery, HubMeta};
use rmcp::model::MetaObject;

/// MCP metadata conversion for the protocol-independent Hub declaration.
pub trait HubDiscoveryMetadata: Sized {
    fn into_meta(self) -> Option<MetaObject>;
    fn from_meta(meta: Option<&MetaObject>) -> Result<Self, serde_json::Error>;
}
impl HubDiscoveryMetadata for HubDiscovery {
    fn into_meta(self) -> Option<MetaObject> {
        if self.is_empty() {
            return None;
        }
        let mut meta = MetaObject::new();
        meta.0.insert(
            HUB_META_KEY.to_owned(),
            serde_json::to_value(HubMeta { discovery: self }).expect("hub metadata serializes"),
        );
        Some(meta)
    }

    fn from_meta(meta: Option<&MetaObject>) -> Result<Self, serde_json::Error> {
        let Some(value) = meta.and_then(|meta| meta.0.get(HUB_META_KEY).cloned()) else {
            return Ok(Self::default());
        };
        serde_json::from_value::<HubMeta>(value).map(|meta| meta.discovery)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ServerSlug;
    use crate::hub::{HubDiscoveryFailure, HubDiscoveryFailureCode, HubDiscoverySurface};

    #[test]
    fn discovery_metadata_is_typed_sorted_and_deduplicated() {
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
        let discovery = HubDiscovery::new([resources.clone(), tools.clone(), resources.clone()]);
        let meta = discovery.clone().into_meta().unwrap();
        assert_eq!(
            meta.0[HUB_META_KEY],
            serde_json::json!({"discovery": {"failures": [
                {"server": "observatory", "surface": "tools", "code": "upstream_unavailable"},
                {"server": "time", "surface": "resources", "code": "upstream_unavailable"}
            ]}})
        );
        assert_eq!(HubDiscovery::from_meta(Some(&meta)).unwrap(), discovery);
        assert_eq!(discovery.failures, vec![tools, resources]);
        assert!(HubDiscovery::default().into_meta().is_none());
    }
}
