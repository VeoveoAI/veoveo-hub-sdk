//! Signed request metadata for current policy checks; contains no bearer token.
use super::*;
use crate::{AccessTokenSubject, PrincipalKind};
use veoveo_types::{InvocationMode, InvocationProvenance};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HubRequestContext {
    /// Hub-admitted immutable routing selection; not business data authorization.
    pub execution: crate::ExecutionContext,
    /// Hub-established HTTP correlation, authenticated by the internal assertion.
    pub audit: veoveo_audit_contract::AuditRequest,
    pub access_token: AccessTokenSubject,
    /// JWT-verified source principal before delegated actor derivation.
    pub principal: Principal,
}

impl HubRequestContext {
    pub fn validate_for(
        &self,
        actor: &Principal,
        authority: &InvocationAuthority,
    ) -> Result<(), InternalTokenError> {
        let source = &self.principal;
        let token = &self.access_token;
        let common = token.issuer == source.issuer
            && token.subject == source.subject
            && token.scopes == source.scopes
            && token.work_context == authority.work_context
            && source.tenant.as_ref() == Some(&authority.tenant)
            && actor.tenant == source.tenant
            && token.invocation_mode == authority.provenance.mode();
        match &authority.provenance {
            InvocationProvenance::Direct { initiator } => {
                if actor != source {
                    return Err(InternalTokenError::InvalidRequestContext);
                }
                if initiator != &source.id {
                    return Err(InternalTokenError::InvalidRequestContext);
                }
                if !(match &token.initiator {
                    Some(actual) => actual == initiator,
                    None => false,
                }) {
                    return Err(InternalTokenError::InvalidRequestContext);
                }
                if token.delegation_id.is_some() {
                    return Err(InternalTokenError::InvalidRequestContext);
                }
            }
            InvocationProvenance::Automated => {
                if actor != source {
                    return Err(InternalTokenError::InvalidRequestContext);
                }
                if source.kind != PrincipalKind::Service {
                    return Err(InternalTokenError::InvalidRequestContext);
                }
                if source.subject.as_str() != token.oauth_client_id.as_str() {
                    return Err(InternalTokenError::InvalidRequestContext);
                }
                if token.initiator.is_some() {
                    return Err(InternalTokenError::InvalidRequestContext);
                }
                if token.delegation_id.is_some() {
                    return Err(InternalTokenError::InvalidRequestContext);
                }
                if token.session_family.is_some() {
                    return Err(InternalTokenError::InvalidRequestContext);
                }
            }
            InvocationProvenance::Delegated {
                initiator,
                delegation_id,
            } => {
                if source.kind != PrincipalKind::User {
                    return Err(InternalTokenError::InvalidRequestContext);
                }
                if initiator != &source.id {
                    return Err(InternalTokenError::InvalidRequestContext);
                }
                if !(match &token.initiator {
                    Some(actual) => actual == initiator,
                    None => false,
                }) {
                    return Err(InternalTokenError::InvalidRequestContext);
                }
                if !(match &token.delegation_id {
                    Some(actual) => actual == delegation_id,
                    None => false,
                }) {
                    return Err(InternalTokenError::InvalidRequestContext);
                }
                if actor.kind != PrincipalKind::Service {
                    return Err(InternalTokenError::InvalidRequestContext);
                }
                if actor.id.as_str() != format!("{}#{}", token.issuer, token.oauth_client_id) {
                    return Err(InternalTokenError::InvalidRequestContext);
                }
                if actor.issuer != token.issuer {
                    return Err(InternalTokenError::InvalidRequestContext);
                }
                if actor.subject.as_str() != token.oauth_client_id.as_str() {
                    return Err(InternalTokenError::InvalidRequestContext);
                }
                if actor.scopes != source.scopes {
                    return Err(InternalTokenError::InvalidRequestContext);
                }
                if actor.data_labels != source.data_labels {
                    return Err(InternalTokenError::InvalidRequestContext);
                }
                if actor.assurances != source.assurances {
                    return Err(InternalTokenError::InvalidRequestContext);
                }
                if !actor.groups.is_empty() {
                    return Err(InternalTokenError::InvalidRequestContext);
                }
                if !actor.group_roles.is_empty() {
                    return Err(InternalTokenError::InvalidRequestContext);
                }
                if !actor.roles.is_empty() {
                    return Err(InternalTokenError::InvalidRequestContext);
                }
                if token.session_family.is_some() {
                    return Err(InternalTokenError::InvalidRequestContext);
                }
            }
        }
        if !common
            || (token.session_family.is_some()
                && (source.kind != PrincipalKind::User
                    || token.invocation_mode != InvocationMode::Direct))
        {
            return Err(InternalTokenError::InvalidRequestContext);
        }
        Ok(())
    }
}

impl HubRequestContext {
    /// Derive audit attribution only after checking the signed invocation relationship.
    /// The record's actor is the principal the call acts for, in its tenant and Work
    /// Context; `via` names the server that made the call for it, if another did.
    pub fn audit_context(
        &self,
        actor: &Principal,
        authority: &InvocationAuthority,
        via: Option<veoveo_audit_contract::AuditVia>,
    ) -> Result<veoveo_audit_contract::AuditContext, InternalTokenError> {
        use veoveo_audit_contract::*;
        self.validate_for(actor, authority)?;
        Ok(AuditContext {
            actor: AuditActor {
                principal: actor.id.clone(),
                kind: match actor.kind {
                    PrincipalKind::User => AuditPrincipalKind::User,
                    PrincipalKind::Service => AuditPrincipalKind::Service,
                },
                tenant: actor.tenant.clone(),
                work_context: Some(authority.work_context.clone()),
            },
            via,
            request: self.audit.clone(),
        })
    }

    /// The invocation facts a record's detail carries beside the envelope: the OAuth
    /// client, the principal that delegated to the actor, the policy revision, and the
    /// actor's scopes and data labels.
    pub fn audit_invocation(
        &self,
        actor: &Principal,
        authority: &InvocationAuthority,
    ) -> Result<crate::audit::AuditInvocation, InternalTokenError> {
        self.validate_for(actor, authority)?;
        Ok(crate::audit::AuditInvocation {
            oauth_client: self.access_token.oauth_client_id.clone(),
            delegating_principal: (self.principal.id != actor.id)
                .then(|| self.principal.id.clone()),
            policy_revision: authority.policy_revision.clone(),
            scopes: actor.scopes.clone(),
            data_labels: actor.data_labels.clone(),
        })
    }
}

impl HubInternalIdentity {
    /// The audit attribution of this identity's request: its actor, the acting server
    /// and Task when another server made the call, and the request.
    pub fn audit_context(&self) -> Result<veoveo_audit_contract::AuditContext, InternalTokenError> {
        let via = self
            .acting_server()
            .map(|server| veoveo_audit_contract::AuditVia {
                server: server.clone(),
                task: self
                    .acting_task()
                    .and_then(|task| veoveo_types::TaskId::parse(task.as_str()).ok()),
            });
        self.request_context
            .as_ref()
            .ok_or(InternalTokenError::InvalidRequestContext)?
            .audit_context(&self.actor, &self.authority, via)
    }

    /// The invocation facts of this identity's request, for a record's detail.
    pub fn audit_invocation(&self) -> Result<crate::audit::AuditInvocation, InternalTokenError> {
        self.request_context
            .as_ref()
            .ok_or(InternalTokenError::InvalidRequestContext)?
            .audit_invocation(&self.actor, &self.authority)
    }
}
