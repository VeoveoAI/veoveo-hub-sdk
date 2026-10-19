//! The domain side of a hosted server, and the one `ServerHandler` that hosts it.
//!
//! A server implements [`DomainServer`] with ready [`super::Routes`], binding
//! typed handlers, resource readers and prompt renderers to policies. It never implements rmcp's
//! `ServerHandler`. [`Hosted`] does that once for every server and supplies the
//! shared protocol behavior:
//!
//! - protocol versions and `get_info` from the checked setup;
//! - authenticated, paged `resources/list`, `resources/templates/list` and
//!   `tools/list`, with every tool argument declaring one JSON type;
//! - `{scheme}://docs` and `{scheme}://contract` reads, and `doc_id` completion;
//! - `prompts/list` and `prompts/get` from the registered prompt policies and renderers;
//! - address admission: an unparseable URI is Invalid Params (-32602), as the
//!   server contract requires, before any domain code runs;
//! - the explicit cache policy of each typed resource route;
//! - tool dispatch, with durable tasks started first when the server has them, and the
//!   published input schema's violations appended to a call its arguments failed;
//! - `tasks/*` and `subscriptions/listen` through its [`TaskSupport`].

use std::future::Future;

use rmcp::{
    ErrorData, RoleServer, ServerHandler,
    model::{
        CallToolRequestParams, CallToolResponse, CancelTaskParams, CompleteRequestParams,
        CompleteResult, CreateTaskResult, GetPromptRequestParams, GetPromptResponse, GetTaskParams,
        GetTaskResult, ListPromptsResult, ListResourceTemplatesResult, ListResourcesResult,
        ListToolsResult, PaginatedRequestParams, ProtocolVersion, ReadResourceRequestParams,
        ReadResourceResponse, ReadResourceResult, ServerConfig, SubscriptionFilter, Tool,
        UpdateTaskParams,
    },
    service::{RequestContext, SubscriptionContext},
};
use veoveo_types::ResourceUri;

use super::auth::hub_identity;
use crate::{
    final_protocol_versions, private_resource_response,
    server_contract::{McpServerContract, McpServerSetup},
};

/// How long a client may reuse a domain read. Both policies are private to the
/// caller; a read carrying request state or input responses is never reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadCache {
    /// Reusable for [`PRIVATE_RESOURCE_TTL_MS`](crate::PRIVATE_RESOURCE_TTL_MS).
    Private,
    /// Never reused, for content whose access or freshness can change between
    /// reads.
    NoStore,
}

/// A domain read and the cache policy the server chose for it. There is no
/// default policy: each read names one.
#[derive(Debug, Clone)]
pub(crate) struct DomainRead {
    result: ReadResourceResult,
    cache: ReadCache,
}

impl DomainRead {
    fn into_response(self, request: &ReadResourceRequestParams) -> ReadResourceResponse {
        let continuation = request.request_state.is_some() || request.input_responses.is_some();
        let cacheable = self.cache == ReadCache::Private && !continuation;
        private_resource_response(self.result, cacheable)
    }
}

/// A server's domain behavior. Implement this; [`Hosted`] provides the protocol.
pub trait DomainServer: Clone + Send + Sync + Sized + 'static {
    /// The server's checked contract: scopes, resource addresses, and documents.
    type Contract: McpServerContract<Server = Self> + 'static;

    /// The checked setup, usually a `LazyLock` forced at startup.
    fn setup() -> &'static McpServerSetup<Self::Contract>;

    /// The ready typed routes, shared by discovery, registration and dispatch.
    fn routes() -> &'static super::Routes<Self>;

    /// Completes template arguments. The host completes `doc_id` on the docs
    /// template before calling this. The default offers no completions.
    fn complete(
        &self,
        request: CompleteRequestParams,
        context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<CompleteResult, ErrorData>> + Send {
        let _ = (request, context);
        std::future::ready(Ok(CompleteResult::default()))
    }
}

/// The error for a prompt name the server does not declare.
pub fn unknown_prompt(name: &str) -> ErrorData {
    ErrorData::invalid_params(format!("unknown prompt `{name}`"), None)
}

/// The answer for a well-known variant in a domain `read`: the host serves
/// documents and the contract before domain dispatch, so this is unreachable in
/// a hosted server and fails closed if it is ever reached.
pub fn served_by_host() -> ErrorData {
    ErrorData::internal_error("well-known resources are served by the host", None)
}

/// Durable tasks and subscription delivery for a hosted server.
///
/// [`NoTasks`] is the implementation for servers without tasks. Servers with
/// durable tasks use the task runtime's implementation.
pub trait TaskSupport: Send + Sync + 'static {
    /// Starts a durable task when the request asks for one; `None` dispatches the
    /// call as an ordinary tool.
    fn start_task(
        &self,
        request: &mut CallToolRequestParams,
        context: &RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<Option<CreateTaskResult>, ErrorData>> + Send;

    fn get_task(
        &self,
        request: GetTaskParams,
        context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<GetTaskResult, ErrorData>> + Send;

    fn update_task(
        &self,
        request: UpdateTaskParams,
        context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<(), ErrorData>> + Send;

    fn cancel_task(
        &self,
        request: CancelTaskParams,
        context: RequestContext<RoleServer>,
    ) -> impl Future<Output = Result<(), ErrorData>> + Send;

    /// The subset of a `subscriptions/listen` filter this server accepts.
    fn accepted_subscription_filter(
        &self,
        requested: &SubscriptionFilter,
    ) -> Option<SubscriptionFilter>;

    fn listen(
        &self,
        context: SubscriptionContext,
    ) -> impl Future<Output = Result<(), ErrorData>> + Send;
}

/// Task support for servers without durable tasks or subscriptions.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoTasks;

pub(super) fn no_tasks() -> ErrorData {
    ErrorData::invalid_request("this server has no tasks", None)
}

impl TaskSupport for NoTasks {
    async fn start_task(
        &self,
        _request: &mut CallToolRequestParams,
        _context: &RequestContext<RoleServer>,
    ) -> Result<Option<CreateTaskResult>, ErrorData> {
        Ok(None)
    }

    async fn get_task(
        &self,
        _request: GetTaskParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<GetTaskResult, ErrorData> {
        Err(no_tasks())
    }

    async fn update_task(
        &self,
        _request: UpdateTaskParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<(), ErrorData> {
        Err(no_tasks())
    }

    async fn cancel_task(
        &self,
        _request: CancelTaskParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<(), ErrorData> {
        Err(no_tasks())
    }

    fn accepted_subscription_filter(
        &self,
        _requested: &SubscriptionFilter,
    ) -> Option<SubscriptionFilter> {
        None
    }

    async fn listen(&self, context: SubscriptionContext) -> Result<(), ErrorData> {
        context.cancelled().await;
        Ok(())
    }
}

/// The `ServerHandler` for every hosted server: a domain plus its task support.
#[derive(Clone)]
pub struct Hosted<D, T = NoTasks> {
    domain: D,
    tasks: T,
}

impl<D: DomainServer> Hosted<D, NoTasks> {
    pub fn new(domain: D) -> Self {
        Self {
            domain,
            tasks: NoTasks,
        }
    }
}

impl<D: DomainServer, T> Hosted<D, T> {
    /// Replaces the task support, for servers with durable tasks.
    pub fn with_tasks<U: TaskSupport>(self, tasks: U) -> Hosted<D, U> {
        Hosted {
            domain: self.domain,
            tasks,
        }
    }

    pub fn domain(&self) -> &D {
        &self.domain
    }

    fn setup() -> &'static McpServerSetup<D::Contract> {
        D::setup()
    }
}

impl<D: DomainServer, T: TaskSupport> ServerHandler for Hosted<D, T> {
    fn supported_protocol_versions(&self) -> std::borrow::Cow<'static, [ProtocolVersion]> {
        final_protocol_versions()
    }

    fn get_info(&self) -> ServerConfig {
        Self::setup().server_config().clone()
    }

    async fn list_resources(
        &self,
        request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        hub_identity(&context)?;
        let resources = super::Listing::all(
            Self::setup()
                .declared_resources()
                .into_iter()
                .filter(|resource| D::routes().resource_visible(&resource.uri, &context))
                .collect(),
        );
        Self::setup().list_resources(resources, request.as_ref())
    }

    async fn list_resource_templates(
        &self,
        request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, ErrorData> {
        hub_identity(&context)?;
        let templates = super::Listing::all(
            Self::setup()
                .declared_resource_templates()
                .into_iter()
                .filter(|template| D::routes().template_visible(&template.uri_template, &context))
                .collect(),
        );
        Self::setup().list_resource_templates(templates, request.as_ref())
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, ErrorData> {
        if let Some(address) = Self::setup().document_address(&request.uri) {
            let _ = address;
            D::routes().authorize_documents(&context)?;
        }
        if let Some(result) = Self::setup().read_documents(&request, &context)? {
            return Ok(result);
        }
        let address = ResourceUri::new(request.uri.as_str())
            .map_err(|_| ErrorData::invalid_params("invalid resource URI", None))?;
        let (result, cache) = D::routes()
            .read(self.domain.clone(), address, context)
            .await?;
        Ok(DomainRead { result, cache }.into_response(&request))
    }

    async fn complete(
        &self,
        request: CompleteRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CompleteResult, ErrorData> {
        if Self::setup().is_document_completion(&request) {
            D::routes().authorize_documents(&context)?;
        }
        if let Some(result) = Self::setup().complete_documents(&request, &context)? {
            return Ok(result);
        }
        self.domain.complete(request, context).await
    }

    async fn list_prompts(
        &self,
        request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, ErrorData> {
        Self::setup().list_prompts(
            D::routes().list_prompts(&context),
            request.as_ref(),
            &context,
        )
    }

    async fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<GetPromptResponse, ErrorData> {
        D::routes()
            .prompt(self.domain.clone(), request, context)
            .await
            .map(Into::into)
    }

    async fn list_tools(
        &self,
        request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        hub_identity(&context)?;
        let tools = super::Listing::all(D::routes().list_tools(&context));
        Self::setup().list_tools(tools, request.as_ref())
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        D::routes().get_tool(name)
    }

    async fn call_tool(
        &self,
        mut request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        D::routes().authorize_tool(&request.name, &context)?;
        if let Some(created) = self.tasks.start_task(&mut request, &context).await? {
            return Ok(created.into());
        }
        let name = request.name.clone();
        let arguments = request.arguments.clone();
        let mut response: CallToolResponse = D::routes()
            .call(self.domain.clone(), request, context)
            .await?
            .into();
        if let CallToolResponse::Complete(result) = &mut response
            && result.is_error == Some(true)
            && let Some(tool) = self.get_tool(&name)
        {
            super::arguments::explain::<D>(&tool, arguments.as_ref(), result);
        }
        Ok(response)
    }

    async fn get_task(
        &self,
        request: GetTaskParams,
        context: RequestContext<RoleServer>,
    ) -> Result<GetTaskResult, ErrorData> {
        self.tasks.get_task(request, context).await
    }

    async fn update_task(
        &self,
        request: UpdateTaskParams,
        context: RequestContext<RoleServer>,
    ) -> Result<(), ErrorData> {
        self.tasks.update_task(request, context).await
    }

    async fn cancel_task(
        &self,
        request: CancelTaskParams,
        context: RequestContext<RoleServer>,
    ) -> Result<(), ErrorData> {
        self.tasks.cancel_task(request, context).await
    }

    fn accepted_subscription_filter(
        &self,
        requested: &SubscriptionFilter,
    ) -> Option<SubscriptionFilter> {
        self.tasks.accepted_subscription_filter(requested)
    }

    async fn listen(&self, context: SubscriptionContext) -> Result<(), ErrorData> {
        self.tasks.listen(context).await
    }
}
