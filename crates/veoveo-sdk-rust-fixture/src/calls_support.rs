//! What Ledger and Relay share: the well-known documents and contract addresses the host
//! serves, an empty scope vocabulary, and the permission check each tool runs.
use std::marker::PhantomData;

use rmcp::{
    ErrorData, RoleServer,
    model::{Implementation, Resource, ResourceTemplate, ServerCapabilities, ServerConfig},
    service::RequestContext,
};
use veoveo_sdk_rust::{
    GatewayInternalIdentity,
    docs::ServerDocs,
    hosting::gateway_identity,
    server_contract::{McpResource, McpResourceTemplate, McpServerContract, McpSetupError},
};
use veoveo_types::{IdentifierError, PermissionName, ResourceAddress, ResourceUri, Vocabulary};

/// A server without scopes: its tools check permissions instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, veoveo_types::Vocabulary)]
#[vocabulary(scope)]
pub enum NoScope {}

/// The addresses the host serves for a calls fixture server `C`.
pub enum CallsResource<C> {
    Docs,
    Document(&'static str),
    Contract,
    #[doc(hidden)]
    Server(PhantomData<fn() -> C>),
}

// Written out so that the server type `C` needs none of these traits.
impl<C> std::fmt::Debug for CallsResource<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Docs => f.write_str("Docs"),
            Self::Document(id) => f.debug_tuple("Document").field(id).finish(),
            Self::Contract => f.write_str("Contract"),
            Self::Server(_) => f.write_str("Server"),
        }
    }
}
impl<C> Clone for CallsResource<C> {
    fn clone(&self) -> Self {
        match self {
            Self::Docs => Self::Docs,
            Self::Document(id) => Self::Document(id),
            Self::Contract => Self::Contract,
            Self::Server(_) => Self::Server(PhantomData),
        }
    }
}
impl<C> PartialEq for CallsResource<C> {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Docs, Self::Docs)
            | (Self::Contract, Self::Contract)
            | (Self::Server(_), Self::Server(_)) => true,
            (Self::Document(left), Self::Document(right)) => left == right,
            _ => false,
        }
    }
}
impl<C> Eq for CallsResource<C> {}

impl<C: McpServerContract> CallsResource<C> {
    pub(crate) fn declared() -> Result<Vec<McpResource<Self>>, McpSetupError> {
        [
            (Self::Docs, "docs"),
            (Self::Document("agents"), "agents"),
            (Self::Document("design"), "design"),
            (Self::Contract, "contract"),
        ]
        .into_iter()
        .map(|(address, name)| McpResource::new(address, |uri| Resource::new(uri, name)))
        .collect()
    }

    pub(crate) fn templates() -> Result<Vec<McpResourceTemplate>, McpSetupError> {
        let docs = veoveo_sdk_rust::docs::docs_member_template(&C::scheme());
        Ok(vec![McpResourceTemplate::new(docs, |uri| {
            ResourceTemplate::new(uri, "doc")
        })?])
    }
}

impl<C: McpServerContract> ResourceAddress for CallsResource<C> {
    type Error = IdentifierError;
    fn parse(uri: &ResourceUri) -> Result<Self, Self::Error> {
        let scheme = C::scheme();
        let rest = uri
            .as_str()
            .strip_prefix(scheme.as_str())
            .and_then(|rest| rest.strip_prefix("://"))
            .ok_or_else(|| IdentifierError::new(uri.as_str(), "another server's address"))?;
        match rest {
            "docs" => Ok(Self::Docs),
            "docs/agents" => Ok(Self::Document("agents")),
            "docs/design" => Ok(Self::Document("design")),
            "contract" => Ok(Self::Contract),
            _ => Err(IdentifierError::new(
                uri.as_str(),
                "unknown fixture address",
            )),
        }
    }
    fn to_uri(&self) -> Result<ResourceUri, Self::Error> {
        let scheme = C::scheme();
        let path = match self {
            Self::Docs => "docs".to_owned(),
            Self::Document(id) => format!("docs/{id}"),
            Self::Contract => "contract".to_owned(),
            Self::Server(_) => {
                return Err(IdentifierError::new("server", "no address of its own"));
            }
        };
        ResourceUri::new(format!("{scheme}://{path}"))
            .map_err(|_| IdentifierError::new("fixture", "invalid fixture address"))
    }
}

/// The documents of a calls fixture server, with its compliance profile.
pub(crate) fn docs(server: &'static str, profile: &str) -> ServerDocs {
    ServerDocs::new(server)
        .with_doc(
            "agents",
            "Agent manual",
            include_str!("../testdata/calls-agents.md"),
        )
        .with_doc("design", "Design", "# Calls Fixture Design")
        .with_profile_json(profile)
        .expect("the calls fixture profile matches its manual")
}

pub(crate) fn contract_config(name: &str, capabilities: ServerCapabilities) -> ServerConfig {
    let mut capabilities = capabilities;
    capabilities.extensions.get_or_insert_default().insert(
        rmcp::model::TASKS_EXTENSION_ID.to_owned(),
        rmcp::model::JsonObject::new(),
    );
    let mut config = ServerConfig::new(capabilities);
    config.server_info = Implementation::new(name, env!("CARGO_PKG_VERSION"));
    config
}

/// What a calls fixture server reports about the call that reached it.
pub(crate) fn inspection(identity: &GatewayInternalIdentity) -> crate::calls::Inspection {
    crate::calls::Inspection {
        caller: identity.actor.id.to_string(),
        client: identity.client.to_string(),
        acting_server: identity.acting_server().map(ToString::to_string),
        acting_task: identity.acting_task().map(ToString::to_string),
        permissions: identity
            .permissions
            .iter()
            .map(ToString::to_string)
            .collect(),
    }
}

/// The forwarded identity of this request, when the Gateway granted `permission`.
pub(crate) fn require<P: Vocabulary>(
    context: &RequestContext<RoleServer>,
    permission: P,
) -> Result<GatewayInternalIdentity, ErrorData> {
    let identity = gateway_identity(context)?;
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
