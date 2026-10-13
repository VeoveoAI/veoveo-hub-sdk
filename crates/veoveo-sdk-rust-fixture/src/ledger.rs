//! Ledger, the provider of the calls fixture: three tools that report the forwarded
//! identity they received, one of them a Task kept in memory.
use std::{
    collections::HashMap,
    sync::{Arc, LazyLock},
    time::Duration,
};

use rmcp::{
    ErrorData, RoleServer,
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{
        CallToolRequestParams, CallToolResult, CancelTaskParams, ContentBlock, CreateTaskResult,
        DetailedTask, GetTaskParams, GetTaskResult, ReadResourceRequestParams, ServerCapabilities,
        ServerConfig, SubscriptionFilter, Task, TaskPayload, UpdateTaskParams,
    },
    service::{RequestContext, SubscriptionContext},
    tool, tool_router,
};
use serde::Serialize;
use tokio::sync::broadcast;
use veoveo_sdk_rust::{
    GatewayInternalIdentity, ServerSlug,
    hosting::{DomainAddress, DomainRead, DomainServer, Hosted, TaskSupport, gateway_identity},
    registration::{AuthorizationBuilder, AuthorizationDeclaration, DeclarationError},
    server_contract::{McpServerContract, McpServerSetup},
};
use veoveo_types::ResourceScheme;

use crate::calls::{Inspection, LedgerPermission, Seal, Tallied, Tally};
use crate::calls_support::{CallsResource, NoScope, contract_config, docs, require};

pub struct LedgerContract;

static DOCS: LazyLock<veoveo_sdk_rust::docs::ServerDocs> =
    LazyLock::new(|| docs("ledger", include_str!("../testdata/ledger-compliance.json")));
static SETUP: LazyLock<McpServerSetup<LedgerContract>> =
    LazyLock::new(|| McpServerSetup::new().expect("Ledger setup"));

impl McpServerContract for LedgerContract {
    type Scope = NoScope;
    type Resource = CallsResource<LedgerContract>;
    fn slug() -> ServerSlug {
        ServerSlug::parse("ledger").unwrap()
    }
    fn scheme() -> ResourceScheme {
        ResourceScheme::parse("ledger").unwrap()
    }
    fn scopes() -> &'static [NoScope] {
        NoScope::ALL
    }
    fn documents() -> &'static veoveo_sdk_rust::docs::ServerDocs {
        &DOCS
    }
    fn server_config() -> ServerConfig {
        contract_config(
            "ledger",
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .enable_completions()
                .build(),
        )
    }
    fn resources() -> Result<
        Vec<veoveo_sdk_rust::server_contract::McpResource<Self::Resource>>,
        veoveo_sdk_rust::server_contract::McpSetupError,
    > {
        CallsResource::declared()
    }
    fn resource_templates() -> Result<
        Vec<veoveo_sdk_rust::server_contract::McpResourceTemplate>,
        veoveo_sdk_rust::server_contract::McpSetupError,
    > {
        CallsResource::<Self>::templates()
    }
}

/// Ledger's declaration: each tool requires one Ledger permission.
pub fn authorization() -> Result<AuthorizationDeclaration, DeclarationError> {
    let tool = |name: &str| veoveo_sdk_rust::LocalToolName::parse(name).unwrap();
    AuthorizationBuilder::<LedgerPermission>::new()
        .tool(tool("inspect"), LedgerPermission::EntriesRead)
        .tool(tool("read_seal"), LedgerPermission::SealsRead)
        .tool(tool("tally"), LedgerPermission::EntriesRead)
        .build(&LedgerContract::slug(), &LedgerContract::scheme())
}

/// What Ledger reports about the call that reached it.
pub fn inspection(identity: &GatewayInternalIdentity) -> Inspection {
    crate::calls_support::inspection(identity)
}

fn structured<T: Serialize>(text: &str, value: &T) -> Result<CallToolResult, ErrorData> {
    let mut result = CallToolResult::success(vec![ContentBlock::text(text)]);
    result.structured_content = Some(
        serde_json::to_value(value)
            .map_err(|error| ErrorData::internal_error(error.to_string(), None))?,
    );
    Ok(result)
}

#[derive(Clone)]
pub struct LedgerDomain {
    tool_router: ToolRouter<Self>,
}

impl Default for LedgerDomain {
    fn default() -> Self {
        Self::new()
    }
}

#[tool_router]
impl LedgerDomain {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }

    #[tool(description = "Report the identity this call carries.")]
    async fn inspect(
        &self,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let identity = require(&context, LedgerPermission::EntriesRead)?;
        structured("inspected", &inspection(&identity))
    }

    #[tool(description = "Read the ledger's seal, a sensitive value.")]
    async fn read_seal(
        &self,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        require(&context, LedgerPermission::SealsRead)?;
        structured(
            "sealed",
            &Seal {
                value: "seal-7".into(),
            },
        )
    }

    #[tool(description = "Count entries in a Task.")]
    async fn tally(
        &self,
        Parameters(request): Parameters<Tally>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        let identity = require(&context, LedgerPermission::EntriesRead)?;
        structured(
            "tallied",
            &Tallied {
                entries: request.entries,
                inspection: inspection(&identity),
            },
        )
    }
}

impl DomainServer for LedgerDomain {
    type Contract = LedgerContract;
    fn setup() -> &'static McpServerSetup<LedgerContract> {
        &SETUP
    }
    fn tool_router(&self) -> &ToolRouter<Self> {
        &self.tool_router
    }
    async fn read(
        &self,
        _address: DomainAddress<LedgerContract>,
        _request: &ReadResourceRequestParams,
        _context: &RequestContext<RoleServer>,
    ) -> Result<DomainRead, ErrorData> {
        Err(veoveo_sdk_rust::hosting::served_by_host())
    }
}

/// Ledger's handler with its in-memory Tasks.
pub fn hosted(tasks: LedgerTasks) -> Hosted<LedgerDomain, LedgerTasks> {
    Hosted::new(LedgerDomain::new()).with_tasks(tasks)
}

struct StoredTask {
    owner: String,
    task: Task,
    payload: TaskPayload,
}

/// `tally` Tasks, kept in memory. Each completes shortly after it starts.
#[derive(Clone)]
pub struct LedgerTasks {
    tasks: Arc<parking_lot::Mutex<HashMap<String, StoredTask>>>,
    changes: broadcast::Sender<String>,
    delay: Duration,
}

impl Default for LedgerTasks {
    fn default() -> Self {
        Self::new(Duration::from_millis(50))
    }
}

impl LedgerTasks {
    /// Tasks that complete `delay` after they start.
    pub fn new(delay: Duration) -> Self {
        Self {
            tasks: Arc::default(),
            changes: broadcast::channel(64).0,
            delay,
        }
    }

    fn detailed(&self, task: &str, owner: &str) -> Result<DetailedTask, ErrorData> {
        let tasks = self.tasks.lock();
        let stored = tasks
            .get(task)
            .filter(|stored| stored.owner == owner)
            .ok_or_else(|| ErrorData::invalid_params("unknown task id", None))?;
        Ok(DetailedTask::new(
            stored.task.clone(),
            stored.payload.clone(),
        ))
    }

    fn settle(&self, task: &str, payload: TaskPayload) {
        if let Some(stored) = self.tasks.lock().get_mut(task) {
            stored.task.last_updated_at = now();
            stored.payload = payload;
        }
        let _ = self.changes.send(task.to_owned());
    }
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

impl TaskSupport for LedgerTasks {
    async fn start_task(
        &self,
        request: &mut CallToolRequestParams,
        context: &RequestContext<RoleServer>,
    ) -> Result<Option<CreateTaskResult>, ErrorData> {
        if request.name != "tally"
            || !context
                .meta
                .client_capabilities()
                .is_some_and(|capabilities| capabilities.supports_tasks())
        {
            return Ok(None);
        }
        let identity = require(context, LedgerPermission::EntriesRead)?;
        let tally: Tally = serde_json::from_value(serde_json::Value::Object(
            request.arguments.clone().unwrap_or_default(),
        ))
        .map_err(|error| ErrorData::invalid_params(error.to_string(), None))?;
        let id = uuid::Uuid::now_v7().to_string();
        let created = now();
        let task = Task::new(
            id.clone(),
            rmcp::model::TaskStatus::Working,
            created.clone(),
            created,
        );
        self.tasks.lock().insert(
            id.clone(),
            StoredTask {
                owner: identity.actor.id.to_string(),
                task: task.clone(),
                payload: TaskPayload::Working,
            },
        );
        let result = structured(
            "tallied",
            &Tallied {
                entries: tally.entries,
                inspection: inspection(&identity),
            },
        )?;
        let tasks = self.clone();
        let delay = self.delay;
        tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            let result = match serde_json::to_value(result) {
                Ok(serde_json::Value::Object(result)) => result,
                _ => return,
            };
            tasks.settle(&id, TaskPayload::Completed { result });
        });
        Ok(Some(CreateTaskResult::new(task)))
    }

    async fn get_task(
        &self,
        request: GetTaskParams,
        context: RequestContext<RoleServer>,
    ) -> Result<GetTaskResult, ErrorData> {
        let identity = gateway_identity(&context)?;
        let task = self.detailed(&request.task_id, identity.actor.id.as_str())?;
        Ok(GetTaskResult::new(task))
    }

    async fn update_task(
        &self,
        _request: UpdateTaskParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<(), ErrorData> {
        Err(ErrorData::invalid_request(
            "Ledger Tasks take no input",
            None,
        ))
    }

    async fn cancel_task(
        &self,
        request: CancelTaskParams,
        context: RequestContext<RoleServer>,
    ) -> Result<(), ErrorData> {
        let identity = gateway_identity(&context)?;
        self.detailed(&request.task_id, identity.actor.id.as_str())?;
        self.settle(&request.task_id, TaskPayload::Cancelled);
        Ok(())
    }

    fn accepted_subscription_filter(
        &self,
        requested: &SubscriptionFilter,
    ) -> Option<SubscriptionFilter> {
        veoveo_sdk_rust::accepted_task_subscription_filter(requested)
    }

    async fn listen(&self, context: SubscriptionContext) -> Result<(), ErrorData> {
        let identity = gateway_identity(context.request_context())?;
        let owner = identity.actor.id.to_string();
        let watched = context.accepted().task_ids.clone().unwrap_or_default();
        let mut changes = self.changes.subscribe();
        let send = |task: DetailedTask| {
            let context = &context;
            async move {
                context
                    .sink()
                    .notify_task_status(task)
                    .await
                    .map_err(|error| ErrorData::internal_error(error.to_string(), None))
            }
        };
        for task in &watched {
            send(self.detailed(task, &owner)?).await?;
        }
        loop {
            tokio::select! {
                () = context.cancelled() => return Ok(()),
                changed = changes.recv() => {
                    let Ok(task) = changed else { return Ok(()) };
                    if watched.contains(&task) {
                        send(self.detailed(&task, &owner)?).await?;
                    }
                }
            }
        }
    }
}
