//! Complete catalog listings: every page of `tools/list`, `resources/list`,
//! `resources/templates/list` or `prompts/list`, within fixed bounds, with the
//! discovery failures the Hub reported on any page.

use std::{collections::BTreeSet, future::Future, time::Duration};

use futures::future::BoxFuture;
use rmcp::{
    RoleClient,
    model::{MetaObject, PaginatedRequestParams, Prompt, Resource, ResourceTemplate, Tool},
    service::{Peer, ServiceError},
};

use crate::hub::{HUB_DISCOVERY_DEGRADATION_META_KEY, HubDiscoveryDegradation};

/// The most pages one listing follows.
pub const MAX_PAGES: usize = 1_024;
/// The most items one listing holds.
pub const MAX_ITEMS: usize = 16_384;
/// The longest one listing may take, every page included.
pub const DEADLINE: Duration = Duration::from_secs(30);

/// Every item of one listing and the discovery failures the Hub reported with it.
/// A listing with failures misses the surfaces of the servers they name.
#[derive(Debug, Clone)]
pub struct CatalogListing<T> {
    pub items: Vec<T>,
    pub degradation: HubDiscoveryDegradation,
}

impl<T> CatalogListing<T> {
    /// Whether every registered server answered.
    pub fn is_complete(&self) -> bool {
        self.degradation.is_empty()
    }
}

/// Why a listing ended without every page.
#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    #[error("{operation} failed: {error}")]
    Request {
        operation: &'static str,
        #[source]
        error: ServiceError,
    },
    #[error("{operation} repeated a continuation cursor")]
    RepeatedCursor { operation: &'static str },
    #[error("{operation} exceeded {MAX_PAGES} pages")]
    TooManyPages { operation: &'static str },
    #[error("{operation} exceeded {MAX_ITEMS} items")]
    TooManyItems { operation: &'static str },
    #[error("{operation} exceeded its {DEADLINE:?} deadline")]
    Deadline { operation: &'static str },
    #[error("{operation} carried malformed Hub discovery metadata: {error}")]
    Metadata {
        operation: &'static str,
        #[source]
        error: serde_json::Error,
    },
}

impl CatalogError {
    /// The request error, when a request failed.
    pub fn request(&self) -> Option<&ServiceError> {
        match self {
            Self::Request { error, .. } => Some(error),
            _ => None,
        }
    }
}

/// One page: its items, the next cursor and the page's metadata.
pub(crate) type Page<T> = (Vec<T>, Option<String>, Option<MetaObject>);

/// Follows `next` from the first page until a page names no cursor.
pub(crate) async fn collect<T, F, Fut>(
    operation: &'static str,
    mut next: F,
) -> Result<CatalogListing<T>, CatalogError>
where
    F: FnMut(Option<String>) -> Fut,
    Fut: Future<Output = Result<Page<T>, ServiceError>>,
{
    let listing = async {
        let mut items = Vec::new();
        let mut failures = Vec::new();
        let mut cursor = None;
        let mut seen = BTreeSet::new();
        for _ in 0..MAX_PAGES {
            let (page, following, meta) = next(cursor)
                .await
                .map_err(|error| CatalogError::Request { operation, error })?;
            if let Some(value) = meta
                .as_ref()
                .and_then(|meta| meta.0.get(HUB_DISCOVERY_DEGRADATION_META_KEY))
            {
                let degradation: HubDiscoveryDegradation = serde_json::from_value(value.clone())
                    .map_err(|error| CatalogError::Metadata { operation, error })?;
                failures.extend(degradation.failures);
            }
            if items.len() + page.len() > MAX_ITEMS {
                return Err(CatalogError::TooManyItems { operation });
            }
            items.extend(page);
            let Some(following) = following else {
                return Ok(CatalogListing {
                    items,
                    degradation: HubDiscoveryDegradation::new(failures),
                });
            };
            if !seen.insert(following.clone()) {
                return Err(CatalogError::RepeatedCursor { operation });
            }
            cursor = Some(following);
        }
        Err(CatalogError::TooManyPages { operation })
    };
    tokio::time::timeout(DEADLINE, listing)
        .await
        .unwrap_or(Err(CatalogError::Deadline { operation }))
}

fn page(cursor: Option<String>) -> Option<PaginatedRequestParams> {
    Some(PaginatedRequestParams::default().with_cursor(cursor))
}

/// Every tool `peer` lists.
pub fn tools(peer: &Peer<RoleClient>) -> BoxFuture<'_, Result<CatalogListing<Tool>, CatalogError>> {
    Box::pin(collect("tools/list", move |cursor| async move {
        let listed = peer.list_tools(page(cursor)).await?;
        Ok((listed.tools, listed.next_cursor, listed.meta))
    }))
}

/// Every resource `peer` lists.
pub fn resources(
    peer: &Peer<RoleClient>,
) -> BoxFuture<'_, Result<CatalogListing<Resource>, CatalogError>> {
    Box::pin(collect("resources/list", move |cursor| async move {
        let listed = peer.list_resources(page(cursor)).await?;
        Ok((listed.resources, listed.next_cursor, listed.meta))
    }))
}

/// Every resource template `peer` lists.
pub fn resource_templates(
    peer: &Peer<RoleClient>,
) -> BoxFuture<'_, Result<CatalogListing<ResourceTemplate>, CatalogError>> {
    Box::pin(collect(
        "resources/templates/list",
        move |cursor| async move {
            let listed = peer.list_resource_templates(page(cursor)).await?;
            Ok((listed.resource_templates, listed.next_cursor, listed.meta))
        },
    ))
}

/// Every prompt `peer` lists.
pub fn prompts(
    peer: &Peer<RoleClient>,
) -> BoxFuture<'_, Result<CatalogListing<Prompt>, CatalogError>> {
    Box::pin(collect("prompts/list", move |cursor| async move {
        let listed = peer.list_prompts(page(cursor)).await?;
        Ok((listed.prompts, listed.next_cursor, listed.meta))
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub::{HubDiscoveryFailure, HubDiscoveryFailureCode, HubDiscoverySurface};
    use veoveo_types::ServerSlug;

    fn ready<T>(page: Page<T>) -> std::future::Ready<Result<Page<T>, ServiceError>> {
        std::future::ready(Ok(page))
    }

    #[tokio::test]
    async fn follows_empty_pages_and_rejects_cycles_and_excessive_catalogs() {
        let listing = collect("fixture", |cursor| {
            ready(match cursor.as_deref() {
                None => (vec![1], Some("second".into()), None),
                Some("second") => (vec![], Some("last".into()), None),
                Some("last") => (vec![2], None, None),
                _ => unreachable!(),
            })
        })
        .await
        .unwrap();
        assert_eq!(listing.items, vec![1, 2]);
        assert!(listing.is_complete());
        let repeated =
            collect::<(), _, _>("fixture", |_| ready((vec![], Some("same".into()), None)))
                .await
                .unwrap_err();
        assert!(matches!(repeated, CatalogError::RepeatedCursor { .. }));
        let oversized = collect("fixture", |_| ready((vec![(); MAX_ITEMS + 1], None, None)))
            .await
            .unwrap_err();
        assert!(matches!(oversized, CatalogError::TooManyItems { .. }));
        let mut sequence = 0;
        let endless = collect::<(), _, _>("fixture", |_| {
            sequence += 1;
            ready((vec![], Some(sequence.to_string()), None))
        })
        .await
        .unwrap_err();
        assert!(matches!(endless, CatalogError::TooManyPages { .. }));
    }

    #[tokio::test]
    async fn reports_every_discovery_failure_a_page_carried() {
        let failure = |server: &str, surface| HubDiscoveryFailure {
            server: ServerSlug::parse(server).unwrap(),
            surface,
            code: HubDiscoveryFailureCode::UpstreamUnavailable,
        };
        let meta = |failure: HubDiscoveryFailure| {
            let mut meta = MetaObject::new();
            meta.0.insert(
                HUB_DISCOVERY_DEGRADATION_META_KEY.to_owned(),
                serde_json::to_value(HubDiscoveryDegradation::new([failure])).unwrap(),
            );
            Some(meta)
        };
        let time = failure("time", HubDiscoverySurface::Tools);
        let media = failure("media", HubDiscoverySurface::Tools);
        let listing = collect("fixture", |cursor| {
            ready(match cursor.as_deref() {
                None => (vec![1], Some("next".into()), meta(time.clone())),
                _ => (vec![2], None, meta(media.clone())),
            })
        })
        .await
        .unwrap();
        assert_eq!(listing.items, vec![1, 2]);
        assert!(!listing.is_complete());
        assert_eq!(
            listing.degradation,
            HubDiscoveryDegradation::new([time, media])
        );
        let mut malformed = MetaObject::new();
        malformed.0.insert(
            HUB_DISCOVERY_DEGRADATION_META_KEY.to_owned(),
            serde_json::json!("degraded"),
        );
        let error = collect::<(), _, _>("fixture", |_| {
            ready((vec![], None, Some(malformed.clone())))
        })
        .await
        .unwrap_err();
        assert!(matches!(error, CatalogError::Metadata { .. }));
    }
}
