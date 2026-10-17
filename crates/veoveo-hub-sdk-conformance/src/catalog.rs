//! Complete catalog reads for certification: the SDK's bounded listings, refused when
//! the Hub reports that a registered server's surface is missing from them.

use anyhow::{Context, Result, bail};
use rmcp::{RoleClient, model::*, service::Peer};
use std::future::Future;
use veoveo_hub_sdk::client::{CatalogListing, catalog};

pub(crate) async fn before_naming_deadline<T>(
    started: std::time::Instant,
    operation: impl Future<Output = Result<T>>,
) -> Result<T> {
    let remaining = crate::naming::NAMING_DEADLINE
        .checked_sub(started.elapsed())
        .ok_or_else(|| anyhow::anyhow!("aggregate naming discovery deadline expired"))?;
    tokio::time::timeout(remaining, operation)
        .await
        .context("aggregate naming discovery deadline expired")?
}

// A degraded listing cannot establish complete discovery, even when it carries
// useful items.
fn complete<T>(listing: CatalogListing<T>, operation: &str) -> Result<Vec<T>> {
    if !listing.is_complete() {
        bail!(
            "{operation} is incomplete: {} admitted hub discovery failures",
            listing.degradation.failures.len()
        );
    }
    Ok(listing.items)
}

pub async fn tools(client: &Peer<RoleClient>) -> Result<Vec<Tool>> {
    complete(catalog::tools(client).await?, "tools/list")
}
pub async fn resources(client: &Peer<RoleClient>) -> Result<Vec<Resource>> {
    complete(catalog::resources(client).await?, "resources/list")
}
pub async fn templates(client: &Peer<RoleClient>) -> Result<Vec<ResourceTemplate>> {
    complete(
        catalog::resource_templates(client).await?,
        "resources/templates/list",
    )
}
pub async fn prompts(client: &Peer<RoleClient>) -> Result<Vec<Prompt>> {
    complete(catalog::prompts(client).await?, "prompts/list")
}
