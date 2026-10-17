//! What a provider publishes for consumers that call it through the Hub: the typed
//! request of each tool, the token exchange that narrows a call to the permissions the
//! consumer declared, and the errors that exchange answers with.
//!
//! A provider's `contract` feature implements [`ProviderContract`] once, [`ToolCall`]
//! for each tool request and [`ResourceRead`] for each resource a consumer reads, so a
//! consumer names a tool or a resource and decodes its result through types, never
//! through strings. The `client` feature sends the calls and reads.
use std::collections::BTreeSet;

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use veoveo_types::{PermissionName, ResourceAddress, ResourceUri, ServerSlug};

use crate::hub::{HubToolName, HubToolNameError};
use crate::identity::LocalToolName;
use crate::registration::PermissionsDetail;

/// A server whose tools other servers call: its registered slug.
pub trait ProviderContract {
    /// The slug the provider registers, such as `artifact`.
    fn slug() -> ServerSlug;
}

/// The request of one provider tool, serialized as the tool's arguments.
pub trait ToolCall: Serialize + Send + Sync + 'static {
    /// The provider whose tool this request calls.
    type Provider: ProviderContract;
    /// The tool's structured result, decoded from `structuredContent`.
    type Output: DeserializeOwned + Send + 'static;
    /// The tool's name on its server, such as `create_upload`.
    const TOOL: &'static str;

    /// The tool's name through the Hub, `{slug}__{tool}`.
    fn hub_tool_name() -> Result<HubToolName, CallContractError> {
        let tool =
            LocalToolName::parse(Self::TOOL).map_err(|_| CallContractError::Tool(Self::TOOL))?;
        Ok(HubToolName::from_parts(&Self::Provider::slug(), &tool)?)
    }
}

/// A tool that runs as a Task. Starting it returns a handle that resolves with the
/// Task's result.
pub trait TaskToolCall: ToolCall {}

/// A read of one provider resource: the provider's typed address and the resource's
/// typed content, which the provider serves as one `application/json` text content.
pub trait ResourceRead: Send + Sync + 'static {
    /// The provider whose resource this read names.
    type Provider: ProviderContract;
    /// The provider's resource address type, which builds the URI.
    type Address: ResourceAddress;
    /// The resource's content, decoded from its JSON text.
    type Content: DeserializeOwned + Send + 'static;

    /// The resource this read names.
    fn address(&self) -> Self::Address;

    /// The resource's URI, built by the provider's address type.
    fn uri(&self) -> Result<ResourceUri, CallContractError> {
        self.address()
            .to_uri()
            .map_err(|error| CallContractError::Address(error.to_string()))
    }
}

/// A provider contract that names an invalid tool or resource.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CallContractError {
    #[error("the contract names an invalid tool `{0}`")]
    Tool(&'static str),
    #[error(transparent)]
    HubName(#[from] HubToolNameError),
    /// The provider's address type could not build the resource's URI.
    #[error("the contract cannot address the resource: {0}")]
    Address(String),
}

/// RFC 8693 §2.1 `grant_type` of a token exchange.
pub const TOKEN_EXCHANGE_GRANT_TYPE: &str = "urn:ietf:params:oauth:grant-type:token-exchange";
/// RFC 8693 §3: an access token, both the forwarded token a server presents as its
/// subject and the token the exchange issues.
pub const ACCESS_TOKEN_TOKEN_TYPE: &str = "urn:ietf:params:oauth:token-type:access_token";
/// A Veoveo token type for a subject that is one of the requesting server's live
/// Tasks, named by the Task's identifier. The Task stands for the caller that created
/// it while it runs.
pub const TASK_TOKEN_TYPE: &str = "https://veoveo.ai/token-types/task";

/// The subject of a token exchange: whom the issued token acts for.
#[derive(Clone, PartialEq, Eq)]
pub enum ExchangeSubject {
    /// The forwarded token the requesting server received for the current call.
    AccessToken(String),
    /// One of the requesting server's live Tasks, by its identifier.
    Task(String),
}

impl ExchangeSubject {
    /// RFC 8693 `subject_token` and `subject_token_type`.
    pub fn parameters(&self) -> (&str, &'static str) {
        match self {
            Self::AccessToken(token) => (token, ACCESS_TOKEN_TOKEN_TYPE),
            Self::Task(task) => (task, TASK_TOKEN_TYPE),
        }
    }
}

impl std::fmt::Debug for ExchangeSubject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AccessToken(_) => f.write_str("AccessToken(<redacted>)"),
            Self::Task(task) => f.debug_tuple("Task").field(task).finish(),
        }
    }
}

/// The RFC 8693 §2.2.1 response to a token exchange, with the RFC 9396 §7 permissions
/// the issued token carries.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenExchangeResponse {
    pub access_token: String,
    pub issued_token_type: String,
    pub token_type: String,
    pub expires_in: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub authorization_details: Vec<PermissionsDetail>,
}

impl TokenExchangeResponse {
    /// The permissions the issued token carries.
    pub fn permissions(&self) -> BTreeSet<PermissionName> {
        self.authorization_details
            .iter()
            .flat_map(|detail| detail.actions.iter().cloned())
            .collect()
    }
}

impl std::fmt::Debug for TokenExchangeResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenExchangeResponse")
            .field("access_token", &"<redacted>")
            .field("issued_token_type", &self.issued_token_type)
            .field("expires_in", &self.expires_in)
            .field("authorization_details", &self.authorization_details)
            .finish()
    }
}

/// The OAuth error codes a token exchange answers with.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum TokenErrorCode {
    /// RFC 6749 §5.2.
    InvalidRequest,
    /// RFC 6749 §5.2: the requesting server did not authenticate.
    InvalidClient,
    /// RFC 6749 §5.2: the subject is not the requesting server's to exchange.
    InvalidGrant,
    /// RFC 6749 §5.2.
    UnauthorizedClient,
    /// RFC 6749 §5.2.
    UnsupportedGrantType,
    /// RFC 6749 §5.2.
    InvalidScope,
    /// RFC 8693 §2.2.2: no registered server provides a requested permission, or the
    /// resource names another profile.
    InvalidTarget,
    /// RFC 9396 §5: a requested permission lies outside the server's declared uses.
    InvalidAuthorizationDetails,
    /// RFC 6749 §5.2 extension for a Hub that cannot judge the request now.
    ServerError,
}

/// An RFC 6749 §5.2 error response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenErrorResponse {
    pub error: TokenErrorCode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_description: Option<String>,
}
