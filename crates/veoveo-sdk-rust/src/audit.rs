//! The MCP audit vocabulary. An MCP producer, such as the Gateway, declares these classes
//! in its audit registry under the owner `mcp` and records them on core's audit
//! envelope. A record names what it is about through an [`McpTarget`] URI under the
//! platform's `veoveo` scheme, and the facts of the caller's invocation that the
//! envelope does not carry travel in each detail's [`AuditInvocation`].
use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use veoveo_audit_contract::{
    AuditDetailError, AuditDetailType, AuditOcsfClass, AuditRegistryBuilder, AuditTarget,
};
use veoveo_types::{
    DataLabelId, OAuthClientId, PolicyVersion, PrincipalId, ResourceTemplateUri,
    ResourceUriBuilder, ResourceUriError, ScopeName, ServerSlug, Sha256Digest, UriSegment,
};

use crate::identity::{CanonicalTaskId, LocalToolName, PromptName};

/// OCSF API Activity's Read activity.
const OCSF_READ: AuditOcsfClass = AuditOcsfClass::api_activity(2);

/// The catalog a discovery request listed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, veoveo_types::Vocabulary)]
pub enum DiscoveryKind {
    Tools,
    Resources,
    ResourceTemplates,
    Prompts,
}

/// How a tool call ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, veoveo_types::Vocabulary)]
pub enum ToolResultKind {
    Complete,
    ErrorResult,
    InputRequired,
    TaskCreated,
    OtherResponse,
    ProtocolError,
}

/// The MCP request that read data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, veoveo_types::Vocabulary)]
pub enum McpReadMethod {
    ResourceRead,
    PromptGet,
    Completion,
    Subscription,
}

/// The facts of a caller's invocation beside the envelope's actor: the OAuth client it
/// reached the Gateway with, the principal that delegated to it, the policy revision
/// that admitted it, and its scopes and data labels.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AuditInvocation {
    pub oauth_client: OAuthClientId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delegating_principal: Option<PrincipalId>,
    pub policy_revision: PolicyVersion,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub scopes: BTreeSet<ScopeName>,
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub data_labels: BTreeSet<DataLabelId>,
}

/// A caller listed a catalog: class `mcp.discovery`, OCSF API Activity Read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Discovery {
    pub collection: DiscoveryKind,
    pub visible: u32,
    pub denied: u32,
    pub visible_digest: Sha256Digest,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invocation: Option<AuditInvocation>,
}
impl AuditDetailType for Discovery {
    const CLASS: &'static str = "mcp.discovery";
    const OCSF: AuditOcsfClass = OCSF_READ;
}

/// A tool call was admitted or refused before it reached its server: class
/// `mcp.tool_admission`. The server that made the call for the caller, and its Task,
/// travel in the envelope's `via`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ToolAdmission {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invocation: Option<AuditInvocation>,
}
impl AuditDetailType for ToolAdmission {
    const CLASS: &'static str = "mcp.tool_admission";
}

/// A tool call finished: class `mcp.tool_completion`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ToolCompletion {
    pub result: ToolResultKind,
    pub duration_ms: u64,
    pub error_code: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invocation: Option<AuditInvocation>,
}
impl AuditDetailType for ToolCompletion {
    const CLASS: &'static str = "mcp.tool_completion";
}

/// A caller read a resource, a prompt, completions or a subscription: class `mcp.read`,
/// OCSF API Activity Read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Read {
    pub method: McpReadMethod,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invocation: Option<AuditInvocation>,
}
impl AuditDetailType for Read {
    const CLASS: &'static str = "mcp.read";
    const OCSF: AuditOcsfClass = OCSF_READ;
}

/// What an MCP record is about. Each target is a URI under the platform's reserved
/// `veoveo` scheme, built by the shared resource URI builder:
///
/// | Target | URI |
/// |---|---|
/// | A server's tool | `veoveo://servers/{server}/tools/{tool}` |
/// | A server's resource template | `veoveo://servers/{server}/resource-templates/{template}` |
/// | A server's prompt | `veoveo://servers/{server}/prompts/{prompt}` |
/// | A server's catalog | `veoveo://servers/{server}/catalog/{collection}` |
/// | The federated catalog | `veoveo://catalog/{collection}` |
/// | A Gateway Task route | `veoveo://servers/{server}/tasks/{route}` |
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpTarget {
    Tool {
        server: ServerSlug,
        tool: LocalToolName,
    },
    ResourceTemplate {
        server: ServerSlug,
        uri: ResourceTemplateUri,
    },
    Prompt {
        server: ServerSlug,
        prompt: PromptName,
    },
    /// A listing; a federated listing names no server.
    Discovery {
        server: Option<ServerSlug>,
        collection: DiscoveryKind,
    },
    TaskRoute {
        server: ServerSlug,
        route: CanonicalTaskId,
    },
}

/// An MCP target that could not become an audit target.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum McpTargetError {
    #[error(transparent)]
    Uri(#[from] ResourceUriError),
    #[error("the target URI is not an audit target: {0}")]
    Target(String),
}

impl McpTarget {
    /// The target's URI as the audit envelope records it.
    pub fn audit_target(&self) -> Result<AuditTarget, McpTargetError> {
        let segment = |value: &str| UriSegment::new(value);
        let uri = match self {
            Self::Tool { server, tool } => server_root(server)?
                .segment(segment("tools")?)
                .segment(segment(tool.as_str())?),
            Self::ResourceTemplate { server, uri } => server_root(server)?
                .segment(segment("resource-templates")?)
                .segment(segment(uri.as_str())?),
            Self::Prompt { server, prompt } => server_root(server)?
                .segment(segment("prompts")?)
                .segment(segment(prompt.as_str())?),
            Self::Discovery {
                server: Some(server),
                collection,
            } => server_root(server)?
                .segment(segment("catalog")?)
                .segment(segment(collection.as_str())?),
            Self::Discovery {
                server: None,
                collection,
            } => {
                ResourceUriBuilder::new("veoveo://catalog")?.segment(segment(collection.as_str())?)
            }
            Self::TaskRoute { server, route } => server_root(server)?
                .segment(segment("tasks")?)
                .segment(segment(route.as_str())?),
        }
        .build()?;
        AuditTarget::parse(uri.as_str()).map_err(|error| McpTargetError::Target(error.to_string()))
    }
}

fn server_root(server: &ServerSlug) -> Result<ResourceUriBuilder, ResourceUriError> {
    Ok(ResourceUriBuilder::new("veoveo://servers")?.segment(UriSegment::new(server.as_str())?))
}

/// Declare the MCP classes in an audit registry.
pub fn register(builder: &mut AuditRegistryBuilder) -> Result<(), AuditDetailError> {
    builder.declare::<Discovery>()?;
    builder.declare::<ToolAdmission>()?;
    builder.declare::<ToolCompletion>()?;
    builder.declare::<Read>()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use veoveo_audit_contract::AuditRegistry;

    fn server() -> ServerSlug {
        ServerSlug::parse("time").unwrap()
    }

    #[test]
    fn the_mcp_vocabulary_joins_a_registry_with_its_ocsf_classes() {
        let mut builder = AuditRegistry::builder();
        register(&mut builder).unwrap();
        let registry = builder.build();
        let detail = registry
            .detail(&ToolCompletion {
                result: ToolResultKind::Complete,
                duration_ms: 12,
                error_code: None,
                invocation: None,
            })
            .unwrap();
        assert_eq!(detail.class().as_str(), "mcp.tool_completion");
        assert_eq!(Discovery::OCSF, AuditOcsfClass::api_activity(2));
        assert_eq!(Read::OCSF, AuditOcsfClass::api_activity(2));
        assert_eq!(ToolAdmission::OCSF, AuditOcsfClass::API_ACTIVITY_OTHER);
        // A delegated call names its acting server in the envelope, not the detail.
        assert_eq!(
            serde_json::to_value(ToolAdmission::default()).unwrap(),
            serde_json::json!({})
        );
    }

    #[test]
    fn every_class_belongs_to_the_mcp_owner() {
        for class in [
            Discovery::CLASS,
            ToolAdmission::CLASS,
            ToolCompletion::CLASS,
            Read::CLASS,
        ] {
            assert!(class.starts_with("mcp."), "{class}");
        }
    }

    #[test]
    fn targets_are_veoveo_uris_whose_components_the_builder_encodes() {
        let target = |target: McpTarget| target.audit_target().unwrap().as_str().to_owned();
        assert_eq!(
            target(McpTarget::Tool {
                server: server(),
                tool: LocalToolName::parse("create_event").unwrap(),
            }),
            "veoveo://servers/time/tools/create_event"
        );
        assert_eq!(
            target(McpTarget::Discovery {
                server: None,
                collection: DiscoveryKind::ResourceTemplates,
            }),
            "veoveo://catalog/resource_templates"
        );
        assert_eq!(
            target(McpTarget::Discovery {
                server: Some(server()),
                collection: DiscoveryKind::Tools,
            }),
            "veoveo://servers/time/catalog/tools"
        );
        assert_eq!(
            target(McpTarget::Prompt {
                server: server(),
                prompt: PromptName::parse("plan_window").unwrap(),
            }),
            "veoveo://servers/time/prompts/plan_window"
        );
        // A template's reserved characters are percent-encoded within its segment.
        let template = target(McpTarget::ResourceTemplate {
            server: server(),
            uri: ResourceTemplateUri::new("time://events/{event}").unwrap(),
        });
        assert_eq!(
            template,
            "veoveo://servers/time/resource-templates/time:%2F%2Fevents%2F%7Bevent%7D"
        );
    }
}
