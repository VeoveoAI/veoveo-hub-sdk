//! The verified caller, as the read rule of a PostgreSQL knowledge source receives it.
use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use veoveo_types::{DataLabelId, PrincipalId, TenantId};

use crate::{HubInternalIdentity, identity::GroupMembership};

/// The `caller jsonb` the SDK binds to a source's read rule: the forwarded identity's
/// facts, named as core's store names them. The `*Id` members are the UUIDs core keeps,
/// `identity.tenant.id` and `identity.principal.id`, which a server's own rows name; the
/// `*Key` members are the tenant and principal as tokens name them, which a grant names.
///
/// ```json
/// {"tenantId": "1b3c…", "tenantKey": "tenant-a",
///  "principalId": "8d2e…", "principalKey": "https://idp.example.com#user-1",
///  "groups": [{"group": "engineering", "role": "read"}],
///  "dataLabels": ["pii"], "asOf": "2026-11-03T10:00:00.123456Z"}
/// ```
///
/// `groups` holds every group of the caller with the role it holds there, `read` when the
/// token names none, ordered by group; `dataLabels` the caller's clearance; `asOf` the time
/// the request is decided at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct KnowledgeCaller {
    pub tenant_id: veoveo_store::TenantId,
    pub tenant_key: TenantId,
    pub principal_id: veoveo_store::PrincipalId,
    pub principal_key: PrincipalId,
    pub groups: BTreeSet<GroupMembership>,
    pub data_labels: BTreeSet<DataLabelId>,
    pub as_of: DateTime<Utc>,
}

/// Why a forwarded identity names no stored tenant or principal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the forwarded identity names no tenant or principal core can store")]
pub struct KnowledgeCallerError;

impl KnowledgeCaller {
    /// The caller of a request with `identity`, decided as of `as_of`: the tenant of its
    /// invocation authority and its principal.
    pub fn of(
        identity: &HubInternalIdentity,
        as_of: DateTime<Utc>,
    ) -> Result<Self, KnowledgeCallerError> {
        let tenant = identity.authority.tenant.clone();
        let principal = identity.actor.id.clone();
        let tenant_id = veoveo_store::deterministic_tenant_id(tenant.as_str())
            .map_err(|_| KnowledgeCallerError)?;
        let principal_id =
            veoveo_store::deterministic_principal_id(tenant.as_str(), principal.as_str())
                .map_err(|_| KnowledgeCallerError)?;
        Ok(Self {
            tenant_id,
            tenant_key: tenant,
            principal_id,
            principal_key: principal,
            groups: identity.actor.group_memberships(),
            data_labels: identity.actor.data_labels.clone(),
            as_of,
        })
    }

    /// The JSON value the read rule receives.
    pub fn to_value(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("a knowledge caller serializes")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hosting::testing;
    use serde_json::json;

    fn identity() -> HubInternalIdentity {
        let mut actor = testing::principal();
        actor
            .groups
            .insert(veoveo_types::GroupId::parse("design").unwrap());
        actor.group_roles.insert(GroupMembership {
            group: veoveo_types::GroupId::parse("design").unwrap(),
            role: crate::identity::GroupRole::Write,
        });
        actor.data_labels.insert(DataLabelId::parse("pii").unwrap());
        HubInternalIdentity {
            issuer: veoveo_types::TokenIssuer::parse(testing::TEST_ISSUER).unwrap(),
            profile: crate::HubProfileId::parse("operations").unwrap(),
            server: veoveo_types::ServerSlug::parse("shop").unwrap(),
            deployment: testing::deployment_id(),
            audience: crate::registration::ResourceIndicator::parse(testing::TEST_RESOURCE)
                .unwrap(),
            client: veoveo_types::OAuthClientId::parse(testing::TEST_CLIENT).unwrap(),
            actor,
            authority: testing::authority(),
            permissions: BTreeSet::new(),
            repository: None,
            request_context: None,
            acting_server: None,
            acting_task: None,
            delegation_context: None,
            jwt_id: crate::JwtId::parse(uuid::Uuid::now_v7().to_string()).unwrap(),
            issued_at: Utc::now(),
            not_before: Utc::now(),
            expires_at: Utc::now() + chrono::TimeDelta::minutes(1),
        }
    }

    #[test]
    fn the_caller_carries_core_ids_keys_groups_with_roles_labels_and_its_time() {
        let as_of = "2026-11-03T10:00:00Z".parse().unwrap();
        let caller = KnowledgeCaller::of(&identity(), as_of).unwrap();
        let tenant = veoveo_store::deterministic_tenant_id("tenant-a").unwrap();
        let principal =
            veoveo_store::deterministic_principal_id("tenant-a", "https://idp.example.com#user-1")
                .unwrap();
        assert_eq!(
            caller.to_value(),
            json!({
                "tenantId": tenant,
                "tenantKey": "tenant-a",
                "principalId": principal,
                "principalKey": "https://idp.example.com#user-1",
                "groups": [
                    {"group": "design", "role": "write"},
                    {"group": "engineering", "role": "read"}
                ],
                "dataLabels": ["pii"],
                "asOf": "2026-11-03T10:00:00Z"
            })
        );
        assert_eq!(
            serde_json::from_value::<KnowledgeCaller>(caller.to_value()).unwrap(),
            caller
        );
    }
}
