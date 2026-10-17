//! The identity a forwarded access token carries: the caller, their groups and the
//! names a request routes through. Servers read these types; the Hub issues them.
use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use veoveo_types::{
    AccessLevel, DataLabelId, DelegationId, GroupId, OAuthClientId, PrincipalId, RoleId, ScopeName,
    TenantId, TokenIssuer, TokenSubject, WorkContextId,
    identifier_syntax::{ClaimTextProfile, PathIdProfile, TokenTextProfile},
};

/// The role a principal holds *within* a group. It has the same wire values and
/// ordering as a grant level, but remains a distinct domain type so callers
/// cannot accidentally substitute one axis for the other.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum GroupRole {
    Read,
    Write,
    Admin,
}

impl GroupRole {
    /// The access level this role confers.
    pub fn access_level(self) -> AccessLevel {
        match self {
            Self::Read => AccessLevel::Read,
            Self::Write => AccessLevel::Write,
            Self::Admin => AccessLevel::Admin,
        }
    }
}

/// One `(GroupId, GroupRole)` membership. A principal carries a set of these;
/// the pairing is the only genuinely new relationship the sharing feature adds
/// over today's flat `Principal.groups`.
#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct GroupMembership {
    pub group: GroupId,
    pub role: GroupRole,
}

/// The caller's role in `group`, if they are a member.
pub fn role_in_group(
    memberships: &BTreeSet<GroupMembership>,
    group: &GroupId,
) -> Option<GroupRole> {
    memberships
        .iter()
        .find(|m| &m.group == group)
        .map(|m| m.role)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Principal {
    pub id: PrincipalId,
    pub kind: PrincipalKind,
    pub issuer: TokenIssuer,
    pub subject: TokenSubject,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tenant: Option<TenantId>,
    #[serde(default)]
    pub groups: BTreeSet<GroupId>,
    /// Per-group role: the `(GroupId, GroupRole)` pairing that lets a principal
    /// hold different levels in different groups (write in Eng, read in Ops).
    /// A member listed in `groups` without an entry here is treated as `Read`
    /// in that group (see [`Principal::group_memberships`]).
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub group_roles: BTreeSet<GroupMembership>,
    #[serde(default)]
    pub roles: BTreeSet<RoleId>,
    #[serde(default)]
    pub scopes: BTreeSet<ScopeName>,
    #[serde(default)]
    pub data_labels: BTreeSet<DataLabelId>,
    #[serde(default)]
    pub assurances: BTreeSet<PrincipalAssurance>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authenticated_at: Option<DateTime<Utc>>,
}

impl Principal {
    /// The effective `(GroupId, GroupRole)` membership set for access decisions.
    ///
    /// Every group in `groups` yields a membership: its explicit role from
    /// `group_roles` when present, otherwise `Read` (bare membership grants
    /// read-level group access).
    pub fn group_memberships(&self) -> BTreeSet<GroupMembership> {
        let explicit: BTreeMap<_, _> = self
            .group_roles
            .iter()
            .map(|m| (m.group.clone(), m.role))
            .collect();
        self.groups
            .iter()
            .map(|group| GroupMembership {
                group: group.clone(),
                role: explicit.get(group).copied().unwrap_or(GroupRole::Read),
            })
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PrincipalKind {
    User,
    Service,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum PrincipalAssurance {
    UsPerson,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AccessTokenSubject {
    pub issuer: TokenIssuer,
    pub subject: TokenSubject,
    pub oauth_client_id: OAuthClientId,
    /// Durable browser session binding. Absence cannot authorize session-bound renewal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_family: Option<HubRefreshFamilyId>,
    pub audience: ProtectedResourceId,
    pub work_context: WorkContextId,
    pub invocation_mode: veoveo_types::InvocationMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initiator: Option<PrincipalId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delegation_id: Option<DelegationId>,
    #[serde(default)]
    pub scopes: BTreeSet<ScopeName>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub jwt_id: Option<JwtId>,
    pub issued_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_before: Option<DateTime<Utc>>,
    pub expires_at: DateTime<Utc>,
}

/// The Hub profile a request arrived through, exposed under `/mcp/{profile}`.
#[veoveo_types::id(text(PathIdProfile))]
pub struct HubProfileId(String);

/// Canonical UUIDv7 identity of one rotating OAuth refresh-token family, carried as the
/// session binding of a forwarded request.
#[veoveo_types::id(uuid(RefreshFamilyIds))]
pub struct HubRefreshFamilyId(String);

/// OAuth protected-resource URI the caller's access token names.
#[veoveo_types::id(text(ClaimTextProfile))]
pub struct ProtectedResourceId(String);

/// JWT id used for replay protection or revocation tracking.
#[veoveo_types::id(text(ClaimTextProfile))]
pub struct JwtId(String);

/// Tool name as one MCP server exposes it.
#[veoveo_types::id(text(PathIdProfile))]
pub struct LocalToolName(String);

/// Prompt name as one MCP server or Hub profile exposes it.
#[veoveo_types::id(text(PathIdProfile))]
pub struct PromptName(String);

/// MCP JSON-RPC method name.
#[veoveo_types::id(text(TokenTextProfile))]
pub struct McpMethodName(String);

/// The task identifier a server returned, before the Hub routes it.
#[veoveo_types::id(text(TokenTextProfile))]
pub struct OpaqueTaskId(String);

/// Opaque Hub Task route bound to the invocation authority that created it.
#[veoveo_types::id(text(TaskRouteIds))]
pub struct CanonicalTaskId(String);

#[doc(hidden)]
pub struct RefreshFamilyIds;
impl veoveo_types::IdProfile for RefreshFamilyIds {
    type Error = veoveo_types::IdentifierError;
    const PROFILE: veoveo_types::IdProfileSpec<Self::Error> = veoveo_types::IdProfileSpec::uuid(
        veoveo_types::UuidGrammar {
            versions: &[7],
            variant: veoveo_types::UuidVariant::Rfc4122,
            spelling: veoveo_types::UuidSpelling::CanonicalLowerHyphenated,
        },
        refresh_family_error,
    );
}

fn refresh_family_error(
    value: &str,
    _: veoveo_types::IdMetadata,
    failure: veoveo_types::IdFailure,
) -> veoveo_types::IdentifierError {
    veoveo_types::IdentifierError::new(
        value,
        if matches!(failure, veoveo_types::IdFailure::ParseUuid) {
            "must be a UUIDv7"
        } else {
            "must be a canonical RFC UUIDv7"
        },
    )
}

#[doc(hidden)]
pub struct TaskRouteIds;
impl veoveo_types::IdProfile for TaskRouteIds {
    type Error = veoveo_types::IdentifierError;
    const PROFILE: veoveo_types::IdProfileSpec<Self::Error> =
        veoveo_types::IdProfileSpec::text(validate_task_route);
}

fn validate_task_route(
    value: &str,
    _: veoveo_types::IdMetadata,
) -> Result<(), veoveo_types::IdentifierError> {
    veoveo_types::identifier_syntax::validate_token_text(value)?;
    if value.len() > 128 {
        return Err(veoveo_types::IdentifierError::new(
            value,
            "must be at most 128 bytes",
        ));
    }
    Ok(())
}
