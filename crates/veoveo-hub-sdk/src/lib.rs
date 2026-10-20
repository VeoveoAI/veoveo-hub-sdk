//! Veoveo's Rust SDK for MCP servers and clients.
//!
//! The `contract` feature holds the declarations and wire types a server and its
//! consumers share: the forwarded identity, registration and authorization declarations,
//! the Hub's client-visible metadata, the embedded document contract and the MCP
//! audit vocabulary. The `client` feature sends typed calls to servers through a Hub
//! profile. The default `server` feature adds hosting over rmcp, the Streamable HTTP
//! transport profile, forwarded-token verification, the registration client, MCP Apps,
//! the MCP Tasks adapter over core's durable Task runtime and calls to other servers for
//! the current caller.

/// The Veoveo hosted MCP server contract identifier.
pub const HOSTED_MCP_CONTRACT: &str = "https://veoveo.ai/contracts/hosted-mcp";

pub mod audit;
pub mod calls;
pub mod definition;
pub mod docs;
pub mod execution;
pub use veoveo_hub_sdk_macros::{
    SingleOutput, collection_resource, collection_tool, resource_read, tool,
};
pub mod hub;
pub mod identity;
pub mod jsonrpc_codes;
pub mod registration;
pub mod usage;

extern crate self as veoveo_hub_sdk;

#[cfg(feature = "client")]
pub mod client;

#[cfg(feature = "server")]
pub mod apps;
#[cfg(feature = "server")]
pub mod catalog;
#[cfg(feature = "server")]
pub mod durable_tasks;
#[cfg(feature = "server")]
pub mod host;
#[cfg(feature = "server")]
pub mod hosting;
#[cfg(feature = "server")]
pub mod internal_auth;
#[cfg(feature = "server")]
pub mod pagination;
#[cfg(feature = "server")]
pub mod protocol;
#[cfg(feature = "server")]
pub mod public_endpoint;
#[cfg(feature = "server")]
pub mod server_contract;
#[cfg(feature = "server")]
pub mod subscriptions;
#[cfg(feature = "server")]
pub mod task_completion;
#[cfg(feature = "server")]
pub mod tasks;
#[cfg(feature = "server")]
pub mod telemetry;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
#[cfg(feature = "server")]
pub mod transport;
#[cfg(feature = "server")]
pub mod uri;

pub use execution::{
    CompositionError, CompositionRevision, ENVIRONMENT_HEADER, EnvironmentSelector,
    ExecutionContext, RESOLVED_COMPOSITION_FORMAT, ResolvedComponent, ResolvedComposition,
};
pub use identity::{
    AccessTokenSubject, CanonicalTaskId, GroupMembership, GroupRole, HubProfileId,
    HubRefreshFamilyId, JwtId, LocalToolName, McpMethodName, OpaqueTaskId, Principal,
    PrincipalAssurance, PrincipalKind, PromptName, ProtectedResourceId, role_in_group,
};
pub use veoveo_types::{BuildId, DeploymentId, EnvironmentId};
pub use veoveo_types::{OAuthClientId, ServerSlug, TokenIssuer, TokenSubject};

#[cfg(feature = "server")]
pub use catalog::HubDiscoveryMetadata;
#[cfg(feature = "server")]
pub use host::{
    HostAuthority, host_authority_is_allowed, parse_allowed_host_authority,
    parse_request_host_authority, public_allowed_hosts,
};
#[cfg(feature = "server")]
pub use internal_auth::{
    ACCESS_TOKEN_TYPE, ActingServer, DelegatedCaller, DelegatingTask, DelegationContext,
    ExchangeGrant, ExchangedIdentity, ExchangedTokenVerifier, ForwardedRequest,
    ForwardedTokenVerifier, HUB_ROUTING_REQUIRED, HubInternalIdentity, HubInternalSigningKey,
    HubInternalTokenIssuer, HubInternalTokenVerifier, HubInternalTrustBundle, HubRequestContext,
    INVOCATION_CLAIM, InternalTokenError, IssuedExchangedToken, IssuedHubInternalToken,
    KEY_DOCUMENT_LIMIT, KEY_FETCH_TIMEOUT, KEY_REFRESH_INTERVAL, MAX_DELEGATION_DEPTH,
    TokenAudience,
};
#[cfg(feature = "server")]
pub use pagination::{CatalogPage, PaginationError, paginate};
#[cfg(feature = "server")]
pub use protocol::{
    MAX_BAGGAGE_BYTES, MAX_TRACESTATE_BYTES, PRIVATE_CATALOG_TTL_MS, PRIVATE_RESOURCE_TTL_MS,
    PUBLIC_IMMUTABLE_TTL_MS, final_protocol_versions, private_resource_response,
    sanitized_request_meta, trace_id_from_traceparent,
};
#[cfg(feature = "server")]
pub use public_endpoint::{PublicDeployment, PublicOrigin, ServerPublicEndpoint};
#[cfg(feature = "server")]
pub use subscriptions::{
    ResourceListObservers, ResourceUpdate, SubscriptionHub, accepted_subscription_filter,
    accepted_task_subscription_filter, listen_resources, receive_resource_list_change,
    receive_resource_update, send_resource_update,
};
#[cfg(feature = "server")]
pub use tasks::{
    HUB_TASK_RESOURCE_TEMPLATE, HubTaskStatus, HubTaskStatusDocument, HubTaskStatusKind,
    RELATED_TASK_META_KEY, hub_task_resource_uri, notify_progress, now_utc,
    parse_hub_task_resource_uri, related_task_meta, set_related_task_meta,
};
#[cfg(feature = "server")]
pub use telemetry::{TelemetryGuard, init_server_telemetry};
#[cfg(feature = "server")]
pub use transport::{
    MAX_MCP_RESPONSE_BYTES, bound_serialized_jsonrpc_response,
    canonical_streamable_http_server_config, enforce_serialized_mcp_response,
    stateless_session_manager,
};
#[cfg(feature = "server")]
pub use uri::{ServerResourceUris, parse_server_doc_uri};
