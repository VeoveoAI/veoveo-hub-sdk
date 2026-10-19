use std::{collections::BTreeSet, time::Duration};

use axum::{Router, extract::Request, http::StatusCode, middleware::Next, response::IntoResponse};
use rmcp::{
    ClientLifecycleMode, ClientServiceExt,
    model::{ReadResourceRequestParams, ResourceContents},
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
        streamable_http_server::StreamableHttpService,
    },
};
use veoveo_hub_sdk_conformance::{
    ConformanceCredentials, HostedServerConformanceProfile, HostedServerProfileFormat,
    HttpBoundaryProfile, SurfaceExpectation, SurfaceProfile,
    run_hosted_server_conformance_with_evidence,
};
use veoveo_hub_sdk_fixture::{
    contract::{ObservatoryResource, ObservatoryScope, Reading, ReadingId},
    mcp::{FixtureGrants, ObservatoryMcp, SETUP},
};
use veoveo_types::{ResourceAddress, ScopeName};

struct OwnedServer(Option<tokio::task::JoinHandle<()>>);
impl Drop for OwnedServer {
    fn drop(&mut self) {
        if let Some(task) = &self.0 {
            task.abort();
        }
    }
}
impl OwnedServer {
    async fn stop(mut self) {
        if let Some(task) = self.0.take() {
            task.abort();
            let _ = task.await;
        }
    }
}

async fn authenticate(mut request: Request, next: Next) -> axum::response::Response {
    let grants = match request
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
    {
        Some("Bearer fixture-read") => BTreeSet::from([
            ObservatoryScope::Read.into(),
            ScopeName::parse("unrelated:custom").unwrap(),
        ]),
        Some("Bearer fixture-unrelated") => {
            BTreeSet::from([ScopeName::parse("unrelated:custom").unwrap()])
        }
        _ => return StatusCode::UNAUTHORIZED.into_response(),
    };
    request.extensions_mut().insert(FixtureGrants(grants));
    next.run(request).await
}

#[tokio::test]
async fn independent_typed_server_passes_hosted_conformance_and_scope_denial() -> anyhow::Result<()>
{
    tokio::time::timeout(Duration::from_secs(60), qualify()).await??;
    Ok(())
}

async fn qualify() -> anyhow::Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    std::sync::LazyLock::force(&SETUP);
    let service = StreamableHttpService::new(
        || Ok(ObservatoryMcp),
        veoveo_hub_sdk::stateless_session_manager(),
        veoveo_hub_sdk::canonical_streamable_http_server_config(),
    );
    let router = Router::new()
        .nest_service("/observatory/mcp", service)
        .route(
            "/observatory/admin/docs/llms.txt",
            axum::routing::get(
                |axum::Extension(grants): axum::Extension<FixtureGrants>| async move {
                    if !SETUP.has_scope(&grants.0, ObservatoryScope::Read) {
                        return (StatusCode::FORBIDDEN, String::new());
                    }
                    (StatusCode::OK, SETUP.documents().llms_txt())
                },
            ),
        )
        .route(
            "/observatory/admin/docs/{id}",
            axum::routing::get(
                |axum::extract::Path(id): axum::extract::Path<String>,
                 axum::Extension(grants): axum::Extension<FixtureGrants>| async move {
                    if !SETUP.has_scope(&grants.0, ObservatoryScope::Read) {
                        return (StatusCode::FORBIDDEN, String::new());
                    }
                    match SETUP.documents().doc(&id) {
                        Some(doc) => (StatusCode::OK, doc.body.to_owned()),
                        None => (StatusCode::NOT_FOUND, String::new()),
                    }
                },
            ),
        )
        .layer(axum::middleware::from_fn(authenticate));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let server = OwnedServer(Some(tokio::spawn(async move {
        axum::serve(listener, router)
            .await
            .expect("owned fixture listener");
    })));
    let endpoint = format!("http://{address}/observatory/mcp");
    let profile = HostedServerConformanceProfile {
        format: HostedServerProfileFormat::McpConformanceProfile,
        profile_id: "modular-fixture".into(),
        contract: veoveo_hub_sdk_conformance::ContractId::HostedMcp,
        endpoint: endpoint.clone(),
        server_slug: "observatory".into(),
        owned_resource_schemes: BTreeSet::from(["observatory".into()]),
        http: HttpBoundaryProfile {
            require_authentication_rejection: true,
            rejected_host: None,
            health_url: None,
            readiness_url: None,
            docs_llms_url: format!("http://{address}/observatory/admin/docs/llms.txt"),
        },
        surfaces: SurfaceProfile {
            tools: SurfaceExpectation::Forbidden,
            resources: SurfaceExpectation::Required,
            resource_templates: SurfaceExpectation::Required,
            prompts: SurfaceExpectation::Forbidden,
            completions: SurfaceExpectation::Required,
            tasks: SurfaceExpectation::Forbidden,
            subscriptions: SurfaceExpectation::Forbidden,
            required_tools: BTreeSet::new(),
            required_resources: BTreeSet::from(["observatory://readings".into()]),
            required_resource_templates: BTreeSet::from([
                "observatory://reading/{reading_id}".into()
            ]),
            required_prompts: BTreeSet::new(),
        },
    };
    let resource = ObservatoryResource::Reading(ReadingId::new("sensor-a")?).to_uri()?;
    let mut observed_reading = None;
    for (token, allowed) in [("fixture-read", true), ("fixture-unrelated", false)] {
        let client = ()
            .serve_with_lifecycle(
                StreamableHttpClientTransport::from_config(
                    StreamableHttpClientTransportConfig::with_uri(endpoint.clone())
                        .auth_header(token),
                ),
                ClientLifecycleMode::Discover {
                    preferred_versions: vec![rmcp::model::ProtocolVersion::V_2026_07_28],
                },
            )
            .await?;
        let result = client
            .read_resource(ReadResourceRequestParams::new(resource.as_str()))
            .await;
        if allowed {
            let result = result?;
            let text = result
                .contents
                .iter()
                .find_map(|value| match value {
                    ResourceContents::TextResourceContents { text, .. } => Some(text),
                    _ => None,
                })
                .expect("typed reading contents");
            let reading = serde_json::from_str::<Reading>(text)?;
            assert_eq!(
                reading,
                Reading {
                    id: ReadingId::new("sensor-a")?,
                    value: 7
                }
            );
            observed_reading = Some(reading);
            let missing = ObservatoryResource::Reading(ReadingId::new("missing")?).to_uri()?;
            expect_mcp_error(
                client
                    .read_resource(ReadResourceRequestParams::new(missing.as_str()))
                    .await
                    .unwrap_err(),
                // RMCP applies SEP-2164 for the negotiated 2026-07-28 protocol.
                rmcp::model::ErrorCode::INVALID_PARAMS,
            );
        } else {
            expect_mcp_error(result.unwrap_err(), rmcp::model::ErrorCode::INVALID_REQUEST);
            expect_mcp_error(
                client.list_resources(None).await.unwrap_err(),
                rmcp::model::ErrorCode::INVALID_REQUEST,
            );
            expect_mcp_error(
                client.list_resource_templates(None).await.unwrap_err(),
                rmcp::model::ErrorCode::INVALID_REQUEST,
            );
        }
        client.cancel().await?;
    }
    let label = veoveo_types::NamingLabel::new("observatory-reading")?;
    let bodies = [
        veoveo_hub_sdk_conformance::OwnerSchemaEvidence::generated::<Reading>(label.clone())
            .observe(
                veoveo_hub_sdk_conformance::SchemaObservation::from_serializable(
                    &observed_reading.expect("successful authenticated reading"),
                )?,
            ),
    ];
    let evidence = veoveo_hub_sdk_conformance::NamingEvidence {
        required_observations: vec![label],
        bodies: &bodies,
        ..Default::default()
    };
    let report = run_hosted_server_conformance_with_evidence(
        &profile,
        &ConformanceCredentials::bearer("fixture-read"),
        &evidence,
    )
    .await?;
    println!("{}", serde_json::to_string(&report)?);
    assert!(
        !report.passed(),
        "missing domain paging evidence must remain incomplete"
    );
    assert!(
        report.checks.iter().all(|check| {
            if check.requirement_id == "VV-MCP-PAGING-001" {
                check.status == veoveo_hub_sdk_conformance::CheckStatus::Incomplete
            } else {
                !matches!(
                    check.status,
                    veoveo_hub_sdk_conformance::CheckStatus::Failed
                        | veoveo_hub_sdk_conformance::CheckStatus::Incomplete
                )
            }
        }),
        "{:#?}",
        report.checks
    );
    let naming = report
        .checks
        .iter()
        .find(|c| c.requirement_id == "VV-MCP-NAMING-001")
        .unwrap();
    assert_eq!(
        naming.evidence.as_ref().unwrap()["outcome"],
        "review_required"
    );

    server.stop().await;
    // A refused connection is not required, because a proxy in front of a closed port
    // can still complete the TCP handshake. The server must not answer.
    let answered = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()?
        .get(format!("http://{address}/observatory/admin/docs/llms.txt"))
        .send()
        .await
        .is_ok();
    assert!(!answered, "fixture server outlived qualification");
    Ok(())
}

fn expect_mcp_error(error: rmcp::ServiceError, expected: rmcp::model::ErrorCode) {
    let rmcp::ServiceError::McpError(error) = error else {
        panic!("expected a protocol rejection, got {error:?}");
    };
    assert_eq!(error.code, expected, "{error:?}");
}
