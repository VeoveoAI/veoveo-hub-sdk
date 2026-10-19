//! Measured traversal of owner-selected domain pages through trusted fetch adapters.
//!
//! This module owns no domain wire DTO, URI builder or cursor codec. Adapters must
//! fetch the selected server and extract keys from the actual response. Policy,
//! adapter correctness and selection completeness remain source-review obligations.
use std::{collections::BTreeSet, future::Future, time::Duration};

use anyhow::Result;
use serde::Serialize;
use serde_json::json;
use veoveo_types::{NamingLabel, ServerSlug};

use crate::{CheckResult, CheckStatus};

pub const PAGING_CHECK: &str = "VV-MCP-PAGING-001";
pub const MAX_PAGING_PAGES: usize = 1024;
pub const MAX_PAGING_ITEMS: usize = 16_384;
pub const MAX_PAGING_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_PAGING_SELECTIONS: usize = 128;
pub const MAX_PAGING_LABEL_BYTES: usize = 1024;
pub const PAGING_DEADLINE: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PagingSurface {
    Resource,
    Tool,
}

/// Owner-declared provenance, subject to review; not remotely authenticated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PagingOrigin {
    SyntheticFixture,
    Component,
    Deployed,
}

/// Owner-declared limits, further bounded by the inspector's independent budgets.
pub struct PagingLimits {
    pub page_items: usize,
    /// Complete serialized MCP result, excluding the JSON-RPC envelope. The host
    /// separately enforces C10 on the final serialized JSON-RPC response.
    pub response_bytes: usize,
    pub cursor_bytes: usize,
}

/// Keys must encode the owner's total order, including its unique tie-breaker.
/// A constructor measures the actual serializable response rather than accepting
/// an owner-supplied byte count. No response or key is retained in report evidence.
pub struct ObservedPage<K> {
    keys: Vec<K>,
    next_cursor: Option<String>,
    response_bytes: usize,
}

impl<K> ObservedPage<K> {
    pub fn from_response<R: Serialize>(
        response: &R,
        keys: Vec<K>,
        next_cursor: Option<String>,
    ) -> Result<Self> {
        // Serialize through a bounded writer: an oversized server response must
        // not cause an additional unbounded byte allocation inside the inspector.
        let mut writer = ByteCounter(0);
        if let Err(error) = serde_json::to_writer(&mut writer, response) {
            if writer.0 > veoveo_hub_sdk::MAX_MCP_RESPONSE_BYTES {
                return Err(anyhow::Error::new(OversizedResponse(writer.0)));
            }
            return Err(error.into());
        }
        Ok(Self {
            keys,
            next_cursor,
            response_bytes: writer.0,
        })
    }
}

struct ByteCounter(usize);
#[derive(Debug)]
struct OversizedResponse(usize);
impl std::fmt::Display for OversizedResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("serialized MCP result exceeds response byte cap")
    }
}
impl std::error::Error for OversizedResponse {}
impl std::io::Write for ByteCounter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0 = self.0.saturating_add(bytes.len());
        if self.0 > veoveo_hub_sdk::MAX_MCP_RESPONSE_BYTES {
            return Err(std::io::Error::other(
                "paging response byte budget exceeded",
            ));
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Private measured fields prevent owner declarations from becoming probe passes.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PagingObservation {
    server: ServerSlug,
    listing: NamingLabel,
    surface: PagingSurface,
    origin: PagingOrigin,
    status: CheckStatus,
    reason: &'static str,
    pages: usize,
    items: usize,
    response_bytes: usize,
    elapsed_micros: u128,
    page_item_limit: usize,
    response_byte_limit: usize,
    cursor_byte_limit: usize,
}

impl PagingObservation {
    pub fn status(&self) -> CheckStatus {
        self.status
    }
}

/// Execute one unchanged-data traversal. `expected` comes from the owner's
/// independently known fixture/store selection, not from the pages under test.
/// The adapter receives each continuation exactly as emitted, including `""`.
/// Fetch/decode errors and timeouts yield incomplete evidence, without copying
/// error text, response bodies, identities or cursor values into reports.
pub async fn inspect_pages<K: Ord + Serialize, F, Fut>(
    server: ServerSlug,
    listing: NamingLabel,
    surface: PagingSurface,
    origin: PagingOrigin,
    limits: &PagingLimits,
    expected: &[K],
    mut fetch: F,
) -> PagingObservation
where
    F: FnMut(Option<String>) -> Fut,
    Fut: Future<Output = Result<ObservedPage<K>>>,
{
    let started = std::time::Instant::now();
    let mut observation = PagingObservation {
        server,
        listing,
        surface,
        origin,
        status: CheckStatus::Failed,
        reason: "invalid probe selection or limits",
        pages: 0,
        items: 0,
        response_bytes: 0,
        elapsed_micros: 0,
        page_item_limit: limits.page_items,
        response_byte_limit: limits.response_bytes,
        cursor_byte_limit: limits.cursor_bytes,
    };
    if observation.listing.as_str().len() > MAX_PAGING_LABEL_BYTES
        || limits.page_items == 0
        || limits.page_items > MAX_PAGING_ITEMS
        || limits.response_bytes == 0
        || limits.response_bytes > veoveo_hub_sdk::MAX_MCP_RESPONSE_BYTES
        || limits.cursor_bytes == 0
        || limits.cursor_bytes > veoveo_hub_sdk::MAX_MCP_RESPONSE_BYTES
        || expected.len() > MAX_PAGING_ITEMS
        || serde_json::to_writer(&mut ByteCounter(0), expected).is_err()
        || expected.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return observation;
    }
    let mut cursor = None;
    let mut cursors = BTreeSet::new();
    let mut cursor_bytes = 0usize;
    loop {
        if observation.pages >= MAX_PAGING_PAGES {
            observation.status = CheckStatus::Incomplete;
            observation.reason = "inspection page budget exhausted";
            break;
        }
        let Some(remaining) = PAGING_DEADLINE.checked_sub(started.elapsed()) else {
            observation.status = CheckStatus::Incomplete;
            observation.reason = "inspection deadline expired";
            break;
        };
        let page = match tokio::time::timeout(remaining, fetch(cursor.take())).await {
            Ok(Ok(page)) => page,
            Ok(Err(error)) => {
                if let Some(oversized) = error.downcast_ref::<OversizedResponse>() {
                    observation.pages += 1;
                    observation.response_bytes =
                        observation.response_bytes.saturating_add(oversized.0);
                    observation.reason = "serialized MCP result exceeds response byte cap";
                    break;
                }
                observation.status = CheckStatus::Incomplete;
                observation.reason = "fetch or response decoding unavailable";
                break;
            }
            Err(_) => {
                observation.status = CheckStatus::Incomplete;
                observation.reason = "inspection deadline expired";
                break;
            }
        };
        // Ready futures may perform synchronous fetch/decode/serialization work;
        // timeout cannot preempt it, so recheck before accepting any page.
        if started.elapsed() >= PAGING_DEADLINE {
            observation.status = CheckStatus::Incomplete;
            observation.reason = "inspection deadline expired";
            break;
        }
        observation.pages += 1;
        observation.response_bytes = observation
            .response_bytes
            .saturating_add(page.response_bytes);
        if page.keys.len() > limits.page_items || page.response_bytes > limits.response_bytes {
            observation.reason = "page exceeds declared item or response byte limit";
            break;
        }
        if observation.response_bytes > MAX_PAGING_BYTES {
            observation.status = CheckStatus::Incomplete;
            observation.reason = "inspection aggregate byte budget exhausted";
            break;
        }
        let end = observation.items.saturating_add(page.keys.len());
        if end > MAX_PAGING_ITEMS {
            observation.status = CheckStatus::Incomplete;
            observation.reason = "inspection item budget exhausted";
            break;
        }
        if expected.get(observation.items..end) != Some(page.keys.as_slice()) {
            observation.reason = "items differ from independent complete ordered selection";
            break;
        }
        observation.items = end;
        match page.next_cursor {
            None => {
                if observation.items == expected.len() {
                    observation.status = CheckStatus::Passed;
                    observation.reason = "selected unchanged-data traversal completed";
                } else {
                    observation.reason = "terminal page omitted expected items";
                }
                break;
            }
            Some(next) => {
                if next.len() > limits.cursor_bytes {
                    observation.reason = "continuation exceeds declared cursor byte limit";
                    break;
                }
                cursor_bytes = cursor_bytes.saturating_add(next.len());
                if cursor_bytes > MAX_PAGING_BYTES {
                    observation.status = CheckStatus::Incomplete;
                    observation.reason = "inspection aggregate cursor byte budget exhausted";
                    break;
                }
                if !cursors.insert(next.clone()) {
                    observation.reason = "continuation cursor repeated";
                    break;
                }
                cursor = Some(next);
            }
        }
    }
    if observation.status == CheckStatus::Passed && started.elapsed() >= PAGING_DEADLINE {
        observation.status = CheckStatus::Incomplete;
        observation.reason = "inspection deadline expired";
    }
    observation.elapsed_micros = started.elapsed().as_micros();
    observation
}

/// In-memory selections; no domain probe instructions are added to MCP or JSON
/// profiles. Each required listing needs its own successful multi-page traversal.
/// Empty and single-page observations are supplementary evidence only.
#[derive(Default)]
pub struct PagingEvidence<'a> {
    pub required_listings: &'a [NamingLabel],
    pub observations: &'a [PagingObservation],
}

pub fn check_paging(evidence: &PagingEvidence<'_>, server: &str) -> CheckResult {
    let result = |status, summary: &str| CheckResult {
        requirement_id: PAGING_CHECK.into(),
        status,
        summary: summary.into(),
        evidence: Some(json!({
            "outcome": "not_established",
            "observationCount": evidence.observations.len(),
            "observations": evidence.observations.iter().take(MAX_PAGING_SELECTIONS)
                .filter(|observation| observation.listing.as_str().len() <= MAX_PAGING_LABEL_BYTES)
                .collect::<Vec<_>>()
        })),
    };
    if evidence.required_listings.is_empty()
        || evidence.required_listings.len() > MAX_PAGING_SELECTIONS
        || evidence.observations.len() > MAX_PAGING_SELECTIONS
        || evidence
            .required_listings
            .iter()
            .any(|listing| listing.as_str().len() > MAX_PAGING_LABEL_BYTES)
    {
        return result(
            CheckStatus::Incomplete,
            "bounded owner paging selection is required",
        );
    }
    let required: BTreeSet<_> = evidence
        .required_listings
        .iter()
        .map(NamingLabel::as_str)
        .collect();
    if required.len() != evidence.required_listings.len() {
        return result(CheckStatus::Failed, "duplicate required paging listing");
    }
    if evidence.observations.iter().any(|observation| {
        observation.server.as_str() != server
            || !required.contains(observation.listing.as_str())
            || observation.status == CheckStatus::Failed
    }) {
        return result(
            CheckStatus::Failed,
            "selected paging traversal failed or was unselected",
        );
    }
    let mut pages = 0usize;
    let mut items = 0usize;
    let mut bytes = 0usize;
    let mut micros = 0u128;
    for observation in evidence.observations {
        pages = pages.saturating_add(observation.pages);
        items = items.saturating_add(observation.items);
        bytes = bytes.saturating_add(observation.response_bytes);
        micros = micros.saturating_add(observation.elapsed_micros);
        if evidence.observations.iter().any(|other| {
            other.listing == observation.listing
                && (other.surface != observation.surface || other.origin != observation.origin)
        }) {
            return result(
                CheckStatus::Failed,
                "listing label has inconsistent surface or provenance",
            );
        }
    }
    if pages > MAX_PAGING_PAGES
        || items > MAX_PAGING_ITEMS
        || bytes > MAX_PAGING_BYTES
        || micros > PAGING_DEADLINE.as_micros()
    {
        return result(
            CheckStatus::Incomplete,
            "aggregate paging inspection budget exhausted",
        );
    }
    if evidence
        .observations
        .iter()
        .any(|observation| observation.status != CheckStatus::Passed)
        || required.iter().any(|listing| {
            !evidence.observations.iter().any(|observation| {
                observation.listing.as_str() == *listing
                    && observation.pages >= 2
                    && observation.items >= 2
            })
        })
    {
        return result(
            CheckStatus::Incomplete,
            "each selected listing requires completed continuation evidence",
        );
    }
    CheckResult {
        requirement_id: PAGING_CHECK.into(),
        status: CheckStatus::Passed,
        summary:
            "selected paging traversals passed; domain policy and adapter coverage require review"
                .into(),
        evidence: Some(json!({
            "outcome": "review_required",
            "observations": evidence.observations,
            "limitations": "Owner adapters and provenance are trusted inputs. Scope, authorization, mutation, replicas and complete listing selection require owner tests and review."
        })),
    }
}

#[cfg(test)]
mod tests;
