//! Identity and embedded documentation, without compliance self-attestation.
use super::{ContractId, ServerDocs};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use veoveo_types::ServerSlug;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ContractDeclaration {
    contract: ContractId,
    server: ServerSlug,
}
impl ContractDeclaration {
    pub fn from_docs(docs: &ServerDocs) -> Self {
        Self {
            contract: ContractId::HostedMcp,
            server: docs.identity().clone(),
        }
    }
    pub fn contract(&self) -> ContractId {
        self.contract
    }
    pub fn server(&self) -> &ServerSlug {
        &self.server
    }
}
