//! The access token an RFC 8693 token exchange issues: a registered server presents it
//! at one Hub profile to call other servers for its own caller.
//!
//! The Hub signs it with the key that signs forwarded tokens. `aud` is the profile's
//! MCP endpoint, never a server's, so no server admits it as a forwarded token, and its
//! invocation claim names the caller instead of a target server. `client_id` and `act`
//! name the calling server's registration (RFC 8693 §4.1), and the RFC 9396 detail holds
//! the permissions the exchange granted.
use super::*;

/// Maximum registered server actors in an RFC 8693 chain; the Hub is not counted.
/// Bounds token growth and synchronous recursion through registered services.
pub const MAX_DELEGATION_DEPTH: usize = 8;

/// The originating Task whose live native route constrains a delegated request.
/// Its server remains explicit when a later server is the immediate actor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DelegatingTask {
    pub server: ServerSlug,
    pub task: OpaqueTaskId,
}

/// Signed constraints inherited from a previously delegated forwarded request.
/// Actors are ordered from the first registered caller to the most recent one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DelegationContext {
    pub actors: Vec<ActingServer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<DelegatingTask>,
    pub expires_at: DateTime<Utc>,
}

impl DelegationContext {
    fn validate(&self) -> Result<(), InternalTokenError> {
        if self.actors.is_empty() || self.actors.len() > MAX_DELEGATION_DEPTH {
            return Err(InternalTokenError::DelegationDepthExceeded);
        }
        // Delegated child Tasks cannot acquire delegation, so only the first actor
        // can own the originating Task. A later actor cannot rename its owner.
        if self
            .task
            .as_ref()
            .is_some_and(|task| task.server != self.actors[0].server)
        {
            return Err(InternalTokenError::InconsistentClaims);
        }
        Ok(())
    }
}

pub(super) fn validate_delegation(
    acting: Option<&ActingServer>,
    task: Option<&OpaqueTaskId>,
    context: Option<&DelegationContext>,
    expires_at: DateTime<Utc>,
) -> Result<(), InternalTokenError> {
    if acting.is_none() && (task.is_some() || context.is_some()) {
        return Err(InternalTokenError::InconsistentClaims);
    }
    if let Some(context) = context {
        context.validate()?;
        if context.actors.len() + usize::from(acting.is_some()) > MAX_DELEGATION_DEPTH {
            return Err(InternalTokenError::DelegationDepthExceeded);
        }
        if task.is_some() || expires_at > context.expires_at {
            return Err(InternalTokenError::InconsistentClaims);
        }
    }
    Ok(())
}

fn prior_actors(context: Option<&DelegationContext>, acting: &ActingServer) -> PriorActor {
    let prior = context
        .into_iter()
        .flat_map(|context| &context.actors)
        .fold(None, |act, server| {
            Some(Box::new(PriorActor {
                sub: server.client.clone(),
                act,
            }))
        });
    PriorActor {
        sub: acting.client.clone(),
        act: prior,
    }
}

pub(super) fn forwarded_actors(
    context: Option<&DelegationContext>,
    acting: Option<&ActingServer>,
) -> Option<PriorActor> {
    acting.map(|acting| prior_actors(context, acting))
}

/// The caller a delegation continues: what the Hub authenticated for the call that
/// reached the calling server, or the call that created its Task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DelegatedCaller {
    /// The OAuth client the caller used to reach the Hub.
    pub client: OAuthClientId,
    pub actor: Principal,
    pub authority: InvocationAuthority,
    pub request_context: HubRequestContext,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delegation_context: Option<DelegationContext>,
}

impl DelegatedCaller {
    /// Continue a verified forwarded call, retaining its original caller and every
    /// server actor. Only the addressed server can present this subject at the Hub.
    pub fn of(identity: &HubInternalIdentity) -> Result<Self, InternalTokenError> {
        let request_context = identity
            .request_context
            .clone()
            .ok_or(InternalTokenError::InvalidRequestContext)?;
        request_context.validate_for(&identity.actor, &identity.authority)?;
        validate_delegation(
            identity.acting_server.as_ref(),
            identity.acting_task.as_ref(),
            identity.delegation_context.as_ref(),
            identity.expires_at,
        )?;
        let delegation_context = identity.acting_server.as_ref().map(|acting| {
            let mut actors = identity
                .delegation_context
                .as_ref()
                .map(|context| context.actors.clone())
                .unwrap_or_default();
            actors.push(acting.clone());
            let task = identity
                .delegation_context
                .as_ref()
                .and_then(|context| context.task.clone())
                .or_else(|| {
                    identity.acting_task.clone().map(|task| DelegatingTask {
                        server: acting.server.clone(),
                        task,
                    })
                });
            DelegationContext {
                actors,
                task,
                expires_at: identity.expires_at,
            }
        });
        Ok(Self {
            client: identity.client.clone(),
            actor: identity.actor.clone(),
            authority: identity.authority.clone(),
            request_context,
            delegation_context,
        })
    }
}

/// What one exchange grants: the caller a server acts for on one profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExchangeGrant {
    pub profile: HubProfileId,
    /// The profile's MCP endpoint, the token's `aud`.
    pub resource: ResourceIndicator,
    pub caller: DelegatedCaller,
    pub acting_server: ActingServer,
    /// The requested permissions the server declared and the caller holds.
    pub permissions: BTreeSet<PermissionName>,
    /// The calling server's Task this delegation lasts as long as, when the subject was a
    /// Task. The Hub checks on every use that the Task is still running.
    pub task: Option<OpaqueTaskId>,
    pub expires_at: DateTime<Utc>,
}

/// A verified exchanged token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExchangedIdentity {
    pub issuer: TokenIssuer,
    pub profile: HubProfileId,
    pub resource: ResourceIndicator,
    pub caller: DelegatedCaller,
    pub acting_server: ActingServer,
    pub permissions: BTreeSet<PermissionName>,
    pub task: Option<OpaqueTaskId>,
    pub jwt_id: JwtId,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

impl ExchangedIdentity {
    /// Originating native Task, independent of the immediate server actor.
    pub fn delegating_task(&self) -> Option<DelegatingTask> {
        self.caller
            .delegation_context
            .as_ref()
            .and_then(|context| context.task.clone())
            .or_else(|| {
                self.task.clone().map(|task| DelegatingTask {
                    server: self.acting_server.server.clone(),
                    task,
                })
            })
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct IssuedExchangedToken {
    pub bearer_token: String,
    pub identity: ExchangedIdentity,
}

impl fmt::Debug for IssuedExchangedToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IssuedExchangedToken")
            .field("bearer_token", &"<redacted>")
            .field("identity", &self.identity)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ExchangedClaims {
    iss: TokenIssuer,
    sub: String,
    aud: String,
    client_id: OAuthClientId,
    exp: i64,
    nbf: i64,
    iat: i64,
    jti: JwtId,
    act: PriorActor,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    authorization_details: Vec<PermissionsDetail>,
    #[serde(rename = "https://veoveo.ai/claims/invocation")]
    invocation: ExchangedInvocation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExchangedInvocation {
    profile: HubProfileId,
    acting_server: ServerSlug,
    caller: DelegatedCaller,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    task: Option<OpaqueTaskId>,
}

impl HubInternalTokenIssuer {
    /// Issue the token of one exchange. It ends no later than `grant.expires_at`.
    pub fn issue_exchanged(
        &self,
        grant: ExchangeGrant,
    ) -> Result<IssuedExchangedToken, InternalTokenError> {
        ensure_jwt_crypto_provider();
        // JWT times are whole seconds; the issued identity carries what the token says.
        let whole = |time: DateTime<Utc>| timestamp_to_datetime(time.timestamp(), "iat");
        let now = whole(Utc::now())?;
        let expires_at = grant
            .caller
            .delegation_context
            .as_ref()
            .map_or(grant.expires_at, |context| {
                grant.expires_at.min(context.expires_at)
            });
        let expires_at = whole(expires_at)?;
        if expires_at <= now {
            return Err(InternalTokenError::ExpiredDelegation);
        }
        grant
            .caller
            .request_context
            .validate_for(&grant.caller.actor, &grant.caller.authority)?;
        if grant.caller.client != grant.caller.request_context.access_token.oauth_client_id {
            return Err(InternalTokenError::InvalidRequestContext);
        }
        validate_delegation(
            Some(&grant.acting_server),
            grant.task.as_ref(),
            grant.caller.delegation_context.as_ref(),
            expires_at,
        )?;
        let identity = ExchangedIdentity {
            issuer: self.issuer.clone(),
            profile: grant.profile,
            resource: grant.resource,
            caller: grant.caller,
            acting_server: grant.acting_server,
            permissions: grant.permissions,
            task: grant.task,
            jwt_id: fresh_jwt_id()?,
            issued_at: now,
            expires_at,
        };
        let claims = ExchangedClaims {
            iss: identity.issuer.clone(),
            sub: identity.caller.actor.id.as_str().to_owned(),
            aud: identity.resource.as_str().to_owned(),
            client_id: identity.acting_server.client.clone(),
            exp: identity.expires_at.timestamp(),
            nbf: now.timestamp(),
            iat: now.timestamp(),
            jti: identity.jwt_id.clone(),
            act: prior_actors(
                identity.caller.delegation_context.as_ref(),
                &identity.acting_server,
            ),
            authorization_details: (!identity.permissions.is_empty())
                .then(|| PermissionsDetail::new(identity.permissions.clone()))
                .into_iter()
                .collect(),
            invocation: ExchangedInvocation {
                profile: identity.profile.clone(),
                acting_server: identity.acting_server.server.clone(),
                caller: identity.caller.clone(),
                task: identity.task.clone(),
            },
        };
        Ok(IssuedExchangedToken {
            bearer_token: self.sign(&claims)?,
            identity,
        })
    }
}

/// Verifies the exchanged tokens one Hub issued, at one of its profiles.
#[derive(Debug, Clone)]
pub struct ExchangedTokenVerifier {
    issuer: TokenIssuer,
    trust_bundle: HubInternalTrustBundle,
}

impl ExchangedTokenVerifier {
    pub fn new(issuer: TokenIssuer, trust_bundle: HubInternalTrustBundle) -> Self {
        Self {
            issuer,
            trust_bundle,
        }
    }

    /// Verify a token presented at the profile whose MCP endpoint is `resource`.
    pub fn verify(
        &self,
        bearer_token: &str,
        resource: &ResourceIndicator,
    ) -> Result<ExchangedIdentity, InternalTokenError> {
        let claims: ExchangedClaims = decode_access_token(
            bearer_token,
            &self.issuer,
            resource.as_str(),
            &self.trust_bundle,
        )?;
        if uuid::Uuid::parse_str(claims.jti.as_str())
            .ok()
            .is_none_or(|jti| jti.get_version_num() != 7)
        {
            return Err(InternalTokenError::InvalidTokenId);
        }
        let invocation = claims.invocation;
        if claims.sub != invocation.caller.actor.id.as_str() {
            return Err(InternalTokenError::SubjectPrincipalMismatch);
        }
        let acting_server = ActingServer {
            server: invocation.acting_server.clone(),
            client: claims.client_id.clone(),
        };
        validate_delegation(
            Some(&acting_server),
            invocation.task.as_ref(),
            invocation.caller.delegation_context.as_ref(),
            timestamp_to_datetime(claims.exp, "exp")?,
        )?;
        if claims.act
            != prior_actors(
                invocation.caller.delegation_context.as_ref(),
                &acting_server,
            )
        {
            return Err(InternalTokenError::InconsistentClaims);
        }
        invocation
            .caller
            .request_context
            .validate_for(&invocation.caller.actor, &invocation.caller.authority)?;
        if invocation.caller.client
            != invocation
                .caller
                .request_context
                .access_token
                .oauth_client_id
        {
            return Err(InternalTokenError::InvalidRequestContext);
        }
        let permissions = match claims.authorization_details.as_slice() {
            [] => BTreeSet::new(),
            [detail] => detail.actions.clone(),
            _ => return Err(InternalTokenError::InconsistentClaims),
        };
        Ok(ExchangedIdentity {
            issuer: claims.iss,
            profile: invocation.profile,
            resource: resource.clone(),
            caller: invocation.caller,
            acting_server: ActingServer {
                server: invocation.acting_server,
                client: claims.client_id,
            },
            permissions,
            task: invocation.task,
            jwt_id: claims.jti,
            issued_at: timestamp_to_datetime(claims.iat, "iat")?,
            expires_at: timestamp_to_datetime(claims.exp, "exp")?,
        })
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;

    use super::super::request_tests::direct_call;
    use super::super::tests::{forwarded, issuer, verifier};
    use super::*;
    use crate::hosting::testing::{TEST_ISSUER, trust_bundle};

    fn profile_resource() -> ResourceIndicator {
        ResourceIndicator::parse("https://hub.veoveo.test/mcp/operations").unwrap()
    }

    fn acting() -> ActingServer {
        ActingServer {
            server: ServerSlug::parse("media").unwrap(),
            client: OAuthClientId::parse("media-0123456789abcd").unwrap(),
        }
    }

    /// The caller of a forwarded call to `media`, with the request context the Hub
    /// signs into every forwarded token.
    fn caller() -> DelegatedCaller {
        let token = issuer().issue(direct_call("media")).unwrap();
        let identity = verifier("media").verify(&token.bearer_token).unwrap();
        DelegatedCaller::of(&identity).unwrap()
    }

    fn grant() -> ExchangeGrant {
        ExchangeGrant {
            profile: HubProfileId::parse("operations").unwrap(),
            resource: profile_resource(),
            caller: caller(),
            acting_server: acting(),
            permissions: BTreeSet::from([
                PermissionName::parse("artifact.artifacts.write").unwrap()
            ]),
            task: None,
            expires_at: Utc::now() + TimeDelta::seconds(60),
        }
    }

    fn exchanged_verifier() -> ExchangedTokenVerifier {
        ExchangedTokenVerifier::new(TokenIssuer::parse(TEST_ISSUER).unwrap(), trust_bundle())
    }

    #[test]
    fn exchanged_tokens_name_the_caller_and_the_acting_server_at_one_profile() {
        let issued = issuer().issue_exchanged(grant()).unwrap();
        assert!(!format!("{issued:?}").contains(&issued.bearer_token));
        let identity = exchanged_verifier()
            .verify(&issued.bearer_token, &profile_resource())
            .unwrap();
        assert_eq!(identity, issued.identity);
        assert_eq!(identity.acting_server, acting());
        assert_eq!(identity.caller, caller());

        let claims: serde_json::Value = serde_json::from_slice(
            &URL_SAFE_NO_PAD
                .decode(issued.bearer_token.split('.').nth(1).unwrap())
                .unwrap(),
        )
        .unwrap();
        assert_eq!(claims["client_id"], "media-0123456789abcd");
        assert_eq!(
            claims["act"],
            serde_json::json!({"sub": "media-0123456789abcd"})
        );
        assert_eq!(claims["aud"], profile_resource().as_str());
        assert_eq!(
            claims["authorization_details"][0]["actions"],
            serde_json::json!(["artifact.artifacts.write"])
        );

        // Another profile's endpoint, or a server's, refuses it.
        let elsewhere = ResourceIndicator::parse("https://hub.veoveo.test/mcp/other").unwrap();
        assert!(
            exchanged_verifier()
                .verify(&issued.bearer_token, &elsewhere)
                .is_err()
        );
        assert!(verifier("artifact").verify(&issued.bearer_token).is_err());
        // A forwarded token is no exchanged token.
        let forwarded = issuer().issue(forwarded("media", &[])).unwrap();
        assert!(
            exchanged_verifier()
                .verify(
                    &forwarded.bearer_token,
                    &crate::internal_auth::tests::resource("media")
                )
                .is_err()
        );
    }

    fn forwarded_exchange(
        identity: &ExchangedIdentity,
        destination: &str,
    ) -> IssuedHubInternalToken {
        let mut request = direct_call(destination);
        request.client = identity.caller.client.clone();
        request.actor = identity.caller.actor.clone();
        request.authority = identity.caller.authority.clone();
        request.request_context = Some(identity.caller.request_context.clone());
        request.acting_server = Some(identity.acting_server.clone());
        request.acting_task = identity.task.clone();
        request.delegation_context = identity.caller.delegation_context.clone();
        request.expires_at = identity.expires_at;
        issuer().issue(request).unwrap()
    }

    fn next_grant(previous: &ExchangedIdentity, destination: &str) -> ExchangeGrant {
        let forwarded = forwarded_exchange(previous, destination);
        let identity = verifier(destination)
            .verify(&forwarded.bearer_token)
            .unwrap();
        let mut next = grant();
        next.caller = DelegatedCaller::of(&identity).unwrap();
        next.acting_server = ActingServer {
            server: ServerSlug::parse(destination).unwrap(),
            client: OAuthClientId::parse(format!("{destination}-0123456789abcd")).unwrap(),
        };
        next.expires_at = Utc::now() + TimeDelta::hours(1);
        next
    }

    fn claims(token: &str) -> serde_json::Value {
        serde_json::from_slice(
            &URL_SAFE_NO_PAD
                .decode(token.split('.').nth(1).unwrap())
                .unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn nested_requests_preserve_original_caller_history_and_originating_task() {
        let mut initial = grant();
        initial.task = Some(OpaqueTaskId::parse("origin-task").unwrap());
        let first = issuer().issue_exchanged(initial).unwrap();
        let second = issuer()
            .issue_exchanged(next_grant(&first.identity, "ledger"))
            .unwrap();
        let identity = exchanged_verifier()
            .verify(&second.bearer_token, &profile_resource())
            .unwrap();
        assert_eq!(identity.caller.client, first.identity.caller.client);
        assert_eq!(identity.caller.actor, first.identity.caller.actor);
        assert_eq!(identity.caller.authority, first.identity.caller.authority);
        assert_eq!(
            identity.caller.request_context,
            first.identity.caller.request_context
        );
        assert_eq!(
            identity.delegating_task(),
            Some(DelegatingTask {
                server: acting().server,
                task: first.identity.task.unwrap()
            })
        );
        assert_eq!(identity.task, None);
        assert_eq!(identity.expires_at, first.identity.expires_at);
        let token = forwarded_exchange(&identity, "observatory");
        let forwarded = verifier("observatory").verify(&token.bearer_token).unwrap();
        assert_eq!(forwarded.acting_server().unwrap().as_str(), "ledger");
        assert_eq!(forwarded.acting_task(), None);
        assert_eq!(forwarded.delegating_task(), identity.delegating_task());
        assert_eq!(
            claims(&token.bearer_token)["act"]["act"]["act"]["sub"],
            acting().client.as_str()
        );
        assert_eq!(
            claims(&second.bearer_token)["act"]["act"]["sub"],
            acting().client.as_str()
        );
        assert!(verifier("ledger").verify(&token.bearer_token).is_err());
    }

    #[test]
    fn a_chain_has_eight_registered_actors_and_cannot_renew_its_deadline() {
        let mut initial = grant();
        initial.expires_at = Utc::now() + TimeDelta::seconds(20);
        let mut issued = issuer().issue_exchanged(initial).unwrap();
        let deadline = issued.identity.expires_at;
        for index in 1..MAX_DELEGATION_DEPTH {
            issued = issuer()
                .issue_exchanged(next_grant(&issued.identity, &format!("hop{index}")))
                .unwrap();
            assert_eq!(issued.identity.expires_at, deadline);
            exchanged_verifier()
                .verify(&issued.bearer_token, &profile_resource())
                .unwrap();
        }
        assert!(matches!(
            issuer().issue_exchanged(next_grant(&issued.identity, "excess")),
            Err(InternalTokenError::DelegationDepthExceeded)
        ));
        let mut expired = next_grant(&issued.identity, "expired");
        expired
            .caller
            .delegation_context
            .as_mut()
            .unwrap()
            .expires_at = Utc::now() - TimeDelta::seconds(1);
        assert!(matches!(
            issuer().issue_exchanged(expired),
            Err(InternalTokenError::ExpiredDelegation)
        ));
    }

    #[test]
    fn nested_claims_cannot_forge_history_task_owner_or_expiry() {
        let first = issuer().issue_exchanged(grant()).unwrap();
        let second = issuer()
            .issue_exchanged(next_grant(&first.identity, "ledger"))
            .unwrap();
        let mut altered = claims(&second.bearer_token);
        altered["act"]["act"]["sub"] = serde_json::json!("another-client");
        assert!(
            exchanged_verifier()
                .verify(&issuer().sign(&altered).unwrap(), &profile_resource())
                .is_err()
        );
        let mut altered = claims(&second.bearer_token);
        altered["act"]["unexpected"] = serde_json::json!(true);
        assert!(
            exchanged_verifier()
                .verify(&issuer().sign(&altered).unwrap(), &profile_resource())
                .is_err()
        );
        let mut altered = claims(&second.bearer_token);
        altered["exp"] = serde_json::json!(second.identity.expires_at.timestamp() + 30);
        assert!(
            exchanged_verifier()
                .verify(&issuer().sign(&altered).unwrap(), &profile_resource())
                .is_err()
        );
        let mut altered = claims(&second.bearer_token);
        let actor = altered[INVOCATION_CLAIM]["caller"]["delegation_context"]["actors"][0].clone();
        altered[INVOCATION_CLAIM]["caller"]["delegation_context"]["actors"] =
            serde_json::json!(vec![actor; MAX_DELEGATION_DEPTH]);
        assert!(matches!(
            exchanged_verifier().verify(&issuer().sign(&altered).unwrap(), &profile_resource()),
            Err(InternalTokenError::DelegationDepthExceeded)
        ));
        let mut invalid = next_grant(&first.identity, "ledger");
        invalid.caller.delegation_context.as_mut().unwrap().task = Some(DelegatingTask {
            server: ServerSlug::parse("unrelated").unwrap(),
            task: OpaqueTaskId::parse("some-task").unwrap(),
        });
        assert!(matches!(
            issuer().issue_exchanged(invalid),
            Err(InternalTokenError::InconsistentClaims)
        ));
        let mut invalid = next_grant(&first.identity, "ledger");
        invalid.task = Some(OpaqueTaskId::parse("delegated-child-task").unwrap());
        assert!(matches!(
            issuer().issue_exchanged(invalid),
            Err(InternalTokenError::InconsistentClaims)
        ));
    }

    #[test]
    fn nested_forwarding_caps_deadline_and_checks_the_complete_actor_chain() {
        let first = issuer().issue_exchanged(grant()).unwrap();
        let second = issuer()
            .issue_exchanged(next_grant(&first.identity, "ledger"))
            .unwrap();
        let mut request = direct_call("observatory");
        request.acting_server = Some(second.identity.acting_server.clone());
        request.delegation_context = second.identity.caller.delegation_context.clone();
        request.expires_at = Utc::now() + TimeDelta::hours(1);
        let issued = issuer().issue(request).unwrap();
        let identity = verifier("observatory")
            .verify(&issued.bearer_token)
            .unwrap();
        assert_eq!(identity.expires_at, first.identity.expires_at);
        let mut altered = claims(&issued.bearer_token);
        altered["act"]["act"]["act"]["sub"] = serde_json::json!("unrelated-client");
        assert!(
            verifier("observatory")
                .verify(&issuer().sign(&altered).unwrap())
                .is_err()
        );
        let mut altered = claims(&issued.bearer_token);
        altered[INVOCATION_CLAIM]["delegation_context"]["unexpected"] = serde_json::json!(true);
        assert!(
            verifier("observatory")
                .verify(&issuer().sign(&altered).unwrap())
                .is_err()
        );
        let mut invalid = next_grant(&first.identity, "ledger");
        invalid.caller.client = OAuthClientId::parse("substituted-client").unwrap();
        assert!(matches!(
            issuer().issue_exchanged(invalid),
            Err(InternalTokenError::InvalidRequestContext)
        ));
    }

    #[test]
    fn exchanges_need_a_live_grant() {
        let mut expired = grant();
        expired.expires_at = Utc::now() - TimeDelta::seconds(1);
        assert!(matches!(
            issuer().issue_exchanged(expired),
            Err(InternalTokenError::ExpiredDelegation)
        ));
    }
}
