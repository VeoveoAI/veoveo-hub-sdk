//! The MCP audit vocabulary. An MCP producer, such as the Gateway, declares these classes
//! and targets in its audit registry under the owner `mcp` and records them through
//! core's audit envelope.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use veoveo_audit_contract::{
    AuditDetailError, AuditDetailType, AuditRegistryBuilder, AuditTargetError, AuditTargetOwner,
    AuditTargetRegistration,
};
use veoveo_types::{ResourceTemplateUri, ServerSlug, Sha256Digest};

use crate::identity::{CanonicalTaskId, LocalToolName, OpaqueTaskId, PromptName};

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

/// A caller listed a catalog: class `mcp.discovery`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Discovery {
    pub collection: DiscoveryKind,
    pub visible: u32,
    pub denied: u32,
    pub visible_digest: Sha256Digest,
}
impl AuditDetailType for Discovery {
    const CLASS: &'static str = "mcp.discovery";
}

/// A tool call was admitted or refused before it reached its server: class
/// `mcp.tool_admission`. `acting_server` names the server that made the call for the
/// caller, when another server made it, and `acting_task` that server's Task, when it
/// called from one.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ToolAdmission {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acting_server: Option<ServerSlug>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acting_task: Option<OpaqueTaskId>,
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
    pub acting_server: Option<ServerSlug>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acting_task: Option<OpaqueTaskId>,
}
impl AuditDetailType for ToolCompletion {
    const CLASS: &'static str = "mcp.tool_completion";
}

/// A caller read a resource, a prompt, completions or a subscription: class `mcp.read`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Read {
    pub method: McpReadMethod,
}
impl AuditDetailType for Read {
    const CLASS: &'static str = "mcp.read";
}

/// Target `{"kind":"tool","server":…,"tool":…}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolTarget {
    pub server: ServerSlug,
    pub tool: LocalToolName,
}

/// Target `{"kind":"resource_template","server":…,"uri":…}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceTemplateTarget {
    pub server: ServerSlug,
    pub uri: ResourceTemplateUri,
}

/// Target `{"kind":"prompt","server":…,"prompt":…}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptTarget {
    pub server: ServerSlug,
    pub prompt: PromptName,
}

/// Target `{"kind":"discovery","server":…,"collection":…}`. A federated listing names no
/// server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryTarget {
    pub server: Option<ServerSlug>,
    pub collection: DiscoveryKind,
}

/// Target `{"kind":"task_route","server":…,"route":…}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskRouteTarget {
    pub server: ServerSlug,
    pub route: CanonicalTaskId,
}

/// The wire shape of every MCP target: a closed object tagged by `kind`.
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum TargetWire {
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
    Discovery {
        server: Option<ServerSlug>,
        collection: DiscoveryKind,
    },
    TaskRoute {
        server: ServerSlug,
        route: CanonicalTaskId,
    },
}

fn wrong_kind<E: serde::de::Error>(expected: &str) -> E {
    E::custom(format_args!(
        "expected an MCP audit target of kind `{expected}`"
    ))
}

impl Serialize for ToolTarget {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        TargetWire::Tool {
            server: self.server.clone(),
            tool: self.tool.clone(),
        }
        .serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for ToolTarget {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match TargetWire::deserialize(deserializer)? {
            TargetWire::Tool { server, tool } => Ok(Self { server, tool }),
            _ => Err(wrong_kind(Self::KIND)),
        }
    }
}
impl Serialize for ResourceTemplateTarget {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        TargetWire::ResourceTemplate {
            server: self.server.clone(),
            uri: self.uri.clone(),
        }
        .serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for ResourceTemplateTarget {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match TargetWire::deserialize(deserializer)? {
            TargetWire::ResourceTemplate { server, uri } => Ok(Self { server, uri }),
            _ => Err(wrong_kind(Self::KIND)),
        }
    }
}
impl Serialize for PromptTarget {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        TargetWire::Prompt {
            server: self.server.clone(),
            prompt: self.prompt.clone(),
        }
        .serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for PromptTarget {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match TargetWire::deserialize(deserializer)? {
            TargetWire::Prompt { server, prompt } => Ok(Self { server, prompt }),
            _ => Err(wrong_kind(Self::KIND)),
        }
    }
}
impl Serialize for DiscoveryTarget {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        TargetWire::Discovery {
            server: self.server.clone(),
            collection: self.collection,
        }
        .serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for DiscoveryTarget {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match TargetWire::deserialize(deserializer)? {
            TargetWire::Discovery { server, collection } => Ok(Self { server, collection }),
            _ => Err(wrong_kind(Self::KIND)),
        }
    }
}
impl Serialize for TaskRouteTarget {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        TargetWire::TaskRoute {
            server: self.server.clone(),
            route: self.route.clone(),
        }
        .serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for TaskRouteTarget {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match TargetWire::deserialize(deserializer)? {
            TargetWire::TaskRoute { server, route } => Ok(Self { server, route }),
            _ => Err(wrong_kind(Self::KIND)),
        }
    }
}

/// Each target's schema is its fields plus the constant `kind` discriminator, closed and
/// self-contained: the audit registry admits no owner definitions beside core's.
fn tagged_schema<F: JsonSchema>(kind: &str) -> schemars::Schema {
    let mut schema = schemars::generate::SchemaSettings::draft2020_12()
        .with(|settings| settings.inline_subschemas = true)
        .into_generator()
        .into_root_schema_for::<F>();
    let object = schema.as_object_mut().expect("target fields are an object");
    object
        .entry("properties")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .expect("target properties")
        .insert(
            "kind".into(),
            serde_json::json!({"type": "string", "const": kind}),
        );
    object
        .entry("required")
        .or_insert_with(|| serde_json::json!([]))
        .as_array_mut()
        .expect("target required fields")
        .push(serde_json::json!("kind"));
    object.insert(
        "additionalProperties".into(),
        serde_json::Value::Bool(false),
    );
    schema
}

#[derive(JsonSchema)]
#[allow(dead_code, reason = "Only the target schemas read these field sets")]
struct ToolFields {
    server: ServerSlug,
    tool: LocalToolName,
}
#[derive(JsonSchema)]
#[allow(dead_code, reason = "Only the target schemas read these field sets")]
struct ResourceTemplateFields {
    server: ServerSlug,
    uri: ResourceTemplateUri,
}
#[derive(JsonSchema)]
#[allow(dead_code, reason = "Only the target schemas read these field sets")]
struct PromptFields {
    server: ServerSlug,
    prompt: PromptName,
}
#[derive(JsonSchema)]
#[allow(dead_code, reason = "Only the target schemas read these field sets")]
struct DiscoveryFields {
    server: Option<ServerSlug>,
    collection: DiscoveryKind,
}
#[derive(JsonSchema)]
#[allow(dead_code, reason = "Only the target schemas read these field sets")]
struct TaskRouteFields {
    server: ServerSlug,
    route: CanonicalTaskId,
}

impl JsonSchema for ToolTarget {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "McpToolTarget".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        tagged_schema::<ToolFields>(Self::KIND)
    }
}
impl JsonSchema for ResourceTemplateTarget {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "McpResourceTemplateTarget".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        tagged_schema::<ResourceTemplateFields>(Self::KIND)
    }
}
impl JsonSchema for PromptTarget {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "McpPromptTarget".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        tagged_schema::<PromptFields>(Self::KIND)
    }
}
impl JsonSchema for DiscoveryTarget {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "McpDiscoveryTarget".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        tagged_schema::<DiscoveryFields>(Self::KIND)
    }
}
impl JsonSchema for TaskRouteTarget {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "McpTaskRouteTarget".into()
    }
    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        tagged_schema::<TaskRouteFields>(Self::KIND)
    }
}

impl AuditTargetOwner for ToolTarget {
    const KIND: &'static str = "tool";
}
impl AuditTargetOwner for ResourceTemplateTarget {
    const KIND: &'static str = "resource_template";
}
impl AuditTargetOwner for PromptTarget {
    const KIND: &'static str = "prompt";
}
impl AuditTargetOwner for DiscoveryTarget {
    const KIND: &'static str = "discovery";
}
impl AuditTargetOwner for TaskRouteTarget {
    const KIND: &'static str = "task_route";
}

/// The registrations an MCP producer holds after [`register`].
#[derive(Clone)]
pub struct McpAuditTargets {
    pub tool: AuditTargetRegistration<ToolTarget>,
    pub resource_template: AuditTargetRegistration<ResourceTemplateTarget>,
    pub prompt: AuditTargetRegistration<PromptTarget>,
    pub discovery: AuditTargetRegistration<DiscoveryTarget>,
    pub task_route: AuditTargetRegistration<TaskRouteTarget>,
}

/// Why the MCP vocabulary could not join a registry.
#[derive(Debug, thiserror::Error)]
pub enum McpAuditError {
    #[error(transparent)]
    Target(#[from] AuditTargetError),
    #[error(transparent)]
    Detail(#[from] AuditDetailError),
}

/// Declare the MCP classes and register the MCP targets in an audit registry.
pub fn register(builder: &mut AuditRegistryBuilder) -> Result<McpAuditTargets, McpAuditError> {
    builder.declare::<Discovery>()?;
    builder.declare::<ToolAdmission>()?;
    builder.declare::<ToolCompletion>()?;
    builder.declare::<Read>()?;
    Ok(McpAuditTargets {
        tool: builder.register_target()?,
        resource_template: builder.register_target()?,
        prompt: builder.register_target()?,
        discovery: builder.register_target()?,
        task_route: builder.register_target()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use veoveo_audit_contract::AuditRegistry;

    #[test]
    fn the_mcp_vocabulary_joins_a_registry_and_round_trips_its_targets() {
        let mut builder = AuditRegistry::builder();
        let targets = register(&mut builder).unwrap();
        let registry = builder.build();
        let tool = ToolTarget {
            server: ServerSlug::parse("time").unwrap(),
            tool: LocalToolName::parse("create_event").unwrap(),
        };
        let target = targets.tool.target(&registry, tool.clone()).unwrap();
        assert_eq!(targets.tool.get(&registry, &target).unwrap(), &tool);
        assert_eq!(
            serde_json::to_value(&tool).unwrap(),
            serde_json::json!({"kind": "tool", "server": "time", "tool": "create_event"})
        );
        let detail = registry
            .detail(&ToolCompletion {
                result: ToolResultKind::Complete,
                duration_ms: 12,
                error_code: None,
                acting_server: None,
                acting_task: None,
            })
            .unwrap();
        assert_eq!(detail.class().as_str(), "mcp.tool_completion");
        // A call another server made from one of its Tasks names the server and the Task.
        let delegated = ToolAdmission {
            acting_server: Some(ServerSlug::parse("media").unwrap()),
            acting_task: Some(OpaqueTaskId::parse("0199d4a1-7c00-7b3e-9f2a-6c1d2e3f4a5b").unwrap()),
        };
        assert_eq!(
            serde_json::to_value(&delegated).unwrap(),
            serde_json::json!({
                "acting_server": "media",
                "acting_task": "0199d4a1-7c00-7b3e-9f2a-6c1d2e3f4a5b"
            })
        );
        let delegated = registry.detail(&delegated).unwrap();
        assert_eq!(delegated.class().as_str(), "mcp.tool_admission");
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
}
