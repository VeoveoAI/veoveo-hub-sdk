//! An MCP session with the Gateway as one caller: MCP `2026-07-28` over Streamable HTTP,
//! with the caller's token, renewed by reconnecting once a third of its life remains.

use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use rmcp::{
    ClientHandler, ClientLifecycleMode, ClientServiceExt, RoleClient,
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, ClientCapabilities, ClientConfig,
        GetPromptRequestParams, GetPromptResult, GetTaskParams, Implementation, JsonObject, Prompt,
        ProtocolVersion, ReadResourceRequestParams, ReadResourceResult, Resource, ResourceTemplate,
        TaskPayload, Tool,
    },
    service::RunningService,
    transport::{
        StreamableHttpClientTransport, streamable_http_client::StreamableHttpClientTransportConfig,
    },
};
use serde_json::Value;
use veoveo_sdk_rust_conformance::catalog;

use crate::gateway::{AccessToken, GatewayAccess};

/// The client's handler: it supports Tasks and ignores server notifications, since every
/// result arrives through a request.
#[derive(Clone, Copy)]
struct Handler;

impl ClientHandler for Handler {
    fn get_info(&self) -> ClientConfig {
        ClientConfig::new(
            ClientCapabilities::builder().enable_tasks().build(),
            Implementation::new("veoveo-sdk-rust-cli", env!("CARGO_PKG_VERSION")),
        )
    }
}

/// A session that reconnects with a fresh token before the current one expires.
pub struct Session {
    access: GatewayAccess,
    http: reqwest::Client,
    token: AccessToken,
    client: RunningService<RoleClient, Handler>,
}

impl Session {
    pub async fn open(access: GatewayAccess) -> Result<Self> {
        let http = access.http_client()?;
        let (token, client) = connect(&access, &http).await?;
        Ok(Self {
            access,
            http,
            token,
            client,
        })
    }

    /// Reconnect when the token has less than a third of its lifetime left.
    pub async fn refresh(&mut self) -> Result<()> {
        if !self.token.is_stale() {
            return Ok(());
        }
        let (token, client) = connect(&self.access, &self.http).await?;
        let previous = std::mem::replace(&mut self.client, client);
        self.token = token;
        let _ = previous.cancel().await;
        Ok(())
    }

    pub async fn tools(&self) -> Result<Vec<Tool>> {
        catalog::tools(self.client.peer()).await
    }

    pub async fn resources(&self) -> Result<Vec<Resource>> {
        catalog::resources(self.client.peer()).await
    }

    pub async fn resource_templates(&self) -> Result<Vec<ResourceTemplate>> {
        catalog::templates(self.client.peer()).await
    }

    pub async fn prompts(&self) -> Result<Vec<Prompt>> {
        catalog::prompts(self.client.peer()).await
    }

    pub async fn read_resource(&self, uri: &str) -> Result<ReadResourceResult> {
        Ok(self
            .client
            .read_resource(ReadResourceRequestParams::new(uri))
            .await?)
    }

    pub async fn prompt(&self, name: &str, arguments: JsonObject) -> Result<GetPromptResult> {
        Ok(self
            .client
            .get_prompt(GetPromptRequestParams::new(name).with_arguments(arguments))
            .await?)
    }

    /// Call `tool` and return its result. A tool that runs as a Task is polled at the
    /// interval its Task names until it ends, for at most `deadline`.
    pub async fn call(
        &self,
        tool: &str,
        arguments: JsonObject,
        deadline: Duration,
    ) -> Result<CallToolResult> {
        let request = CallToolRequestParams::new(tool.to_owned()).with_arguments(arguments);
        match self.client.call_tool_once(request).await? {
            CallToolResponse::Complete(result) => Ok(result),
            CallToolResponse::Task(created) => {
                let task_id = created.task.task_id.clone();
                let poll = Duration::from_millis(
                    created
                        .task
                        .poll_interval_ms
                        .unwrap_or(500)
                        .clamp(50, 5_000),
                );
                tokio::time::timeout(deadline, async {
                    loop {
                        let current = self
                            .client
                            .get_task(GetTaskParams::new(task_id.clone()))
                            .await?;
                        match current.task.payload {
                            TaskPayload::Working => tokio::time::sleep(poll).await,
                            TaskPayload::Completed { result } => {
                                return Ok(serde_json::from_value(Value::Object(
                                    result.into_iter().collect(),
                                ))?);
                            }
                            TaskPayload::Failed { error } => {
                                bail!(
                                    "Task {task_id} failed: {}",
                                    Value::Object(error.into_iter().collect())
                                )
                            }
                            TaskPayload::Cancelled => bail!("Task {task_id} was cancelled"),
                            TaskPayload::InputRequired { .. } => {
                                bail!("Task {task_id} asks for input this client cannot give")
                            }
                            _ => bail!("Task {task_id} ended in an unsupported state"),
                        }
                    }
                })
                .await
                .map_err(|_| anyhow!("Task {task_id} did not end within {deadline:?}"))?
            }
            CallToolResponse::InputRequired(_) => {
                bail!("`{tool}` asks for input before it runs, which this client cannot give")
            }
            _ => bail!("`{tool}` answered with a response this client does not support"),
        }
    }

    pub async fn close(self) -> Result<()> {
        self.client
            .cancel()
            .await
            .context("close the MCP session")?;
        Ok(())
    }
}

async fn connect(
    access: &GatewayAccess,
    http: &reqwest::Client,
) -> Result<(AccessToken, RunningService<RoleClient, Handler>)> {
    let token = access.access_token(http).await?;
    let config = StreamableHttpClientTransportConfig::with_uri(access.mcp_endpoint()?.to_string())
        .auth_header(token.value.clone());
    let transport = StreamableHttpClientTransport::with_client(http.clone(), config);
    let client = Handler
        .serve_with_lifecycle(
            transport,
            ClientLifecycleMode::Discover {
                preferred_versions: vec![ProtocolVersion::V_2026_07_28],
            },
        )
        .await
        .context("open an MCP session with the Gateway")?;
    Ok((token, client))
}
