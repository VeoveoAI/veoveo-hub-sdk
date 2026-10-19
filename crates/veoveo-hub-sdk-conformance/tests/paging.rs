//! Test-owned synthetic revision-bound listing. This qualifies the reusable probe
//! and adapters, not a production service or a real model/provider.
use std::{
    collections::BTreeSet,
    sync::{
        Arc, LazyLock,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    time::Duration,
};

use anyhow::{Result, ensure};
use axum::{Router, extract::Request, http::StatusCode, middleware::Next, response::IntoResponse};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rmcp::{
    ClientLifecycleMode, ClientServiceExt, RoleClient, RoleServer, ServerHandler,
    model::*,
    service::{Peer, RequestContext},
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
        streamable_http_server::StreamableHttpService,
    },
};
use serde::{Deserialize, Serialize};
use veoveo_hub_sdk::docs::{ContractDeclaration, DOC_ID_AGENTS, DOC_ID_DESIGN, ServerDocs};
use veoveo_hub_sdk_conformance::{
    ConformanceCredentials, HostedServerConformanceProfile, HostedServerProfileFormat,
    HttpBoundaryProfile, SurfaceExpectation, SurfaceProfile,
    paging::{self, ObservedPage, PagingEvidence, PagingLimits, PagingOrigin, PagingSurface},
    run_hosted_server_conformance_with_paging,
};
use veoveo_types::{NamingLabel, ResourceScheme, ServerSlug};

const GROUP: &str = "main + β";
const ENTRIES_TEMPLATE: &str = "domain://entries{?owner,group,limit,cursor}";
static DOCS: LazyLock<ServerDocs> = LazyLock::new(|| {
    ServerDocs::new("domain")
        .with_doc(
            DOC_ID_AGENTS,
            "Agent manual",
            include_str!("../../veoveo-hub-sdk/testdata/compliance-domain.md"),
        )
        .with_doc(
            DOC_ID_DESIGN,
            "Domain design",
            "Synthetic revision-bound paging fixture.",
        )
        .with_profile_json(include_str!(
            "../../veoveo-hub-sdk/testdata/compliance-domain.json"
        ))
        .unwrap()
});

#[derive(Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Entry {
    rank: u32,
    id: u32,
}

#[derive(Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Query {
    owner: String,
    group: String,
    limit: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cursor: Option<String>,
}

#[derive(Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Page {
    items: Vec<Entry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    next_cursor: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Cursor {
    owner: String,
    group: String,
    limit: usize,
    revision: u32,
    after: (u32, u32),
}

#[derive(Clone, Default)]
struct Fixture {
    revision: Arc<AtomicU32>,
    revoked: Arc<AtomicBool>,
}

impl Fixture {
    fn authorize(
        &self,
        context: &RequestContext<RoleServer>,
        owner: &str,
    ) -> Result<(), ErrorData> {
        let bearer = context
            .extensions
            .get::<axum::http::request::Parts>()
            .and_then(|parts| parts.headers.get(axum::http::header::AUTHORIZATION))
            .and_then(|value| value.to_str().ok());
        if bearer != Some(format!("Bearer {owner}").as_str()) || self.revoked.load(Ordering::SeqCst)
        {
            return Err(ErrorData::invalid_request("fixture access denied", None));
        }
        Ok(())
    }
    fn page(&self, query: Query) -> Result<Page, ErrorData> {
        let invalid =
            || ErrorData::invalid_params("invalid cursor or listing; restart without cursor", None);
        if query.limit == 0 || query.limit > 2 {
            return Err(invalid());
        }
        let revision = self.revision.load(Ordering::SeqCst);
        let after = match query.cursor {
            None => None,
            Some(wire) => {
                if wire.len() > 4096 {
                    return Err(invalid());
                }
                let cursor: Cursor =
                    serde_json::from_slice(&URL_SAFE_NO_PAD.decode(wire).map_err(|_| invalid())?)
                        .map_err(|_| invalid())?;
                if cursor.owner != query.owner
                    || cursor.group != query.group
                    || cursor.limit != query.limit
                    || cursor.revision != revision
                {
                    return Err(invalid());
                }
                Some(cursor.after)
            }
        };
        let count = match query.group.as_str() {
            GROUP => 5 + revision,
            "boundary" => 4,
            "single" => 2,
            _ => 0,
        };
        let all: Vec<_> = (1..=count).map(|id| Entry { rank: id / 3, id }).collect();
        let remaining: Vec<_> = all
            .into_iter()
            .filter(|entry| after.is_none_or(|after| (entry.rank, entry.id) > after))
            .collect();
        let next = remaining.len() > query.limit;
        let items: Vec<_> = remaining.into_iter().take(query.limit).collect();
        let next_cursor = if next {
            let last = items.last().unwrap();
            Some(
                URL_SAFE_NO_PAD.encode(
                    serde_json::to_vec(&Cursor {
                        owner: query.owner,
                        group: query.group,
                        limit: query.limit,
                        revision,
                        after: (last.rank, last.id),
                    })
                    .unwrap(),
                ),
            )
        } else {
            None
        };
        Ok(Page { items, next_cursor })
    }
}

fn resource_uri(query: &Query) -> String {
    let mut uri = url::Url::parse("domain://entries").unwrap();
    {
        let mut pairs = uri.query_pairs_mut();
        pairs
            .append_pair("owner", &query.owner)
            .append_pair("group", &query.group)
            .append_pair("limit", &query.limit.to_string());
        if let Some(cursor) = &query.cursor {
            pairs.append_pair("cursor", cursor);
        }
    }
    uri.into()
}

impl ServerHandler for Fixture {
    fn get_info(&self) -> ServerConfig {
        let mut config = ServerConfig::default();
        config.server_info = Implementation::new("domain", "paging-fixture");
        config.capabilities = ServerCapabilities::builder()
            .enable_tools()
            .enable_resources()
            .build();
        config
    }
    async fn list_tools(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let mut tool = Tool::new(
            "list_entries",
            "Read a bounded revision-bound fixture page.",
            serde_json::from_value::<JsonObject>(
                serde_json::to_value(schemars::schema_for!(Query)).unwrap(),
            )
            .unwrap(),
        );
        // Match Hosted::published: optional tool arguments are omitted, not null.
        veoveo_hub_sdk::hosting::publish_input_schema(Arc::make_mut(&mut tool.input_schema));
        tool.output_schema = Some(Arc::new(
            serde_json::from_value::<JsonObject>(
                serde_json::to_value(schemars::schema_for!(Page)).unwrap(),
            )
            .unwrap(),
        ));
        Ok(ListToolsResult {
            tools: vec![tool],
            ..Default::default()
        })
    }
    async fn list_resources(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        Ok(ListResourcesResult {
            resources: [
                "domain://entries",
                "domain://docs",
                "domain://docs/agents",
                "domain://docs/design",
                "domain://contract",
            ]
            .into_iter()
            .map(|uri| Resource::new(uri, "Fixture resource"))
            .collect(),
            ..Default::default()
        })
    }
    async fn list_resource_templates(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, ErrorData> {
        Ok(ListResourceTemplatesResult {
            resource_templates: vec![
                ResourceTemplate::new(ENTRIES_TEMPLATE, "Fixture entry pages")
                    .with_mime_type("application/json"),
            ],
            ..Default::default()
        })
    }
    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, ErrorData> {
        if !request.uri.starts_with("domain://entries") {
            self.authorize(&context, "owner-a")?;
            if request.uri == "domain://contract" {
                let text = serde_json::to_string(&ContractDeclaration::from_docs(&DOCS)).unwrap();
                return Ok(ReadResourceResult::new(vec![ResourceContents::text(
                    text,
                    request.uri,
                )])
                .into());
            }
            if let Some(result) =
                DOCS.read_authorized_docs(&ResourceScheme::parse("domain").unwrap(), &request)?
            {
                return Ok(result);
            }
        }
        let url = url::Url::parse(&request.uri)
            .map_err(|_| ErrorData::invalid_params("invalid URI", None))?;
        if url.host_str() != Some("entries") {
            return Err(ErrorData::invalid_params("unknown resource", None));
        }
        let pairs: Vec<_> = url.query_pairs().collect();
        let mut names = BTreeSet::new();
        if pairs.iter().any(|(name, _)| !names.insert(name.as_ref())) {
            return Err(ErrorData::invalid_params("duplicate query parameter", None));
        }
        let find = |name| {
            pairs
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.to_string())
        };
        let query = Query {
            owner: find("owner").unwrap_or_default(),
            group: find("group").unwrap_or_default(),
            limit: find("limit")
                .and_then(|value| value.parse().ok())
                .unwrap_or(2),
            cursor: find("cursor"),
        };
        self.authorize(&context, &query.owner)?;
        let text = serde_json::to_string(&self.page(query)?).unwrap();
        Ok(ReadResourceResult::new(vec![ResourceContents::text(text, request.uri)]).into())
    }
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        if request.name != "list_entries" {
            return Err(ErrorData::invalid_params("unknown tool", None));
        }
        let query: Query = serde_json::from_value(serde_json::Value::Object(
            request.arguments.unwrap_or_default(),
        ))
        .map_err(|_| ErrorData::invalid_params("invalid arguments", None))?;
        self.authorize(&context, &query.owner)?;
        Ok(CallToolResult::structured(serde_json::to_value(self.page(query)?).unwrap()).into())
    }
}

async fn authenticated(request: Request, next: Next) -> axum::response::Response {
    if !matches!(
        request
            .headers()
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok()),
        Some("Bearer owner-a" | "Bearer owner-b")
    ) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    next.run(request).await
}

async fn fetch(
    client: &Peer<RoleClient>,
    surface: PagingSurface,
    query: Query,
) -> Result<(Page, ObservedPage<(u32, u32)>)> {
    match surface {
        PagingSurface::Resource => {
            let result = client
                .read_resource(ReadResourceRequestParams::new(resource_uri(&query)))
                .await?;
            let text = result
                .contents
                .iter()
                .find_map(|content| match content {
                    ResourceContents::TextResourceContents { text, .. } => Some(text),
                    _ => None,
                })
                .unwrap();
            let page: Page = serde_json::from_str(text)?;
            let observation = ObservedPage::from_response(
                &result,
                page.items
                    .iter()
                    .map(|entry| (entry.rank, entry.id))
                    .collect(),
                page.next_cursor.clone(),
            )?;
            Ok((page, observation))
        }
        PagingSurface::Tool => {
            let result = client
                .call_tool(
                    CallToolRequestParams::new("list_entries")
                        .with_arguments(serde_json::to_value(query)?.as_object().unwrap().clone()),
                )
                .await?;
            ensure!(
                result.is_error != Some(true),
                "fixture tool returned application error"
            );
            let page: Page = serde_json::from_value(result.structured_content.clone().unwrap())?;
            let observation = ObservedPage::from_response(
                &result,
                page.items
                    .iter()
                    .map(|entry| (entry.rank, entry.id))
                    .collect(),
                page.next_cursor.clone(),
            )?;
            Ok((page, observation))
        }
    }
}

struct OwnedServer(tokio::task::JoinHandle<()>);
impl Drop for OwnedServer {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[tokio::test]
async fn resource_and_tool_adapters_measure_traversal_and_owner_policy() -> Result<()> {
    tokio::time::timeout(Duration::from_secs(60), qualify()).await??;
    Ok(())
}

async fn qualify() -> Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let fixture = Fixture::default();
    let factory = fixture.clone();
    let service = StreamableHttpService::new(
        move || Ok(factory.clone()),
        veoveo_hub_sdk::stateless_session_manager(),
        veoveo_hub_sdk::canonical_streamable_http_server_config(),
    );
    let router = Router::new()
        .nest_service("/domain/mcp", service)
        .route(
            "/domain/admin/docs/llms.txt",
            axum::routing::get(|| async { DOCS.llms_txt() }),
        )
        .route(
            "/domain/admin/docs/{id}",
            axum::routing::get(
                |axum::extract::Path(id): axum::extract::Path<String>| async move {
                    match DOCS.doc(&id) {
                        Some(doc) => (StatusCode::OK, doc.body.to_owned()),
                        None => (StatusCode::NOT_FOUND, String::new()),
                    }
                },
            ),
        )
        .layer(axum::middleware::from_fn(authenticated));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let _server = OwnedServer(tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    }));
    let endpoint = format!("http://{address}/domain/mcp");
    let client = ()
        .serve_with_lifecycle(
            StreamableHttpClientTransport::from_config(
                StreamableHttpClientTransportConfig::with_uri(endpoint.clone())
                    .auth_header("owner-a"),
            ),
            ClientLifecycleMode::Discover {
                preferred_versions: vec![ProtocolVersion::V_2026_07_28],
            },
        )
        .await?;
    let other_client = ()
        .serve_with_lifecycle(
            StreamableHttpClientTransport::from_config(
                StreamableHttpClientTransportConfig::with_uri(endpoint.clone())
                    .auth_header("owner-b"),
            ),
            ClientLifecycleMode::Discover {
                preferred_versions: vec![ProtocolVersion::V_2026_07_28],
            },
        )
        .await?;
    let mut observations = vec![];
    let required = [
        NamingLabel::new("domain-resource-entries")?,
        NamingLabel::new("domain-tool-entries")?,
    ];
    let limits = PagingLimits {
        page_items: 2,
        response_bytes: 16 * 1024,
        cursor_bytes: 4096,
    };
    for (surface, label) in [
        (PagingSurface::Resource, &required[0]),
        (PagingSurface::Tool, &required[1]),
    ] {
        for (group, count) in [(GROUP, 5), ("boundary", 4), ("single", 2), ("empty", 0)] {
            let count = count
                + if group == GROUP {
                    fixture.revision.load(Ordering::SeqCst)
                } else {
                    0
                };
            let expected: Vec<_> = (1..=count).map(|id| (id / 3, id)).collect();
            let peer = client.peer();
            observations.push(
                paging::inspect_pages(
                    ServerSlug::parse("domain")?,
                    label.clone(),
                    surface,
                    PagingOrigin::SyntheticFixture,
                    &limits,
                    &expected,
                    |cursor| async move {
                        Ok(fetch(
                            peer,
                            surface,
                            Query {
                                owner: "owner-a".into(),
                                group: group.into(),
                                limit: 2,
                                cursor,
                            },
                        )
                        .await?
                        .1)
                    },
                )
                .await,
            );
        }
        let query = Query {
            owner: "owner-a".into(),
            group: GROUP.into(),
            limit: 2,
            cursor: None,
        };
        let (first, _) = fetch(client.peer(), surface, query.clone()).await?;
        let mut next = Query {
            cursor: first.next_cursor,
            ..query
        };
        assert!(next.cursor.is_some());
        for changed in [
            Query {
                group: "boundary".into(),
                ..next.clone()
            },
            Query {
                limit: 1,
                ..next.clone()
            },
            Query {
                cursor: Some("malformed".into()),
                ..next.clone()
            },
            Query {
                cursor: Some("x".repeat(4097)),
                ..next.clone()
            },
        ] {
            expect_rejection(
                fetch(client.peer(), surface, changed).await,
                ErrorCode::INVALID_PARAMS,
            );
        }
        expect_rejection(
            fetch(
                client.peer(),
                surface,
                Query {
                    owner: "owner-b".into(),
                    ..next.clone()
                },
            )
            .await,
            ErrorCode::INVALID_REQUEST,
        );
        expect_rejection(
            fetch(
                other_client.peer(),
                surface,
                Query {
                    owner: "owner-b".into(),
                    ..next.clone()
                },
            )
            .await,
            ErrorCode::INVALID_PARAMS,
        );
        fixture.revision.fetch_add(1, Ordering::SeqCst);
        expect_rejection(
            fetch(client.peer(), surface, next.clone()).await,
            ErrorCode::INVALID_PARAMS,
        );
        next.cursor = None;
        let (first, _) = fetch(client.peer(), surface, next.clone()).await?;
        next.cursor = first.next_cursor;
        fixture.revoked.store(true, Ordering::SeqCst);
        expect_rejection(
            fetch(client.peer(), surface, next).await,
            ErrorCode::INVALID_REQUEST,
        );
        fixture.revoked.store(false, Ordering::SeqCst);
    }
    let first = fetch(
        client.peer(),
        PagingSurface::Resource,
        Query {
            owner: "owner-a".into(),
            group: GROUP.into(),
            limit: 2,
            cursor: None,
        },
    )
    .await?
    .0;
    let uri = resource_uri(&Query {
        owner: "owner-a".into(),
        group: GROUP.into(),
        limit: 2,
        cursor: first.next_cursor,
    });
    expect_rejection(
        client
            .read_resource(ReadResourceRequestParams::new(format!(
                "{uri}&cursor=duplicate"
            )))
            .await
            .map_err(Into::into),
        ErrorCode::INVALID_PARAMS,
    );

    let profile = HostedServerConformanceProfile {
        format: HostedServerProfileFormat::McpConformanceProfile,
        profile_id: "synthetic-paging".into(),
        contract: veoveo_hub_sdk_conformance::ContractId::HostedMcp,
        endpoint,
        server_slug: "domain".into(),
        owned_resource_schemes: BTreeSet::from(["domain".into()]),
        http: HttpBoundaryProfile {
            require_authentication_rejection: true,
            rejected_host: None,
            health_url: None,
            readiness_url: None,
            docs_llms_url: format!("http://{address}/domain/admin/docs/llms.txt"),
        },
        surfaces: SurfaceProfile {
            tools: SurfaceExpectation::Required,
            resources: SurfaceExpectation::Required,
            resource_templates: SurfaceExpectation::Required,
            prompts: SurfaceExpectation::Forbidden,
            completions: SurfaceExpectation::Forbidden,
            tasks: SurfaceExpectation::Forbidden,
            subscriptions: SurfaceExpectation::Forbidden,
            required_tools: BTreeSet::from(["list_entries".into()]),
            required_resources: BTreeSet::from(["domain://entries".into()]),
            required_resource_templates: BTreeSet::from([ENTRIES_TEMPLATE.into()]),
            required_prompts: BTreeSet::new(),
        },
    };
    let report = run_hosted_server_conformance_with_paging(
        &profile,
        &ConformanceCredentials::bearer("owner-a"),
        &Default::default(),
        &PagingEvidence {
            required_listings: &required,
            observations: &observations,
        },
    )
    .await?;
    println!("{}", serde_json::to_string(&report)?);
    assert!(report.passed(), "{:#?}", report.checks);
    let c04 = veoveo_hub_sdk_conformance::requirements::requirement_verification(
        veoveo_hub_sdk::docs::RequirementId::C04,
    );
    assert_eq!(
        c04.assess(&report.checks),
        veoveo_hub_sdk_conformance::requirements::RequirementOutcome::ReviewRequired
    );
    client.cancel().await?;
    other_client.cancel().await?;
    Ok(())
}

fn expect_rejection<T>(result: Result<T>, expected: ErrorCode) {
    let error = match result {
        Ok(_) => panic!("request unexpectedly admitted"),
        Err(error) => error,
    };
    let Some(rmcp::ServiceError::McpError(error)) = error.downcast_ref::<rmcp::ServiceError>()
    else {
        panic!("expected MCP rejection, got {error:?}");
    };
    assert_eq!(error.code, expected);
}
