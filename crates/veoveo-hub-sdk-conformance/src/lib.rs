//! Domain-neutral certification for a running Veoveo hosted MCP server.

pub mod catalog;
pub mod naming;
pub mod paging;
pub mod requirements;
pub mod schema_evidence;

mod profile;
mod report;
mod runner;
mod tool_schema;

pub use profile::{
    ContractId, HOSTED_MCP_CONTRACT, HOSTED_SERVER_PROFILE_FORMAT, HostedServerConformanceProfile,
    HostedServerProfileFormat, HttpBoundaryProfile, SurfaceExpectation, SurfaceProfile,
};
pub use report::{
    CONFORMANCE_REPORT_FORMAT, CheckResult, CheckStatus, ConformanceReport,
    ConformanceReportFormat, ObservedImplementation,
};
pub use runner::{
    run_hosted_server_conformance, run_hosted_server_conformance_with_evidence,
    run_hosted_server_conformance_with_paging,
};
pub use schema_evidence::{
    NamingEvidence, OwnerSchemaEvidence, SchemaEvidenceOrigin, SchemaObservation,
    ToolNameProjection,
};
pub use tool_schema::{SchemaStats, validate_tool_input_schema};

#[must_use]
pub fn hosted_server_conformance_profile_schema() -> schemars::Schema {
    schemars::schema_for!(HostedServerConformanceProfile)
}

#[must_use]
pub fn conformance_report_schema() -> schemars::Schema {
    schemars::schema_for!(ConformanceReport)
}

/// Runtime credentials supplied outside the serializable conformance profile.
#[derive(Clone, Default)]
pub struct ConformanceCredentials {
    bearer_token: Option<String>,
}

impl ConformanceCredentials {
    pub fn bearer(token: impl Into<String>) -> Self {
        Self {
            bearer_token: Some(token.into()),
        }
    }

    pub(crate) fn bearer_token(&self) -> Option<&str> {
        self.bearer_token.as_deref()
    }
}

/// Native SDK connection mechanics shared by owner-local utilities.
pub mod client;
