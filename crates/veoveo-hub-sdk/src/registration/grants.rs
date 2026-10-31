//! The permissions the Hub grants for one request, carried as an RFC 9396
//! authorization details entry in the RFC 9068 access token it forwards to a server.
use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use veoveo_types::{PermissionName, ServerSlug};

use crate::registration::DeclarationError;

/// The RFC 9396 `type` of [`PermissionsDetail`].
pub const PERMISSIONS_DETAIL_TYPE: &str = "https://veoveo.ai/oauth/permissions";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, veoveo_types::Vocabulary)]
pub enum PermissionsDetailType {
    #[vocabulary(rename = "https://veoveo.ai/oauth/permissions")]
    Permissions,
}

/// `{"type": "https://veoveo.ai/oauth/permissions", "actions": ["time.read"]}`. The
/// RFC 9396 `actions` member lists the granted permissions, all in the namespace of
/// the token's audience.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PermissionsDetail {
    #[serde(rename = "type")]
    pub kind: PermissionsDetailType,
    pub actions: BTreeSet<PermissionName>,
}

impl PermissionsDetail {
    pub fn new(actions: BTreeSet<PermissionName>) -> Self {
        Self {
            kind: PermissionsDetailType::Permissions,
            actions,
        }
    }

    /// Every action must belong to `audience`'s namespace.
    pub fn check(&self, audience: &ServerSlug) -> Result<(), DeclarationError> {
        match self
            .actions
            .iter()
            .find(|permission| !permission.is_owned_by(audience))
        {
            Some(permission) => Err(DeclarationError::PermissionOutsideNamespace {
                permission: permission.clone(),
            }),
            None => Ok(()),
        }
    }
}

/// The RFC 9396 `type` of [`RepositoryDetail`].
pub const REPOSITORY_DETAIL_TYPE: &str = "https://veoveo.ai/oauth/repository";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, veoveo_types::Vocabulary)]
pub enum RepositoryDetailType {
    #[vocabulary(rename = "https://veoveo.ai/oauth/repository")]
    Repository,
}

/// A Lore repository's ID in its canonical form: 32 lowercase hexadecimal digits.
#[veoveo_types::id(text(RepositoryIds))]
pub struct RepositoryId(String);

#[doc(hidden)]
pub struct RepositoryIds;
impl veoveo_types::IdProfile for RepositoryIds {
    type Error = veoveo_types::IdentifierError;
    const PROFILE: veoveo_types::IdProfileSpec<Self::Error> =
        veoveo_types::IdProfileSpec::text(|value, _| {
            if value.len() == 32
                && value
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                Ok(())
            } else {
                Err(veoveo_types::IdentifierError::new(
                    value,
                    "a repository ID is 32 lowercase hexadecimal digits",
                ))
            }
        });
}

/// One of Lore's actions on a repository, as Lore names it, such as `write`, `owner` or
/// `push-protected`: 1 to 64 of `a`–`z` and `-`, starting with a letter. Lore checks
/// these; the SDK carries them without a closed vocabulary, so a new Lore action passes
/// through.
#[veoveo_types::id(text(RepositoryActions))]
pub struct RepositoryAction(String);

#[doc(hidden)]
pub struct RepositoryActions;
impl veoveo_types::IdProfile for RepositoryActions {
    type Error = veoveo_types::IdentifierError;
    const PROFILE: veoveo_types::IdProfileSpec<Self::Error> =
        veoveo_types::IdProfileSpec::text(|value, _| {
            let mut bytes = value.bytes();
            if value.len() <= 64
                && bytes.next().is_some_and(|b| b.is_ascii_lowercase())
                && bytes.all(|b| b.is_ascii_lowercase() || b == b'-')
            {
                Ok(())
            } else {
                Err(veoveo_types::IdentifierError::new(
                    value,
                    "a repository action is 1 to 64 of a-z and -, starting with a letter",
                ))
            }
        });
}

/// The caller's grant on the linked repository a runner serves: the repository and the
/// caller's Lore actions on it, the set a Lore token for that repository would carry. An
/// empty set is a grant that reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RepositoryGrant {
    pub repository: RepositoryId,
    pub actions: BTreeSet<RepositoryAction>,
}

impl RepositoryGrant {
    /// Whether the grant holds `action`.
    pub fn allows(&self, action: &RepositoryAction) -> bool {
        self.actions.contains(action)
    }
}

/// `{"type": "https://veoveo.ai/oauth/repository", "identifier": "<repository ID>",
/// "actions": ["write"]}`: the RFC 9396 entry a forwarded token for a runner carries
/// beside its permissions, with the caller's [`RepositoryGrant`] on the runner's linked
/// repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RepositoryDetail {
    #[serde(rename = "type")]
    pub kind: RepositoryDetailType,
    pub identifier: RepositoryId,
    pub actions: BTreeSet<RepositoryAction>,
}

impl RepositoryDetail {
    pub fn new(grant: RepositoryGrant) -> Self {
        Self {
            kind: RepositoryDetailType::Repository,
            identifier: grant.repository,
            actions: grant.actions,
        }
    }

    /// The grant the entry carries.
    pub fn grant(&self) -> RepositoryGrant {
        RepositoryGrant {
            repository: self.identifier.clone(),
            actions: self.actions.clone(),
        }
    }
}
