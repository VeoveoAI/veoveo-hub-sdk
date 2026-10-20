//! Execution selection and immutable resolved routing, independent of business data authority.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use veoveo_types::{DeploymentId, EnvironmentId, IdentifierError, ServerSlug, Sha256Digest};

/// HTTP selector on an authenticated MCP connection. Its value is an environment UUIDv7.
/// Parsing confers no authority: the Hub admits selection and captures the composition.
/// Repeated header fields must be rejected by the HTTP boundary, not concatenated.
pub const ENVIRONMENT_HEADER: &str = "veoveo-environment";

/// An untrusted connection selector; never substitute it for an admitted execution context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EnvironmentSelector(EnvironmentId);

impl EnvironmentSelector {
    pub fn parse(value: &str) -> Result<Self, IdentifierError> {
        EnvironmentId::parse(value).map(Self)
    }

    pub fn environment(&self) -> EnvironmentId {
        self.0
    }
}

/// Format and SHA-256 domain of the SDK's immutable resolved composition record.
pub const RESOLVED_COMPOSITION_FORMAT: &str = "https://veoveo.ai/formats/resolved-composition";

/// The digest of one canonical resolved composition record, not a native source revision.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(transparent)]
pub struct CompositionRevision(Sha256Digest);

impl CompositionRevision {
    pub fn digest(&self) -> &Sha256Digest {
        &self.0
    }
}

/// Captured selection retained through downstream calls and long-running Tasks.
/// The target deployment is separate: a downstream hop changes target, not composition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ExecutionContext {
    pub environment: EnvironmentId,
    pub composition: CompositionRevision,
}

/// One selected logical component. Unavailable explicitly selects no serving deployment;
/// it never means inherit or fall back. Transient target health does not change this record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ResolvedComponent {
    Deployment { deployment: DeploymentId },
    Unavailable {},
}

/// Complete resolved routing for an environment. No labels, source selectors, mutable
/// health, secrets or business WorkContext belong in the hashed record. Owners retain
/// the record and its deployment bindings for the lifetime of captured operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedComposition {
    environment: EnvironmentId,
    components: BTreeMap<ServerSlug, ResolvedComponent>,
    revision: CompositionRevision,
}

impl ResolvedComposition {
    pub fn new(
        environment: EnvironmentId,
        components: BTreeMap<ServerSlug, ResolvedComponent>,
    ) -> Self {
        let bytes = canonical_bytes(environment, &components);
        let mut hash = Sha256::new();
        hash.update(RESOLVED_COMPOSITION_FORMAT.as_bytes());
        hash.update([0]);
        hash.update(bytes);
        Self {
            environment,
            components,
            revision: CompositionRevision(Sha256Digest::from_bytes(hash.finalize().into())),
        }
    }

    pub fn context(&self) -> ExecutionContext {
        ExecutionContext {
            environment: self.environment,
            composition: self.revision.clone(),
        }
    }

    pub fn revision(&self) -> &CompositionRevision {
        &self.revision
    }

    pub fn components(&self) -> &BTreeMap<ServerSlug, ResolvedComponent> {
        &self.components
    }

    /// Compact UTF-8 JSON, fixed member order, version 1 and lexical component-key order.
    /// Recompute the revision on import; a supplied digest alone is not an admitted record.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        canonical_bytes(self.environment, &self.components)
    }

    /// Read the exact canonical record, rejecting unsupported formats, duplicate keys,
    /// unknown fields and noncanonical encodings. The digest is always recomputed.
    pub fn from_canonical_bytes(bytes: &[u8]) -> Result<Self, CompositionError> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Record {
            format: String,
            format_version: u8,
            environment: EnvironmentId,
            components: BTreeMap<ServerSlug, ResolvedComponent>,
        }
        let record: Record = serde_json::from_slice(bytes).map_err(CompositionError::Json)?;
        if record.format != RESOLVED_COMPOSITION_FORMAT || record.format_version != 1 {
            return Err(CompositionError::UnsupportedFormat);
        }
        let composition = Self::new(record.environment, record.components);
        if composition.canonical_bytes() != bytes {
            return Err(CompositionError::NonCanonical);
        }
        Ok(composition)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CompositionError {
    #[error("invalid resolved composition record: {0}")]
    Json(serde_json::Error),
    #[error("unsupported resolved composition format")]
    UnsupportedFormat,
    #[error("resolved composition record is not canonical")]
    NonCanonical,
}

fn canonical_bytes(
    environment: EnvironmentId,
    components: &BTreeMap<ServerSlug, ResolvedComponent>,
) -> Vec<u8> {
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Record<'a> {
        format: &'static str,
        format_version: u8,
        environment: EnvironmentId,
        components: &'a BTreeMap<ServerSlug, ResolvedComponent>,
    }
    serde_json::to_vec(&Record {
        format: RESOLVED_COMPOSITION_FORMAT,
        format_version: 1,
        environment,
        components,
    })
    .expect("resolved composition contains only admitted string identifiers and fixed fields")
}
