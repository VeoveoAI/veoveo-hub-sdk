//! The access token an RFC 8693 token exchange issues: a registered server presents it
//! at one Gateway profile to call other servers for its own caller.
//!
//! The Gateway signs it with the key that signs forwarded tokens. `aud` is the profile's
//! MCP endpoint, never a server's, so no server admits it as a forwarded token, and its
//! invocation claim names the caller instead of a target server. `client_id` and `act`
//! name the calling server's registration (RFC 8693 §4.1), and the RFC 9396 detail holds
//! the permissions the exchange granted.
use super::*;

/// The caller a delegation continues: what the Gateway authenticated for the call that
/// reached the calling server, or the call that created its Task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DelegatedCaller {
    /// The OAuth client the caller used to reach the Gateway.
    pub client: OAuthClientId,
    pub actor: Principal,
    pub authority: InvocationAuthority,
    pub request_context: GatewayRequestContext,
}

impl DelegatedCaller {
    /// The caller of a forwarded call. A delegated identity names no caller of its own:
    /// an exchanged identity is never exchanged again.
    pub fn of(identity: &GatewayInternalIdentity) -> Result<Self, InternalTokenError> {
        if identity.acting_server.is_some() {
            return Err(InternalTokenError::RedelegatedIdentity);
        }
        let request_context = identity
            .request_context
            .clone()
            .ok_or(InternalTokenError::InvalidRequestContext)?;
        request_context.validate_for(&identity.actor, &identity.authority)?;
        Ok(Self {
            client: identity.client.clone(),
            actor: identity.actor.clone(),
            authority: identity.authority.clone(),
            request_context,
        })
    }
}

/// What one exchange grants: the caller a server acts for on one profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExchangeGrant {
    pub profile: GatewayProfileId,
    /// The profile's MCP endpoint, the token's `aud`.
    pub resource: ResourceIndicator,
    pub caller: DelegatedCaller,
    pub acting_server: ActingServer,
    /// The requested permissions the server declared and the caller holds.
    pub permissions: BTreeSet<PermissionName>,
    /// The calling server's Task this delegation lasts as long as, when the subject was a
    /// Task. The Gateway checks on every use that the Task is still running.
    pub task: Option<String>,
    pub expires_at: DateTime<Utc>,
}

/// A verified exchanged token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExchangedIdentity {
    pub issuer: TokenIssuer,
    pub profile: GatewayProfileId,
    pub resource: ResourceIndicator,
    pub caller: DelegatedCaller,
    pub acting_server: ActingServer,
    pub permissions: BTreeSet<PermissionName>,
    pub task: Option<String>,
    pub jwt_id: JwtId,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
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
    profile: GatewayProfileId,
    acting_server: ServerSlug,
    caller: DelegatedCaller,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    task: Option<String>,
}

impl GatewayInternalTokenIssuer {
    /// Issue the token of one exchange. It ends no later than `grant.expires_at`.
    pub fn issue_exchanged(
        &self,
        grant: ExchangeGrant,
    ) -> Result<IssuedExchangedToken, InternalTokenError> {
        ensure_jwt_crypto_provider();
        // JWT times are whole seconds; the issued identity carries what the token says.
        let whole = |time: DateTime<Utc>| timestamp_to_datetime(time.timestamp(), "iat");
        let now = whole(Utc::now())?;
        let expires_at = whole(grant.expires_at)?;
        if expires_at <= now {
            return Err(InternalTokenError::ExpiredDelegation);
        }
        grant
            .caller
            .request_context
            .validate_for(&grant.caller.actor, &grant.caller.authority)?;
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
            act: PriorActor {
                sub: identity.acting_server.client.clone(),
            },
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

/// Verifies the exchanged tokens one Gateway issued, at one of its profiles.
#[derive(Debug, Clone)]
pub struct ExchangedTokenVerifier {
    issuer: TokenIssuer,
    trust_bundle: GatewayInternalTrustBundle,
}

impl ExchangedTokenVerifier {
    pub fn new(issuer: TokenIssuer, trust_bundle: GatewayInternalTrustBundle) -> Self {
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
        if claims.act.sub != claims.client_id {
            return Err(InternalTokenError::InconsistentClaims);
        }
        invocation
            .caller
            .request_context
            .validate_for(&invocation.caller.actor, &invocation.caller.authority)?;
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
        ResourceIndicator::parse("https://gateway.veoveo.test/mcp/operations").unwrap()
    }

    fn acting() -> ActingServer {
        ActingServer {
            server: ServerSlug::parse("media").unwrap(),
            client: OAuthClientId::parse("media-0123456789abcd").unwrap(),
        }
    }

    /// The caller of a forwarded call to `media`, with the request context the Gateway
    /// signs into every forwarded token.
    fn caller() -> DelegatedCaller {
        let token = issuer().issue(direct_call("media")).unwrap();
        let identity = verifier("media").verify(&token.bearer_token).unwrap();
        DelegatedCaller::of(&identity).unwrap()
    }

    fn grant() -> ExchangeGrant {
        ExchangeGrant {
            profile: GatewayProfileId::parse("operations").unwrap(),
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
        let elsewhere = ResourceIndicator::parse("https://gateway.veoveo.test/mcp/other").unwrap();
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

    #[test]
    fn delegated_identities_are_not_exchanged_again() {
        let mut request = direct_call("artifact");
        request.acting_server = Some(acting());
        let token = issuer().issue(request).unwrap();
        let identity = verifier("artifact").verify(&token.bearer_token).unwrap();
        assert!(matches!(
            DelegatedCaller::of(&identity),
            Err(InternalTokenError::RedelegatedIdentity)
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
