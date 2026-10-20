//! Typed calls to servers through one Hub profile.
//!
//! A [`HubClient`] speaks MCP `2026-07-28` over Streamable HTTP to
//! `{issuer}/mcp/{profile}`. A [`BearerSource`] supplies the bearer for each provider:
//! a server's own client exchanges its caller's token for one (`ServerCalls` in the
//! `server` feature), and a program with its own access token uses [`StaticBearer`].
//!
//! Calls are builders that run when awaited. A request type from a provider's
//! `contract` feature names the tool and its result:
//!
//! ```ignore
//! // Illustrative provider-owned request types, not SDK domain APIs.
//! let capacity = client.call(QueryCapacity {}).await?;
//! let generation = client.start(GenerateReport { input }).await?;
//! let report = generation.await?;
//! ```
//!
//! [`HubClient::call`] resolves with the tool's structured result, waiting for the
//! Task when the tool runs as one. [`HubClient::start`] resolves once a tool that
//! runs as a Task has started and returns its [`TaskHandle`], which resolves with the
//! result when the Task completes.
//!
//! A program that acts as one caller with its own token, such as a command-line client
//! or an agent, uses `ClientCredentialsBearer` from the `client-credentials` feature, or
//! a [`StaticBearer`], and the calls that need no provider contract: [`HubClient::call_tool`] by Hub tool name,
//! [`HubClient::read_resource`] by address, and the complete catalog listings
//! [`HubClient::tools`], [`HubClient::resources`],
//! [`HubClient::resource_templates`] and [`HubClient::prompts`].
//!
//! [`HubClient::read`] reads a provider resource as its contract's content. The
//! same builder waits for the content to reach a state, or yields it each time it
//! changes, through a `subscriptions/listen` subscription to the resource:
//!
//! ```ignore
//! // Illustrative provider-owned resource and its readiness predicate.
//! let current = client.read(ReadReport { report }).await?;
//! let ready = client
//!     .read(ReadReport { report })
//!     .until(|report| report.ready)
//!     .timeout(Duration::from_secs(600))
//!     .await?;
//! ```
use std::{
    collections::{BTreeMap, HashMap},
    future::{Future, IntoFuture},
    marker::PhantomData,
    pin::Pin,
    sync::Arc,
    time::Duration,
};

use futures::{StreamExt as _, future::BoxFuture, stream::BoxStream};
use rmcp::{
    ClientLifecycleMode, ClientServiceExt, RoleClient,
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, CancelTaskParams,
        ClientCapabilities, ClientConfig, ClientJsonRpcMessage, DetailedTask, GetTaskParams,
        Implementation, JsonObject, Prompt, ProtocolVersion, ReadResourceRequestParams,
        ReadResourceResult, Resource, ResourceContents, ResourceTemplate, ServerNotification,
        SubscriptionFilter, TaskPayload, Tool,
    },
    service::{RunningService, ServiceError, Subscription},
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
use veoveo_types::{ResourceUri, ServerSlug};

use crate::{
    calls::{
        CallContractError, ProviderContract, ResourceRead, TaskToolCall, TokenErrorCode, ToolCall,
    },
    hub::{HubToolName, HubToolNamespaceError},
};

pub mod catalog;
#[cfg(feature = "client-credentials")]
mod credentials;

pub use catalog::{CatalogError, CatalogListing};
#[cfg(feature = "client-credentials")]
pub use credentials::{ClientCredentialsBearer, CredentialsError, caller_signer};

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

    /// Drops a bearer the Hub no longer admits, so the next call obtains another.
    fn forget(&self, provider: &ServerSlug) {
        let _ = provider;
    }

    /// The bearer of the profile-wide requests a caller makes in its own name: catalog
    /// listings and reads by address. A source whose bearers each belong to one
    /// provider, such as a server's calls for its caller, has none.
    fn caller_bearer(&self) -> BoxFuture<'_, Result<Bearer, CallError>> {
        Box::pin(std::future::ready(Err(CallError::NoCallerBearer)))
    }

    /// Drops a caller bearer the Hub no longer admits.
    fn forget_caller(&self) {}
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

    fn caller_bearer(&self) -> BoxFuture<'_, Result<Bearer, CallError>> {
        Box::pin(std::future::ready(Ok(self.0.clone())))
    }
}

/// Why a call produced no result.
#[derive(Debug, thiserror::Error)]
pub enum CallError {
    /// A request or durable Task belongs to another immutable worker deployment.
    #[error("the invocation belongs to another server deployment")]
    DeploymentMismatch,
    /// The Hub refused to issue a token for the call, with the OAuth error of its
    /// token endpoint, such as `invalid_target` when no registered server provides a
    /// permission the call needs.
    #[error("the Hub refused the call: {error:?}{}", describe(.description))]
    Refused {
        error: TokenErrorCode,
        description: Option<String>,
    },
    /// The calling server declared no use of the provider's permissions.
    #[error("this server declares no use of {provider}'s permissions")]
    Undeclared { provider: ServerSlug },
    /// The Hub or the provider answered with a JSON-RPC error, such as a denied
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
    /// A resource read answered without the one JSON text content the contract reads.
    #[error("the resource {uri} has no JSON content: {reason}")]
    Content { uri: String, reason: String },
    /// The Hub did not accept a subscription to the resource, so its updates cannot
    /// be followed.
    #[error("the Hub did not accept a subscription to {uri}")]
    Unsubscribed { uri: String },
    #[error(transparent)]
    Contract(#[from] CallContractError),
    /// A call by name named no `{server}__{tool}` Hub tool.
    #[error("`{name}` is no Hub tool name: {error}")]
    ToolName {
        name: HubToolName,
        error: HubToolNamespaceError,
    },
    /// The bearer source holds no bearer of its own for profile-wide requests.
    #[error("this client holds no caller bearer for profile-wide requests")]
    NoCallerBearer,
    /// A catalog listing broke its paging bounds.
    #[error(transparent)]
    Catalog(#[from] CatalogError),
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

/// Whose bearer a connection carries: one provider's, for calls of that provider's
/// tools and reads of its resources, or the caller's own, for profile-wide requests.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Route {
    Caller,
    Provider(ServerSlug),
}

impl Route {
    async fn bearer(&self, bearers: &dyn BearerSource) -> Result<Bearer, CallError> {
        match self {
            Self::Caller => bearers.caller_bearer().await,
            Self::Provider(provider) => bearers.bearer(provider).await,
        }
    }

    fn forget(&self, bearers: &dyn BearerSource) {
        match self {
            Self::Caller => bearers.forget_caller(),
            Self::Provider(provider) => bearers.forget(provider),
        }
    }
}

/// The MCP connection for calls to one provider: it carries that provider's bearer.
struct Connection {
    service: Service,
}

/// Typed calls through one Hub profile.
#[derive(Clone)]
pub struct HubClient {
    inner: Arc<Inner>,
}

struct Inner {
    endpoint: url::Url,
    http: reqwest::Client,
    bearers: Arc<dyn BearerSource>,
    implementation: Implementation,
    connections: tokio::sync::Mutex<BTreeMap<Route, Arc<Connection>>>,
}

impl std::fmt::Debug for HubClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HubClient")
            .field("endpoint", &self.inner.endpoint.as_str())
            .finish_non_exhaustive()
    }
}

impl HubClient {
    /// A client of the profile whose MCP endpoint is `endpoint`, such as
    /// `https://hub.example/mcp/default`.
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
                implementation: Implementation::new("veoveo-hub-sdk", env!("CARGO_PKG_VERSION")),
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

    /// Reads a provider resource and resolves with its typed content. The builder can
    /// also wait for the content to reach a state, or follow its changes.
    pub fn read<R: ResourceRead>(&self, request: R) -> Read<R> {
        Read {
            client: self.clone(),
            request,
            timeout: None,
        }
    }

    /// The handle of a Task this client started earlier, such as one a durable step
    /// recorded before a restart.
    pub async fn task<R: TaskToolCall>(
        &self,
        task: impl Into<String>,
    ) -> Result<TaskHandle<R::Output>, CallError> {
        let connection = self
            .connection(&Route::Provider(R::Provider::slug()))
            .await?;
        Ok(TaskHandle::started(connection, task.into()))
    }

    /// Calls a tool by its Hub name, `{server}__{tool}`, with JSON arguments and
    /// resolves with its result, waiting for its Task when the tool runs as one. An
    /// error result is a result: the tool or the server's argument check reported it.
    /// The call carries the bearer of the server the name namespaces.
    pub fn call_tool(&self, name: HubToolName, arguments: JsonObject) -> CallByName {
        CallByName {
            client: self.clone(),
            name,
            arguments,
            timeout: None,
        }
    }

    /// Reads a resource by its address with the caller's bearer.
    pub async fn read_resource(&self, uri: &ResourceUri) -> Result<ReadResourceResult, CallError> {
        let params = ReadResourceRequestParams::new(uri.as_str());
        let (_, result) = self
            .request(&Route::Caller, |connection| {
                let params = params.clone();
                async move { connection.service.read_resource(params).await }
            })
            .await?;
        Ok(result)
    }

    /// Every tool the caller sees, across every page, with the caller's bearer.
    pub async fn tools(&self) -> Result<CatalogListing<Tool>, CallError> {
        self.list(catalog::tools).await
    }

    /// Every resource the caller sees, across every page.
    pub async fn resources(&self) -> Result<CatalogListing<Resource>, CallError> {
        self.list(catalog::resources).await
    }

    /// Every resource template the caller sees, across every page.
    pub async fn resource_templates(&self) -> Result<CatalogListing<ResourceTemplate>, CallError> {
        self.list(catalog::resource_templates).await
    }

    /// Every prompt the caller sees, across every page.
    pub async fn prompts(&self) -> Result<CatalogListing<Prompt>, CallError> {
        self.list(catalog::prompts).await
    }

    /// One complete listing through the caller's connection. A bearer the Hub
    /// stopped admitting is replaced once, as for every other request.
    async fn list<T, F>(&self, listing: F) -> Result<CatalogListing<T>, CallError>
    where
        F: for<'a> Fn(
            &'a rmcp::service::Peer<RoleClient>,
        ) -> BoxFuture<'a, Result<CatalogListing<T>, CatalogError>>,
    {
        let connection = self.connection(&Route::Caller).await?;
        match listing(connection.service.peer()).await {
            Err(CatalogError::Request { error, .. }) if unauthorized(&error) => {
                Route::Caller.forget(&*self.inner.bearers);
                self.inner.connections.lock().await.remove(&Route::Caller);
                let connection = self.connection(&Route::Caller).await?;
                Ok(listing(connection.service.peer()).await?)
            }
            listed => Ok(listed?),
        }
    }

    async fn connection(&self, route: &Route) -> Result<Arc<Connection>, CallError> {
        // The bearer comes first, so a refusal reaches the caller as its OAuth error.
        route.bearer(&*self.inner.bearers).await?;
        let mut connections = self.inner.connections.lock().await;
        if let Some(connection) = connections.get(route) {
            return Ok(connection.clone());
        }
        let service = match self.connect(route).await {
            Ok(service) => service,
            // The source's bearer is no longer admitted; the next one shows why.
            Err(error)
                if matches!(
                    &*error,
                    rmcp::service::ClientInitializeError::TransportError { error, .. }
                        if auth_required(error)
                ) =>
            {
                route.forget(&*self.inner.bearers);
                route.bearer(&*self.inner.bearers).await?;
                self.connect(route)
                    .await
                    .map_err(|error| CallError::Transport(error.to_string()))?
            }
            Err(error) => return Err(CallError::Transport(error.to_string())),
        };
        let connection = Arc::new(Connection { service });
        connections.insert(route.clone(), connection.clone());
        Ok(connection)
    }

    async fn connect(
        &self,
        route: &Route,
    ) -> Result<Service, Box<rmcp::service::ClientInitializeError>> {
        let transport = StreamableHttpClientTransport::with_client(
            BearerHttp {
                http: self.inner.http.clone(),
                bearers: self.inner.bearers.clone(),
                route: route.clone(),
            },
            StreamableHttpClientTransportConfig::with_uri(self.inner.endpoint.as_str()),
        );
        // The capability builder is a server-side rmcp feature; the client sets the Tasks
        // extension on the plain capability document.
        let mut capabilities = ClientCapabilities::default();
        capabilities.extensions.get_or_insert_default().insert(
            rmcp::model::TASKS_EXTENSION_ID.to_owned(),
            JsonObject::new(),
        );
        let config = ClientConfig::new(capabilities, self.inner.implementation.clone());
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
        let name = R::hub_tool_name()?;
        let arguments = match serde_json::to_value(request)? {
            serde_json::Value::Object(arguments) => arguments,
            _ => {
                return Err(CallError::Contract(CallContractError::Tool(R::TOOL)));
            }
        };
        let params = CallToolRequestParams::new(name.to_string()).with_arguments(arguments);
        self.request(&Route::Provider(R::Provider::slug()), |connection| {
            let params = params.clone();
            async move { connection.service.call_tool_once(params).await }
        })
        .await
    }

    /// Reads `uri` from `provider` as the JSON text it serves.
    async fn read_text(&self, provider: &ServerSlug, uri: &str) -> Result<String, CallError> {
        let params = ReadResourceRequestParams::new(uri);
        let (_, result) = self
            .request(&Route::Provider(provider.clone()), |connection| {
                let params = params.clone();
                async move { connection.service.read_resource(params).await }
            })
            .await?;
        json_text(uri, result)
    }

    /// Sends one request through the route's connection. A bearer the Hub stopped
    /// admitting, such as one whose Task ended, is replaced once, so the caller sees
    /// why the Hub refuses another.
    async fn request<T, F, Fut>(
        &self,
        route: &Route,
        send: F,
    ) -> Result<(Arc<Connection>, T), CallError>
    where
        F: Fn(Arc<Connection>) -> Fut,
        Fut: Future<Output = Result<T, ServiceError>>,
    {
        let connection = self.connection(route).await?;
        match send(connection.clone()).await {
            Ok(answer) => Ok((connection, answer)),
            Err(error) if unauthorized(&error) => {
                route.forget(&*self.inner.bearers);
                self.inner.connections.lock().await.remove(route);
                let connection = self.connection(route).await?;
                let answer = send(connection.clone()).await?;
                Ok((connection, answer))
            }
            Err(error) => Err(error.into()),
        }
    }
}

/// The one JSON text content a read of `uri` answered with.
fn json_text(uri: &str, result: ReadResourceResult) -> Result<String, CallError> {
    let content = |reason: &str| CallError::Content {
        uri: uri.to_owned(),
        reason: reason.to_owned(),
    };
    let mut texts = result
        .contents
        .into_iter()
        .filter_map(|contents| match contents {
            ResourceContents::TextResourceContents {
                uri: served,
                mime_type,
                text,
                ..
            } if served == uri => Some((mime_type, text)),
            _ => None,
        });
    let (mime_type, text) = texts
        .next()
        .ok_or_else(|| content("no text content names the resource"))?;
    if texts.next().is_some() {
        return Err(content("more than one text content names the resource"));
    }
    if mime_type
        .as_deref()
        .is_some_and(|mime_type| !is_json(mime_type))
    {
        return Err(content("the content is not application/json"));
    }
    Ok(text)
}

/// Whether a media type is JSON: `application/json` or a `+json` suffix (RFC 6839).
fn is_json(mime_type: &str) -> bool {
    let essence = mime_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();
    essence == "application/json" || essence.ends_with("+json")
}

/// A resource read that runs when awaited.
#[must_use = "a read runs when it is awaited"]
pub struct Read<R: ResourceRead> {
    client: HubClient,
    request: R,
    timeout: Option<Duration>,
}

impl<R: ResourceRead> Read<R> {
    /// Fails the read, or the wait of [`Read::until`], with [`CallError::Timeout`] when
    /// no answer arrives in time.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    /// Waits until the content satisfies `ready` and resolves with it. The read
    /// subscribes to the resource first and reads it again on each update the provider
    /// announces, so it waits without polling.
    pub fn until<F>(self, ready: F) -> Until<R, F>
    where
        F: FnMut(&R::Content) -> bool + Send + 'static,
    {
        Until { read: self, ready }
    }

    /// The content now and each time it changes. Typed subscription ends reopen
    /// with bounded backoff and a fresh snapshot; they do not end the observation.
    /// Dropping the stream stops observation and pending recovery. A read,
    /// acknowledgment, decode or subscription error yields one error and then ends
    /// the stream. [`Read::timeout`] intentionally does not apply; use an external
    /// deadline when consuming an otherwise untimed stream.
    pub fn updates(self) -> ResourceUpdates<R::Content> {
        let uri = self.request.uri();
        let provider = R::Provider::slug();
        let client = self.client;
        let state = match uri {
            Ok(uri) => Follow::Start {
                client,
                provider,
                uri: uri.to_string(),
            },
            Err(error) => Follow::Failed(Some(CallError::Contract(error))),
        };
        ResourceUpdates {
            stream: futures::stream::unfold(state, Follow::advance::<R::Content>).boxed(),
        }
    }
}

impl<R: ResourceRead> IntoFuture for Read<R> {
    type Output = Result<R::Content, CallError>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send>>;

    fn into_future(self) -> Self::IntoFuture {
        let Read {
            client,
            request,
            timeout,
        } = self;
        let read = async move {
            let uri = request.uri()?;
            let text = client.read_text(&R::Provider::slug(), uri.as_str()).await?;
            Ok(serde_json::from_str(&text)?)
        };
        Box::pin(within(timeout, read))
    }
}

/// A wait for a resource's content to reach a state, which runs when awaited.
#[must_use = "a wait runs when it is awaited"]
pub struct Until<R: ResourceRead, F> {
    read: Read<R>,
    ready: F,
}

impl<R: ResourceRead, F> Until<R, F> {
    /// Fails the wait with [`CallError::Timeout`] when the content has not reached the
    /// state in time.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.read.timeout = Some(timeout);
        self
    }
}

impl<R, F> IntoFuture for Until<R, F>
where
    R: ResourceRead,
    F: FnMut(&R::Content) -> bool + Send + 'static,
{
    type Output = Result<R::Content, CallError>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send>>;

    fn into_future(self) -> Self::IntoFuture {
        let Until { read, mut ready } = self;
        let timeout = read.timeout;
        let mut updates = read.updates();
        let wait = async move {
            while let Some(content) = updates.next().await {
                let content = content?;
                if ready(&content) {
                    return Ok(content);
                }
            }
            Err(CallError::Transport(
                "the subscription to the resource ended before it reached the state".into(),
            ))
        };
        Box::pin(within(timeout, wait))
    }
}

/// A resource's content now and each time it changes, from [`Read::updates`].
#[must_use = "streams do nothing unless polled"]
pub struct ResourceUpdates<C> {
    stream: BoxStream<'static, Result<C, CallError>>,
}

impl<C> std::fmt::Debug for ResourceUpdates<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResourceUpdates").finish_non_exhaustive()
    }
}

impl<C> futures::Stream for ResourceUpdates<C> {
    type Item = Result<C, CallError>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        context: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.stream.as_mut().poll_next(context)
    }
}

const SUBSCRIPTION_REOPEN_FIRST_DELAY: Duration = Duration::from_millis(100);
const SUBSCRIPTION_REOPEN_MAX_DELAY: Duration = Duration::from_secs(5);

/// Rate-limit ended subscriptions without limiting a healthy observation's lifetime.
/// Only observed time awaiting notifications counts as listening: a slow snapshot
/// read or a successful acknowledgment cannot establish stream health by itself.
struct SubscriptionRecovery {
    delay: Duration,
    listening: Duration,
}

impl Default for SubscriptionRecovery {
    fn default() -> Self {
        Self {
            delay: SUBSCRIPTION_REOPEN_FIRST_DELAY,
            listening: Duration::ZERO,
        }
    }
}

impl SubscriptionRecovery {
    async fn next(
        &mut self,
        subscription: &mut Subscription,
    ) -> Result<Option<ServerNotification>, ServiceError> {
        let started = tokio::time::Instant::now();
        let result = subscription.next().await;
        self.listening = self.listening.saturating_add(started.elapsed());
        result
    }

    async fn after_end(&mut self) {
        if self.listening >= SUBSCRIPTION_REOPEN_MAX_DELAY {
            self.delay = SUBSCRIPTION_REOPEN_FIRST_DELAY;
        }
        self.listening = Duration::ZERO;
        tokio::time::sleep(self.delay).await;
        self.delay = (self.delay * 2).min(SUBSCRIPTION_REOPEN_MAX_DELAY);
    }
}

/// The state of a followed resource.
enum Follow {
    Start {
        client: HubClient,
        provider: ServerSlug,
        uri: String,
    },
    Listening(Box<Listening>),
    Failed(Option<CallError>),
}

struct Listening {
    client: HubClient,
    provider: ServerSlug,
    uri: String,
    subscription: Subscription,
    /// The JSON text last yielded, so an update that changed nothing yields nothing.
    last: Option<String>,
    /// Whether the next step reads the resource before waiting for an update.
    stale: bool,
    recovery: SubscriptionRecovery,
}

impl Follow {
    async fn advance<C: DeserializeOwned>(self) -> Option<(Result<C, CallError>, Self)> {
        let mut listening = match self {
            Self::Failed(error) => return error.map(|error| (Err(error), Self::Failed(None))),
            Self::Start {
                client,
                provider,
                uri,
            } => match subscribe(&client, &provider, &uri).await {
                Ok(subscription) => Box::new(Listening {
                    client,
                    provider,
                    uri,
                    subscription,
                    last: None,
                    stale: true,
                    recovery: SubscriptionRecovery::default(),
                }),
                Err(error) => return Some((Err(error), Self::Failed(None))),
            },
            Self::Listening(listening) => listening,
        };
        loop {
            if listening.stale {
                listening.stale = false;
                let text = match listening
                    .client
                    .read_text(&listening.provider, &listening.uri)
                    .await
                {
                    Ok(text) => text,
                    Err(error) => return Some((Err(error), Self::Failed(None))),
                };
                if listening.last.as_deref() != Some(text.as_str()) {
                    let content = serde_json::from_str(&text).map_err(CallError::from);
                    listening.last = Some(text);
                    let next = if content.is_ok() {
                        Self::Listening(listening)
                    } else {
                        Self::Failed(None)
                    };
                    return Some((content, next));
                }
            }
            match listening.recovery.next(&mut listening.subscription).await {
                Ok(Some(ServerNotification::ResourceUpdatedNotification(update)))
                    if update.params.uri == listening.uri =>
                {
                    listening.stale = true;
                }
                Ok(Some(_)) => {}
                // The provider or the Hub ended the subscription: open another and
                // read again, so a change made in between is not missed.
                Ok(None) => {
                    listening.recovery.after_end().await;
                    match subscribe(&listening.client, &listening.provider, &listening.uri).await {
                        Ok(subscription) => {
                            listening.subscription = subscription;
                            listening.stale = true;
                        }
                        Err(error) => return Some((Err(error), Self::Failed(None))),
                    }
                }
                Err(error) => return Some((Err(error.into()), Self::Failed(None))),
            }
        }
    }
}

/// Subscribes to updates of `uri` through `provider`'s connection.
async fn subscribe(
    client: &HubClient,
    provider: &ServerSlug,
    uri: &str,
) -> Result<Subscription, CallError> {
    let filter = SubscriptionFilter::builder()
        .resource_subscriptions([uri.to_owned()])
        .build();
    let (_, subscription) = client
        .request(&Route::Provider(provider.clone()), |connection| {
            let filter = filter.clone();
            async move { connection.service.listen(filter).await }
        })
        .await?;
    if subscription
        .acknowledged()
        .resource_subscriptions
        .as_ref()
        .is_none_or(|uris| !uris.iter().any(|accepted| accepted == uri))
    {
        return Err(CallError::Unsubscribed {
            uri: uri.to_owned(),
        });
    }
    Ok(subscription)
}

/// Whether the Hub answered HTTP 401 to the bearer a request carried.
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
    client: HubClient,
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

/// A call of a tool by its Hub name, which runs when awaited.
#[must_use = "a call runs when it is awaited"]
pub struct CallByName {
    client: HubClient,
    name: HubToolName,
    arguments: JsonObject,
    timeout: Option<Duration>,
}

impl CallByName {
    /// Fails the call with [`CallError::Timeout`] when no result arrives in time,
    /// including the time its Task runs.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }
}

impl IntoFuture for CallByName {
    type Output = Result<CallToolResult, CallError>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send>>;

    fn into_future(self) -> Self::IntoFuture {
        let CallByName {
            client,
            name,
            arguments,
            timeout,
        } = self;
        let call = async move {
            let (provider, _) = name.parts().map_err(|error| CallError::ToolName {
                name: name.clone(),
                error,
            })?;
            let params = CallToolRequestParams::new(name.to_string()).with_arguments(arguments);
            let answer = client
                .request(&Route::Provider(provider), |connection| {
                    let params = params.clone();
                    async move { connection.service.call_tool_once(params).await }
                })
                .await?;
            match answer {
                (_, CallToolResponse::Complete(result)) => Ok(result),
                (connection, CallToolResponse::Task(created)) => {
                    task_result(&connection, &created.task.task_id).await
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
    client: HubClient,
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
        task_result(&self.connection, &task).await.and_then(decode)
    }
}

/// The tool result of `task` once it finishes, delivered through a Task status
/// subscription on `connection`.
async fn task_result(connection: &Connection, task: &str) -> Result<CallToolResult, CallError> {
    let filter = SubscriptionFilter::builder().task_id(task).build();
    // Each acknowledged subscription delivers current status. Reopen only ended
    // observations, within the caller's deadline, without replaying the tool call.
    let mut recovery = SubscriptionRecovery::default();
    loop {
        let mut subscription = connection.service.listen(filter.clone()).await?;
        if subscription
            .acknowledged()
            .task_ids
            .as_ref()
            .is_none_or(|ids| !ids.iter().any(|id| id == task))
        {
            return Err(CallError::Transport(format!(
                "the Hub did not acknowledge a subscription to Task {task}"
            )));
        }
        while let Some(notification) = recovery.next(&mut subscription).await? {
            let ServerNotification::TaskStatusNotification(notification) = notification else {
                continue;
            };
            let detailed = notification.params.task;
            if detailed.task.task_id != task {
                continue;
            }
            if let Some(outcome) = outcome(task, detailed) {
                let _ = subscription.cancel().await;
                return outcome;
            }
        }
        recovery.after_end().await;
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

/// The tool result of a finished Task, or `None` while it runs.
fn outcome(task: &str, detailed: DetailedTask) -> Option<Result<CallToolResult, CallError>> {
    match detailed.payload {
        TaskPayload::Working | TaskPayload::InputRequired { .. } => None,
        TaskPayload::Completed { result } => Some(
            serde_json::from_value::<CallToolResult>(serde_json::Value::Object(result))
                .map_err(CallError::from),
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

/// The HTTP client of one connection: every request carries the bearer the source
/// holds for the connection's route at the time it is sent.
#[derive(Clone)]
struct BearerHttp {
    http: reqwest::Client,
    bearers: Arc<dyn BearerSource>,
    route: Route,
}

impl BearerHttp {
    async fn bearer(&self) -> Result<String, StreamableHttpError<reqwest::Error>> {
        self.route
            .bearer(&*self.bearers)
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

#[cfg(test)]
mod tests {
    use rmcp::model::{ReadResourceResult, ResourceContents};

    use super::{CallError, is_json, json_text};

    const URI: &str = "ledger://entries/batch-1";

    fn read(contents: Vec<ResourceContents>) -> ReadResourceResult {
        ReadResourceResult::new(contents)
    }

    fn json(uri: &str, text: &str) -> ResourceContents {
        ResourceContents::text(text, uri).with_mime_type("application/json")
    }

    #[test]
    fn a_read_yields_the_one_json_text_naming_the_resource() {
        let text = json_text(URI, read(vec![json(URI, r#"{"state":"open"}"#)])).unwrap();
        assert_eq!(text, r#"{"state":"open"}"#);
        // Content for another URI is not this resource's.
        let text = json_text(
            URI,
            read(vec![json("ledger://entries/other", "{}"), json(URI, "[]")]),
        )
        .unwrap();
        assert_eq!(text, "[]");
    }

    #[test]
    fn a_read_without_one_json_text_is_no_content() {
        for result in [
            read(vec![]),
            read(vec![json("ledger://entries/other", "{}")]),
            read(vec![json(URI, "{}"), json(URI, "{}")]),
            read(vec![ResourceContents::text("plain", URI)]),
            read(vec![ResourceContents::blob("e30=", URI)]),
        ] {
            assert!(
                matches!(json_text(URI, result), Err(CallError::Content { .. })),
                "accepted a read without one JSON text"
            );
        }
    }

    #[test]
    fn json_media_types_include_structured_suffixes() {
        for mime in [
            "application/json",
            "Application/JSON; charset=utf-8",
            "application/problem+json",
        ] {
            assert!(is_json(mime), "{mime}");
        }
        for mime in ["text/plain", "application/jsonl", "text/json-ish"] {
            assert!(!is_json(mime), "{mime}");
        }
    }
}

#[cfg(all(test, feature = "server"))]
#[path = "client/observation_tests.rs"]
mod observation_tests;

#[cfg(test)]
#[path = "client/recovery_policy_tests.rs"]
mod recovery_policy_tests;
