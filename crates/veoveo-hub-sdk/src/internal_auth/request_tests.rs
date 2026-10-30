use super::*;
use chrono::TimeDelta;

#[derive(Deserialize)]
struct Fixture {
    name: String,
    actor: Principal,
    authority: InvocationAuthority,
    request_context: HubRequestContext,
}
/// A request carrying `context`, through the OAuth client its access token names.
fn forwarded_with(
    actor: &Principal,
    authority: InvocationAuthority,
    context: HubRequestContext,
) -> ForwardedRequest {
    ForwardedRequest {
        deployment: tests::deployment_id(),
        profile: HubProfileId::parse("operator").unwrap(),
        server: ServerSlug::parse("computers").unwrap(),
        audience: tests::resource("computers"),
        client: context.access_token.oauth_client_id.clone(),
        actor: actor.clone(),
        authority,
        permissions: Default::default(),
        request_context: Some(context),
        acting_server: None,
        acting_task: None,
        delegation_context: None,
        expires_at: Utc::now() + TimeDelta::seconds(60),
    }
}

fn fixtures() -> Vec<Fixture> {
    serde_json::from_str(include_str!("../../testdata/hub-request-context.json")).unwrap()
}

/// A direct call to `server` as the Hub forwards it, with its signed request
/// context.
pub(super) fn direct_call(server: &str) -> ForwardedRequest {
    let fixture = fixtures().remove(0);
    let mut request = forwarded_with(&fixture.actor, fixture.authority, fixture.request_context);
    request.server = ServerSlug::parse(server).unwrap();
    request.audience = tests::resource(server);
    request
}

#[test]
fn all_invocation_modes_preserve_signed_source_and_session_context() {
    let issuer = tests::issuer();
    let verifier = tests::verifier("computers");
    for fixture in fixtures() {
        let issued = issuer
            .issue(forwarded_with(
                &fixture.actor,
                fixture.authority,
                fixture.request_context.clone(),
            ))
            .unwrap();
        let received = verifier.verify(&issued.bearer_token).unwrap();
        assert_eq!(received.actor, fixture.actor, "{}", fixture.name);
        assert_eq!(
            received.request_context,
            Some(fixture.request_context),
            "{}",
            fixture.name
        );
    }
}

#[test]
fn execution_capture_survives_a_downstream_hop_to_a_different_deployment() {
    let first = tests::issuer()
        .issue(direct_call("first"))
        .unwrap()
        .identity;
    let delegated = DelegatedCaller::of(&first).unwrap();
    let captured = delegated.request_context.execution.clone();
    let mut next = forwarded_with(
        &delegated.actor,
        delegated.authority,
        delegated.request_context,
    );
    next.server = ServerSlug::parse("second").unwrap();
    next.audience = tests::resource("second");
    next.deployment = DeploymentId::new();
    next.acting_server = Some(ActingServer {
        deployment: DeploymentId::parse("019a5b30-0000-7000-8000-00000000bcde").unwrap(),
        server: first.server,
        client: OAuthClientId::parse("first-registration").unwrap(),
    });
    let verifier = HubInternalTokenVerifier::new(
        TokenIssuer::parse(tests::TEST_ISSUER).unwrap(),
        TokenAudience {
            server: next.server.clone(),
            deployment: next.deployment,
            resource: next.audience.clone(),
        },
        tests::trust_bundle(),
    );
    let received = verifier
        .verify(&tests::issuer().issue(next).unwrap().bearer_token)
        .unwrap();
    assert_ne!(first.deployment, received.deployment);
    assert_eq!(received.request_context.unwrap().execution, captured);
}

#[test]
fn same_slug_and_resource_cannot_accept_another_deployments_token() {
    let request = direct_call("computers");
    let mut audience = TokenAudience {
        server: request.server.clone(),
        resource: request.audience.clone(),
        deployment: request.deployment,
    };
    let issued = tests::issuer().issue(request).unwrap();
    audience.deployment = DeploymentId::new();
    let verifier = HubInternalTokenVerifier::new(
        TokenIssuer::parse(tests::TEST_ISSUER).unwrap(),
        audience,
        tests::trust_bundle(),
    );
    assert!(matches!(
        verifier.verify(&issued.bearer_token),
        Err(InternalTokenError::DeploymentMismatch)
    ));
}

#[test]
fn source_expiry_caps_assertions_and_cannot_be_extended_by_a_signed_context() {
    let mut fixture = fixtures().remove(0);
    fixture.request_context.access_token.expires_at = Utc::now() + TimeDelta::seconds(10);
    let key = tests::signing_key();
    let issuer = tests::issuer();
    let verifier = tests::verifier("computers");
    let issue = |context| {
        issuer.issue(forwarded_with(
            &fixture.actor,
            fixture.authority.clone(),
            context,
        ))
    };
    let issued = issue(fixture.request_context.clone()).unwrap();
    assert_eq!(
        issued.identity.expires_at,
        fixture.request_context.access_token.expires_at
    );
    verifier.verify(&issued.bearer_token).unwrap();
    let mut claims = AccessTokenClaims::from_identity(&issued.identity);
    claims.standard.exp += 1;
    let mut header = Header::new(Algorithm::EdDSA);
    header.typ = Some(ACCESS_TOKEN_TYPE.into());
    header.kid = Some(key.key_id().to_owned());
    let forged = encode(
        &header,
        &claims,
        &EncodingKey::from_ed_der(&key.private_key_der),
    )
    .unwrap();
    assert!(matches!(
        verifier.verify(&forged),
        Err(InternalTokenError::InvalidRequestContext)
    ));
    fixture.request_context.access_token.expires_at = Utc::now() - TimeDelta::seconds(1);
    assert!(matches!(
        issue(fixture.request_context),
        Err(InternalTokenError::ExpiredDelegation)
    ));
}

#[test]
fn admitted_task_delegation_outlives_source_expiry_without_renewing_its_deadline() {
    let now = Utc::now();
    let deadline = DateTime::<Utc>::from_timestamp(now.timestamp() + 300, 0).unwrap();
    let mut fixture = fixtures().remove(0);
    fixture.request_context.access_token.expires_at = now - TimeDelta::minutes(5);
    let mut request = forwarded_with(&fixture.actor, fixture.authority, fixture.request_context);
    request.expires_at = deadline + TimeDelta::minutes(5);
    let issuer = tests::issuer();
    assert!(matches!(
        issuer.issue(request.clone()),
        Err(InternalTokenError::ExpiredDelegation)
    ));

    // Hub admits the live Task before this SDK boundary. The synthetic context
    // represents that admission; this test does not establish native Task liveness.
    let origin = ActingServer {
        server: ServerSlug::parse("first").unwrap(),
        deployment: DeploymentId::parse("019a5b30-0000-7000-8000-00000000bcde").unwrap(),
        client: OAuthClientId::parse("first-registration").unwrap(),
    };
    request.acting_server = Some(ActingServer {
        server: ServerSlug::parse("second").unwrap(),
        deployment: DeploymentId::parse("019a5b30-0000-7000-8000-00000000bcdf").unwrap(),
        client: OAuthClientId::parse("second-registration").unwrap(),
    });
    request.delegation_context = Some(DelegationContext {
        task: Some(DelegatingTask {
            server: origin.server.clone(),
            deployment: origin.deployment,
            task: OpaqueTaskId::parse("origin-task").unwrap(),
        }),
        actors: vec![origin],
        expires_at: deadline,
    });
    let expected = request.clone();
    let issued = issuer.issue(request).unwrap();
    assert_eq!(issued.identity.expires_at, deadline);
    let received = tests::verifier("computers")
        .verify(&issued.bearer_token)
        .unwrap();
    assert_eq!(received.expires_at, deadline);
    assert_eq!(received.profile, expected.profile);
    assert_eq!(received.server, expected.server);
    assert_eq!(received.deployment, expected.deployment);
    assert_eq!(received.audience, expected.audience);
    assert_eq!(received.client, expected.client);
    assert_eq!(received.actor, expected.actor);
    assert_eq!(received.authority, expected.authority);
    assert_eq!(received.permissions, expected.permissions);
    assert_eq!(received.request_context, expected.request_context);
    assert_eq!(received.acting_server, expected.acting_server);
    assert_eq!(received.acting_task, expected.acting_task);
    assert_eq!(
        received.delegating_task(),
        expected.delegation_context.as_ref().unwrap().task
    );
    assert_eq!(received.delegation_context, expected.delegation_context);

    // The delegation's own deadline remains binding even though source expiry
    // is no longer the deadline of this admitted continuation.
    let mut ended = expected;
    ended.delegation_context.as_mut().unwrap().expires_at = now - TimeDelta::minutes(2);
    assert!(matches!(
        issuer.issue(ended),
        Err(InternalTokenError::ExpiredDelegation)
    ));
}

#[test]
fn signed_context_rejects_mismatched_actor_tenant_scope_and_provenance() {
    for fixture in fixtures() {
        for mismatch in [
            "subject", "issuer", "tenant", "context", "scope", "mode", "actor",
        ] {
            let mut context = fixture.request_context.clone();
            let mut actor = fixture.actor.clone();
            match mismatch {
                "subject" => {
                    context.access_token.subject = crate::TokenSubject::parse("foreign").unwrap()
                }
                "issuer" => {
                    context.access_token.issuer =
                        TokenIssuer::parse("https://foreign.example").unwrap()
                }
                "tenant" => {
                    context.principal.tenant =
                        Some(veoveo_types::TenantId::parse("foreign").unwrap())
                }
                "context" => {
                    context.access_token.work_context =
                        veoveo_types::WorkContextId::parse("foreign").unwrap()
                }
                "scope" => {
                    context
                        .access_token
                        .scopes
                        .insert(veoveo_types::ScopeName::parse("admin:use").unwrap());
                }
                "mode" => {
                    context.access_token.invocation_mode = if context.access_token.invocation_mode
                        == veoveo_types::InvocationMode::Direct
                    {
                        veoveo_types::InvocationMode::Automated
                    } else {
                        veoveo_types::InvocationMode::Direct
                    }
                }
                _ => actor.id = PrincipalId::parse("foreign").unwrap(),
            }
            assert!(
                context.validate_for(&actor, &fixture.authority).is_err(),
                "{} {mismatch}",
                fixture.name
            );
        }
    }
}

#[test]
fn forwarded_tokens_reject_other_types_and_unknown_attribution() {
    let fixture = fixtures()
        .into_iter()
        .find(|fixture| fixture.name == "automated")
        .unwrap();
    let key = tests::signing_key();
    let issuer = tests::issuer();
    let verifier = tests::verifier("computers");
    let issued = issuer
        .issue(forwarded_with(
            &fixture.actor,
            fixture.authority.clone(),
            fixture.request_context.clone(),
        ))
        .unwrap();
    let mut segments = issued.bearer_token.split('.');
    let issued_header: serde_json::Value =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(segments.next().unwrap()).unwrap()).unwrap();
    assert_eq!(issued_header["typ"], ACCESS_TOKEN_TYPE);
    let original: serde_json::Value =
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(segments.next().unwrap()).unwrap()).unwrap();
    let sign = |header: &Header, claims: &serde_json::Value| {
        encode(
            header,
            claims,
            &EncodingKey::from_ed_der(&key.private_key_der),
        )
        .unwrap()
    };
    let mut header = Header::new(Algorithm::EdDSA);
    header.typ = Some(ACCESS_TOKEN_TYPE.into());
    header.kid = Some(key.key_id().to_owned());
    verifier.verify(&sign(&header, &original)).unwrap();
    // RFC 9068 §4: `at+jwt` or its full media type, and nothing else.
    let mut media_type = header.clone();
    media_type.typ = Some("application/at+jwt".into());
    verifier.verify(&sign(&media_type, &original)).unwrap();
    for typ in [Some("JWT"), Some("application/jwt"), None] {
        let mut other = header.clone();
        other.typ = typ.map(str::to_owned);
        assert!(matches!(
            verifier.verify(&sign(&other, &original)),
            Err(InternalTokenError::UnsupportedTokenType)
        ));
    }
    for variant in 0..5 {
        let mut claims = original.clone();
        let invocation = INVOCATION_CLAIM;
        match variant {
            0 => {
                claims.as_object_mut().unwrap().remove("client_id");
            }
            1 => claims["client_id"] = serde_json::json!("another-client"),
            2 => claims["groups"] = serde_json::json!(["foreign"]),
            3 => {
                claims[invocation]["request_context"]["format"] =
                    serde_json::json!("https://veoveo.ai/formats/hub-request-context")
            }
            _ => {
                claims[invocation]["request_context"]["access_token"]["managed_agent"] =
                    serde_json::json!({"instance":"fixture-agent","generation":1,"epoch":2});
            }
        }
        assert!(
            verifier.verify(&sign(&header, &claims)).is_err(),
            "{variant}"
        );
    }
    let mut value = serde_json::to_value(&fixture.request_context).unwrap();
    value["access_token"]["session_family"] =
        serde_json::json!("019b7b88-7f03-7123-8123-abcdefabcdef");
    let context: HubRequestContext = serde_json::from_value(value).unwrap();
    assert!(
        context
            .validate_for(&fixture.actor, &fixture.authority)
            .is_err()
    );
}

#[test]
fn the_token_names_the_client_the_caller_used() {
    let fixture = fixtures().remove(0);
    let issuer = tests::issuer();
    let verifier = tests::verifier("computers");
    let request = forwarded_with(
        &fixture.actor,
        fixture.authority.clone(),
        fixture.request_context.clone(),
    );
    let identity = verifier
        .verify(&issuer.issue(request.clone()).unwrap().bearer_token)
        .unwrap();
    assert_eq!(
        identity.client,
        fixture.request_context.access_token.oauth_client_id
    );
    let elsewhere = ForwardedRequest {
        client: OAuthClientId::parse("another-client").unwrap(),
        ..request
    };
    assert!(matches!(
        issuer.issue(elsewhere),
        Err(InternalTokenError::InvalidRequestContext)
    ));
}

#[test]
fn audit_attribution_names_the_actor_the_acting_server_and_the_invocation_facts() {
    let fixture = fixtures().remove(0);
    let issuer = tests::issuer();
    let verifier = tests::verifier("computers");
    let direct = verifier
        .verify(
            &issuer
                .issue(forwarded_with(
                    &fixture.actor,
                    fixture.authority.clone(),
                    fixture.request_context.clone(),
                ))
                .unwrap()
                .bearer_token,
        )
        .unwrap();
    let context = direct.audit_context().unwrap();
    assert_eq!(context.actor.principal, fixture.actor.id);
    assert_eq!(context.actor.tenant, fixture.actor.tenant);
    assert_eq!(
        context.actor.work_context.as_ref(),
        Some(&fixture.authority.work_context)
    );
    assert_eq!(context.via, None);
    assert_eq!(context.request, fixture.request_context.audit);
    let invocation = direct.audit_invocation().unwrap();
    assert_eq!(
        invocation.oauth_client,
        fixture.request_context.access_token.oauth_client_id
    );
    assert_eq!(
        invocation.policy_revision,
        fixture.authority.policy_revision
    );
    assert_eq!(invocation.scopes, fixture.actor.scopes);

    // A call another server made from one of its Tasks names both in the envelope.
    let task = veoveo_types::TaskId::new();
    let mut delegated = forwarded_with(
        &fixture.actor,
        fixture.authority.clone(),
        fixture.request_context.clone(),
    );
    delegated.acting_server = Some(ActingServer {
        deployment: DeploymentId::parse("019a5b30-0000-7000-8000-00000000bcde").unwrap(),
        server: ServerSlug::parse("media").unwrap(),
        client: OAuthClientId::parse("media-client").unwrap(),
    });
    delegated.acting_task = Some(OpaqueTaskId::parse(task.to_string()).unwrap());
    let delegated = verifier
        .verify(&issuer.issue(delegated).unwrap().bearer_token)
        .unwrap();
    let via = delegated.audit_context().unwrap().via.unwrap();
    assert_eq!(via.server.as_str(), "media");
    assert_eq!(via.task, Some(task));
}
