//! What Ledger and Relay share: the well-known documents and contract addresses the host
//! serves, an empty scope vocabulary, and the permission check each tool runs.
use rmcp::{
    ErrorData, RoleServer,
    model::{Implementation, ServerCapabilities, ServerConfig},
    service::RequestContext,
};
use veoveo_hub_sdk::{HubInternalIdentity, docs::ServerDocs, hosting::hub_identity};
use veoveo_types::{PermissionName, Vocabulary};

/// A server without scopes: its tools check permissions instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, veoveo_types::Vocabulary)]
#[vocabulary(scope)]
pub enum NoScope {}

/// The documents of a calls fixture server, with its typed server identity.
pub(crate) fn docs(server: &'static str) -> ServerDocs {
    ServerDocs::new(veoveo_types::ServerSlug::parse(server).unwrap())
        .with_doc(
            "agents",
            "Agent manual",
            include_str!("../testdata/calls-agents.md"),
        )
        .with_doc("design", "Design", "# Calls Fixture Design")
}

pub(crate) fn contract_config(name: &str, capabilities: ServerCapabilities) -> ServerConfig {
    let mut config = ServerConfig::new(capabilities);
    config.server_info = Implementation::new(name, env!("CARGO_PKG_VERSION"));
    config
}

/// What a calls fixture server reports about the call that reached it.
pub(crate) fn inspection(identity: &HubInternalIdentity) -> crate::calls::Inspection {
    crate::calls::Inspection {
        caller: identity.actor.id.to_string(),
        client: identity.client.to_string(),
        acting_server: identity.acting_server().map(ToString::to_string),
        acting_task: identity.acting_task().map(ToString::to_string),
        prior_servers: identity
            .delegation_context
            .as_ref()
            .map(|context| {
                context
                    .actors
                    .iter()
                    .map(|actor| actor.server.to_string())
                    .collect()
            })
            .unwrap_or_default(),
        originating_task: identity
            .delegating_task()
            .map(|task| (task.server.to_string(), task.task.to_string())),
        permissions: identity
            .permissions
            .iter()
            .map(ToString::to_string)
            .collect(),
    }
}

/// The forwarded identity of this request, when the Hub granted `permission`.
pub(crate) fn require<P: Vocabulary>(
    context: &RequestContext<RoleServer>,
    permission: P,
) -> Result<HubInternalIdentity, ErrorData> {
    let identity = hub_identity(context)?;
    let permission = PermissionName::parse(permission.as_str())
        .map_err(|_| ErrorData::internal_error("invalid permission spelling", None))?;
    if !identity.grants(&permission) {
        return Err(ErrorData::invalid_request(
            format!("the call does not carry {permission}"),
            None,
        ));
    }
    Ok(identity)
}
