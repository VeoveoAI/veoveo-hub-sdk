//! Typed calls to servers through one Gateway profile.
//!
//! A [`GatewayClient`] speaks MCP `2026-07-28` over Streamable HTTP to
//! `{issuer}/mcp/{profile}`. A [`BearerSource`] supplies the bearer for each provider:
//! a server's own client exchanges its caller's token for one (`ServerCalls` in the
//! `server` feature), and a program with its own access token uses [`StaticBearer`].
//!
//! Calls are builders that run when awaited. A request type from a provider's
//! `contract` feature names the tool and its result:
//!
//! ```ignore
//! let usage = client.call(ReadStorageUsage {}).await?;
//! let upload = client.start(VerifyArtifact { artifact }).await?;
//! let verified = upload.await?;
//! ```
//!
//! [`GatewayClient::call`] resolves with the tool's structured result, waiting for the
//! Task when the tool runs as one. [`GatewayClient::start`] resolves once a tool that
//! runs as a Task has started and returns its [`TaskHandle`], which resolves with the
//! result when the Task completes.
use std::{
    collections::{BTreeMap, HashMap},
    future::{Future, IntoFuture},
    marker::PhantomData,
    pin::Pin,
    sync::Arc,
    time::Duration,
};

use futures::{future::BoxFuture, stream::BoxStream};
use rmcp::{
    ClientLifecycleMode, ClientServiceExt, RoleClient,
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, CancelTaskParams,
        ClientCapabilities, ClientConfig, ClientJsonRpcMessage, DetailedTask, GetTaskParams,
        Implementation, ProtocolVersion, ServerNotification, SubscriptionFilter, TaskPayload,
    },
    service::{RunningService, ServiceError},
    transport::{
        StreamableHttpClientTransport,
        streamable_http_client::{
            SseError, StreamableHttpClient, StreamableHttpClientTransportConfig,
            StreamableHttpError, StreamableHttpPostResponse,
        },
    },
};
use serde::de::DeserializeOwned;
use sse_stream::Sse;
use veoveo_types::ServerSlug;

use crate::calls::{CallContractError, ProviderContract, TaskToolCall, TokenErrorCode, ToolCall};

/// A bearer token, never printed.
#[derive(Clone, PartialEq, Eq)]
pub struct Bearer(String);

impl Bearer {
    pub fn new(token: impl Into<String>) -> Self {
        Self(token.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Bearer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Bearer(<redacted>)")
    }
}

/// Supplies the bearer of calls to one provider's tools.
pub trait BearerSource: Send + Sync + 'static {
    fn bearer<'a>(&'a self, provider: &'a ServerSlug) -> BoxFuture<'a, Result<Bearer, CallError>>;

    /// Drops a bearer the Gateway no longer admits, so the next call obtains another.
    fn forget(&self, provider: &ServerSlug) {
        let _ = provider;
    }
}

/// One bearer for every provider: a program's own access token for the profile.
#[derive(Debug, Clone)]
pub struct StaticBearer(Bearer);

impl StaticBearer {
    pub fn new(token: impl Into<String>) -> Self {
        Self(Bearer::new(token))
    }
}

impl BearerSource for StaticBearer {
    fn bearer<'a>(&'a self, _: &'a ServerSlug) -> BoxFuture<'a, Result<Bearer, CallError>> {
        Box::pin(std::future::ready(Ok(self.0.clone())))
    }
}

/// Why a call produced no result.
#[derive(Debug, thiserror::Error)]
pub enum CallError {
    /// The Gateway refused to issue a token for the call, with the OAuth error of its
    /// token endpoint, such as `invalid_target` when no registered server provides a
    /// permission the call needs.
    #[error("the Gateway refused the call: {error:?}{}", describe(.description))]
    Refused {
        error: TokenErrorCode,
        description: Option<String>,
    },
    /// The calling server declared no use of the provider's permissions.
    #[error("this server declares no use of {provider}'s permissions")]
    Undeclared { provider: ServerSlug },
    /// The Gateway or the provider answered with a JSON-RPC error, such as a denied
    /// permission or invalid arguments.
    #[error("the call was rejected with JSON-RPC error {code}: {message}")]
    Rejected { code: i32, message: String },
    /// The tool ran and reported an error result.
    #[error("the tool reported an error: {message}")]
    Tool { message: String },
    /// The tool's Task failed or was cancelled.
    #[error("the Task {task} ended without a result: {reason}")]
    Task { task: String, reason: String },
    /// The tool asked for client input, which these calls do not answer.
    #[error("the tool asked for input that a server-to-server call cannot provide")]
    InputRequired,
    /// No answer arrived within the call's timeout.
    #[error("the call timed out after {0:?}")]
    Timeout(Duration),
    /// The result is not the contract's output type.
    #[error("the result is not the contract's output: {0}")]
    Decode(#[from] serde_json::Error),
    #[error(transparent)]
    Contract(#[from] CallContractError),
    /// The transport or the protocol failed before an answer arrived.
    #[error("the call failed: {0}")]
    Transport(String),
}

fn describe(description: &Option<String>) -> String {
    description
        .as_deref()
        .map(|text| format!(": {text}"))
        .unwrap_or_default()
}

impl From<ServiceError> for CallError {
    fn from(error: ServiceError) -> Self {
        match error {
            ServiceError::McpError(error) => Self::Rejected {
                code: error.code.0,
                message: error.message.into_owned(),
            },
            ServiceError::Timeout { timeout } => Self::Timeout(timeout),
            other => Self::Transport(other.to_string()),
        }
    }
}

type Service = RunningService<RoleClient, ClientConfig>;

/// The MCP connection for calls to one provider: it carries that provider's bearer.
struct Connection {
    service: Service,
}

/// Typed calls through one Gateway profile.
#[derive(Clone)]
pub struct GatewayClient {
    inner: Arc<Inner>,
}

struct Inner {
    endpoint: url::Url,
    http: reqwest::Client,
    bearers: Arc<dyn BearerSource>,
    implementation: Implementation,
    connections: tokio::sync::Mutex<BTreeMap<ServerSlug, Arc<Connection>>>,
}

impl std::fmt::Debug for GatewayClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GatewayClient")
            .field("endpoint", &self.inner.endpoint.as_str())
            .finish_non_exhaustive()
    }
}

impl GatewayClient {
    /// A client of the profile whose MCP endpoint is `endpoint`, such as
    /// `https://gateway.example/mcp/default`.
    pub fn new(endpoint: url::Url, http: reqwest::Client, bearers: impl BearerSource) -> Self {
        Self::with_source(endpoint, http, Arc::new(bearers))
    }

    pub(crate) fn with_source(
        endpoint: url::Url,
        http: reqwest::Client,
        bearers: Arc<dyn BearerSource>,
    ) -> Self {
        Self {
            inner: Arc::new(Inner {
                endpoint,
                http,
                bearers,
                implementation: Implementation::new("veoveo-sdk-rust", env!("CARGO_PKG_VERSION")),
                connections: tokio::sync::Mutex::new(BTreeMap::new()),
            }),
        }
    }

    /// Names this client in the `clientInfo` of its requests.
    pub fn named(mut self, name: impl Into<String>, version: impl Into<String>) -> Self {
        if let Some(inner) = Arc::get_mut(&mut self.inner) {
            inner.implementation = Implementation::new(name.into(), version.into());
        }
        self
    }

    /// The profile's MCP endpoint.
    pub fn endpoint(&self) -> &url::Url {
        &self.inner.endpoint
    }

    /// Calls a tool and resolves with its result, waiting for its Task when the tool
    /// runs as one.
    pub fn call<R: ToolCall>(&self, request: R) -> Call<R> {
        Call {
            client: self.clone(),
            request,
            timeout: None,
        }
    }

    /// Starts a tool that runs as a Task and resolves with its handle.
    pub fn start<R: TaskToolCall>(&self, request: R) -> StartTask<R> {
        StartTask {
            client: self.clone(),
            request,
        }
    }

    /// The handle of a Task this client started earlier, such as one a durable step
    /// recorded before a restart.
    pub async fn task<R: TaskToolCall>(
        &self,
        task: impl Into<String>,
    ) -> Result<TaskHandle<R::Output>, CallError> {
        let connection = self.connection(&R::Provider::slug()).await?;
        Ok(TaskHandle::started(connection, task.into()))
    }

    async fn connection(&self, provider: &ServerSlug) -> Result<Arc<Connection>, CallError> {
        // The bearer comes first, so a refusal reaches the caller as its OAuth error.
        self.inner.bearers.bearer(provider).await?;
        let mut connections = self.inner.connections.lock().await;
        if let Some(connection) = connections.get(provider) {
            return Ok(connection.clone());
        }
        let service = match self.connect(provider).await {
            Ok(service) => service,
            // The source's bearer is no longer admitted; the next one shows why.
            Err(error)
                if matches!(
                    &*error,
                    rmcp::service::ClientInitializeError::TransportError { error, .. }
                        if auth_required(error)
                ) =>
            {
                self.inner.bearers.forget(provider);
                self.inner.bearers.bearer(provider).await?;
                self.connect(provider)
                    .await
                    .map_err(|error| CallError::Transport(error.to_string()))?
            }
            Err(error) => return Err(CallError::Transport(error.to_string())),
        };
        let connection = Arc::new(Connection { service });
        connections.insert(provider.clone(), connection.clone());
        Ok(connection)
    }

    async fn connect(
        &self,
        provider: &ServerSlug,
    ) -> Result<Service, Box<rmcp::service::ClientInitializeError>> {
        let transport = StreamableHttpClientTransport::with_client(
            BearerHttp {
                http: self.inner.http.clone(),
                bearers: self.inner.bearers.clone(),
                provider: provider.clone(),
            },
            StreamableHttpClientTransportConfig::with_uri(self.inner.endpoint.as_str()),
        );
        let config = ClientConfig::new(
            ClientCapabilities::builder().enable_tasks().build(),
            self.inner.implementation.clone(),
        );
        config
            .serve_with_lifecycle(
                transport,
                ClientLifecycleMode::Discover {
                    preferred_versions: vec![ProtocolVersion::V_2026_07_28],
                },
            )
            .await
            .map_err(Box::new)
    }

    async fn send<R: ToolCall>(
        &self,
        request: R,
    ) -> Result<(Arc<Connection>, CallToolResponse), CallError> {
        let name = R::gateway_tool_name()?;
        let arguments = match serde_json::to_value(request)? {
            serde_json::Value::Object(arguments) => arguments,
            _ => {
                return Err(CallError::Contract(CallContractError::Tool(R::TOOL)));
            }
        };
        let provider = R::Provider::slug();
        let params = CallToolRequestParams::new(name.to_string()).with_arguments(arguments);
        let connection = self.connection(&provider).await?;
        match connection.service.call_tool_once(params.clone()).await {
            Ok(response) => Ok((connection, response)),
            // A bearer the Gateway stopped admitting, such as one whose Task ended, is
            // replaced once, so the caller sees why the Gateway refuses another.
            Err(error) if unauthorized(&error) => {
                self.inner.bearers.forget(&provider);
                self.inner.connections.lock().await.remove(&provider);
                let connection = self.connection(&provider).await?;
                let response = connection.service.call_tool_once(params).await?;
                Ok((connection, response))
            }
            Err(error) => Err(error.into()),
        }
    }
}

/// Whether the Gateway answered HTTP 401 to the bearer a request carried.
fn unauthorized(error: &ServiceError) -> bool {
    matches!(error, ServiceError::TransportSend(error) if auth_required(error))
}

fn auth_required(error: &rmcp::transport::DynamicTransportError) -> bool {
    matches!(
        error
            .error
            .downcast_ref::<StreamableHttpError<reqwest::Error>>(),
        Some(StreamableHttpError::AuthRequired(_))
    )
}

/// A tool call that runs when awaited.
#[must_use = "a call runs when it is awaited"]
pub struct Call<R: ToolCall> {
    client: GatewayClient,
    request: R,
    timeout: Option<Duration>,
}

impl<R: ToolCall> Call<R> {
    /// Fails the call with [`CallError::Timeout`] when no result arrives in time,
    /// including the time its Task runs.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }
}

impl<R: ToolCall> IntoFuture for Call<R> {
    type Output = Result<R::Output, CallError>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send>>;

    fn into_future(self) -> Self::IntoFuture {
        let Call {
            client,
            request,
            timeout,
        } = self;
        let call = async move {
            match client.send(request).await? {
                (_, CallToolResponse::Complete(result)) => decode(result),
                (connection, CallToolResponse::Task(created)) => {
                    TaskHandle::<R::Output>::started(connection, created.task.task_id).await
                }
                (_, CallToolResponse::InputRequired(_)) => Err(CallError::InputRequired),
                (_, _) => Err(CallError::Transport(
                    "unexpected tools/call response".into(),
                )),
            }
        };
        Box::pin(within(timeout, call))
    }
}

/// Starts a tool that runs as a Task when awaited.
#[must_use = "a call runs when it is awaited"]
pub struct StartTask<R: TaskToolCall> {
    client: GatewayClient,
    request: R,
}

impl<R: TaskToolCall> IntoFuture for StartTask<R> {
    type Output = Result<TaskHandle<R::Output>, CallError>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(async move {
            match self.client.send(self.request).await? {
                (connection, CallToolResponse::Task(created)) => {
                    Ok(TaskHandle::started(connection, created.task.task_id))
                }
                // A provider may finish a Task tool at once; the handle is resolved.
                (connection, CallToolResponse::Complete(result)) => Ok(TaskHandle {
                    connection,
                    task: None,
                    finished: Some(decode(result)),
                    timeout: None,
                    output: PhantomData,
                }),
                (_, CallToolResponse::InputRequired(_)) => Err(CallError::InputRequired),
                (_, _) => Err(CallError::Transport(
                    "unexpected tools/call response".into(),
                )),
            }
        })
    }
}

/// A provider Task this client started. Awaiting it resolves with the Task's result,
/// delivered through a Task status subscription.
#[must_use = "a Task handle resolves when it is awaited"]
pub struct TaskHandle<O> {
    connection: Arc<Connection>,
    task: Option<String>,
    finished: Option<Result<O, CallError>>,
    timeout: Option<Duration>,
    output: PhantomData<fn() -> O>,
}

impl<O> std::fmt::Debug for TaskHandle<O> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TaskHandle")
            .field("task", &self.task)
            .finish_non_exhaustive()
    }
}

/// The status of a provider Task when it was read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskState {
    Working,
    InputRequired,
    Completed,
    Failed { reason: String },
    Cancelled,
}

impl<O: DeserializeOwned + Send + 'static> TaskHandle<O> {
    fn started(connection: Arc<Connection>, task: String) -> Self {
        Self {
            connection,
            task: Some(task),
            finished: None,
            timeout: None,
            output: PhantomData,
        }
    }

    /// The provider's Task identifier, absent when the tool finished at once.
    pub fn id(&self) -> Option<&str> {
        self.task.as_deref()
    }

    /// Fails the wait with [`CallError::Timeout`] when the Task has not finished in
    /// time. The Task keeps running.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Reads the Task's current status once.
    pub async fn state(&self) -> Result<TaskState, CallError> {
        let Some(task) = &self.task else {
            return Ok(TaskState::Completed);
        };
        let current = self
            .connection
            .service
            .get_task(GetTaskParams::new(task.clone()))
            .await?;
        Ok(state(&current.task))
    }

    /// Requests cancellation of the Task.
    pub async fn cancel(&self) -> Result<(), CallError> {
        if let Some(task) = &self.task {
            self.connection
                .service
                .cancel_task(CancelTaskParams::new(task.clone()))
                .await?;
        }
        Ok(())
    }

    async fn wait(self) -> Result<O, CallError> {
        if let Some(finished) = self.finished {
            return finished;
        }
        let task = self.task.expect("an unfinished handle names its Task");
        let filter = SubscriptionFilter::builder().task_id(&task).build();
        // A subscription first delivers the Task's current status, so a reopened one
        // misses no outcome. Three openings without one end the wait.
        for _ in 0..3 {
            let mut subscription = self.connection.service.listen(filter.clone()).await?;
            if subscription
                .acknowledged()
                .task_ids
                .as_ref()
                .is_none_or(|ids| !ids.contains(&task))
            {
                return Err(CallError::Transport(format!(
                    "the Gateway did not acknowledge a subscription to Task {task}"
                )));
            }
            while let Some(notification) = subscription.next().await? {
                let ServerNotification::TaskStatusNotification(notification) = notification else {
                    continue;
                };
                let detailed = notification.params.task;
                if detailed.task.task_id != task {
                    continue;
                }
                if let Some(outcome) = outcome(&task, detailed) {
                    let _ = subscription.cancel().await;
                    return outcome;
                }
            }
        }
        Err(CallError::Transport(format!(
            "the subscription to Task {task} kept ending before it finished"
        )))
    }
}

impl<O: DeserializeOwned + Send + 'static> IntoFuture for TaskHandle<O> {
    type Output = Result<O, CallError>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send>>;

    fn into_future(self) -> Self::IntoFuture {
        let timeout = self.timeout;
        Box::pin(within(timeout, self.wait()))
    }
}

fn state(task: &DetailedTask) -> TaskState {
    match &task.payload {
        TaskPayload::Working => TaskState::Working,
        TaskPayload::InputRequired { .. } => TaskState::InputRequired,
        TaskPayload::Completed { .. } => TaskState::Completed,
        TaskPayload::Failed { error } => TaskState::Failed {
            reason: failure_reason(error),
        },
        _ => TaskState::Cancelled,
    }
}

/// The result of a finished Task, or `None` while it runs.
fn outcome<O: DeserializeOwned>(
    task: &str,
    detailed: DetailedTask,
) -> Option<Result<O, CallError>> {
    match detailed.payload {
        TaskPayload::Working | TaskPayload::InputRequired { .. } => None,
        TaskPayload::Completed { result } => Some(
            serde_json::from_value::<CallToolResult>(serde_json::Value::Object(result))
                .map_err(CallError::from)
                .and_then(decode),
        ),
        TaskPayload::Failed { error } => Some(Err(CallError::Task {
            task: task.to_owned(),
            reason: failure_reason(&error),
        })),
        _ => Some(Err(CallError::Task {
            task: task.to_owned(),
            reason: "cancelled".into(),
        })),
    }
}

fn failure_reason(error: &serde_json::Map<String, serde_json::Value>) -> String {
    error
        .get("message")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("failed")
        .to_owned()
}

/// A tool's structured result as the contract's output type.
fn decode<O: DeserializeOwned>(result: CallToolResult) -> Result<O, CallError> {
    if result.is_error.unwrap_or(false) {
        let message = result
            .content
            .iter()
            .filter_map(|content| content.as_text().map(|text| text.text.as_str()))
            .collect::<Vec<_>>()
            .join("\n");
        return Err(CallError::Tool { message });
    }
    Ok(serde_json::from_value(
        result.structured_content.unwrap_or(serde_json::Value::Null),
    )?)
}

async fn within<T>(
    timeout: Option<Duration>,
    work: impl Future<Output = Result<T, CallError>>,
) -> Result<T, CallError> {
    match timeout {
        Some(timeout) => tokio::time::timeout(timeout, work)
            .await
            .unwrap_or(Err(CallError::Timeout(timeout))),
        None => work.await,
    }
}

/// The HTTP client of one provider's connection: every request carries the bearer the
/// source holds for that provider at the time it is sent.
#[derive(Clone)]
struct BearerHttp {
    http: reqwest::Client,
    bearers: Arc<dyn BearerSource>,
    provider: ServerSlug,
}

impl BearerHttp {
    async fn bearer(&self) -> Result<String, StreamableHttpError<reqwest::Error>> {
        self.bearers
            .bearer(&self.provider)
            .await
            .map(|bearer| bearer.0)
            .map_err(|error| {
                StreamableHttpError::UnexpectedServerResponse(error.to_string().into())
            })
    }
}

impl StreamableHttpClient for BearerHttp {
    type Error = reqwest::Error;

    async fn post_message(
        &self,
        uri: Arc<str>,
        message: ClientJsonRpcMessage,
        session_id: Option<Arc<str>>,
        _: Option<String>,
        custom_headers: HashMap<reqwest::header::HeaderName, reqwest::header::HeaderValue>,
    ) -> Result<StreamableHttpPostResponse, StreamableHttpError<Self::Error>> {
        let bearer = self.bearer().await?;
        self.http
            .post_message(uri, message, session_id, Some(bearer), custom_headers)
            .await
    }

    async fn delete_session(
        &self,
        uri: Arc<str>,
        session_id: Arc<str>,
        _: Option<String>,
        custom_headers: HashMap<reqwest::header::HeaderName, reqwest::header::HeaderValue>,
    ) -> Result<(), StreamableHttpError<Self::Error>> {
        let bearer = self.bearer().await?;
        self.http
            .delete_session(uri, session_id, Some(bearer), custom_headers)
            .await
    }

    async fn get_stream(
        &self,
        uri: Arc<str>,
        session_id: Option<Arc<str>>,
        last_event_id: Option<String>,
        _: Option<String>,
        custom_headers: HashMap<reqwest::header::HeaderName, reqwest::header::HeaderValue>,
    ) -> Result<BoxStream<'static, Result<Sse, SseError>>, StreamableHttpError<Self::Error>> {
        let bearer = self.bearer().await?;
        self.http
            .get_stream(uri, session_id, last_event_id, Some(bearer), custom_headers)
            .await
    }
}
