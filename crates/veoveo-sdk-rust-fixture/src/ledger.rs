//! Ledger, the provider of the calls fixture: three tools that report the forwarded
//! identity they received, one of them a Task kept in memory, and entries that close a
//! while after they open, served as `ledger://entries/{entry}` with resource updates.
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
        DetailedTask, GetTaskParams, GetTaskResult, ReadResourceRequestParams, ResourceTemplate,
        ServerCapabilities, ServerConfig, SubscriptionFilter, Task, TaskPayload, UpdateTaskParams,
    },
    service::{RequestContext, SubscriptionContext},
    tool, tool_router,
};
use serde::Serialize;
use tokio::sync::broadcast;
use veoveo_sdk_rust::{
    GatewayInternalIdentity, ServerSlug,
    hosting::{
        DomainAddress, DomainRead, DomainServer, Hosted, TaskSupport, gateway_identity, json_read,
    },
    registration::{AuthorizationBuilder, AuthorizationDeclaration, DeclarationError},
    server_contract::{McpResource, McpResourceTemplate, McpServerContract, McpServerSetup},
};
use veoveo_types::{
    IdentifierError, ResourceAddress, ResourceScheme, ResourceTemplateUri, ResourceUri,
};

use crate::calls::{
    ENTRY_TEMPLATE, Entry, EntryName, EntryState, Inspection, LedgerAddress, LedgerPermission,
    OpenEntry, Seal, Tallied, Tally,
};
use crate::calls_support::{CallsResource, NoScope, contract_config, docs, require};

pub struct LedgerContract;

static DOCS: LazyLock<veoveo_sdk_rust::docs::ServerDocs> =
    LazyLock::new(|| docs("ledger", include_str!("../testdata/ledger-compliance.json")));
static SETUP: LazyLock<McpServerSetup<LedgerContract>> =
    LazyLock::new(|| McpServerSetup::new().expect("Ledger setup"));

/// The addresses Ledger serves: the host's documents and contract, and its entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LedgerResource {
    Host(CallsResource<LedgerContract>),
    Entry(EntryName),
}

impl ResourceAddress for LedgerResource {
    type Error = IdentifierError;
    fn parse(uri: &ResourceUri) -> Result<Self, Self::Error> {
        match LedgerAddress::parse(uri) {
            Ok(LedgerAddress::Entry(entry)) => Ok(Self::Entry(entry)),
            Err(_) => CallsResource::parse(uri).map(Self::Host),
        }
    }
    fn to_uri(&self) -> Result<ResourceUri, Self::Error> {
        match self {
            Self::Host(address) => address.to_uri(),
            Self::Entry(entry) => LedgerAddress::Entry(entry.clone())
                .to_uri()
                .map_err(|_| IdentifierError::new(entry.as_str(), "invalid entry address")),
        }
    }
}

impl McpServerContract for LedgerContract {
    type Scope = NoScope;
    type Resource = LedgerResource;
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
                .enable_resources_subscribe()
                .enable_completions()
                .build(),
        )
    }
    fn resources()
    -> Result<Vec<McpResource<Self::Resource>>, veoveo_sdk_rust::server_contract::McpSetupError>
    {
        CallsResource::declared_as(LedgerResource::Host)
    }
    fn resource_templates()
    -> Result<Vec<McpResourceTemplate>, veoveo_sdk_rust::server_contract::McpSetupError> {
        let mut templates = CallsResource::<Self>::templates()?;
        templates.push(McpResourceTemplate::new(entry_template(), |uri| {
            ResourceTemplate::new(uri, "entry")
        })?);
        Ok(templates)
    }
}

fn entry_template() -> ResourceTemplateUri {
    ResourceTemplateUri::new(ENTRY_TEMPLATE).expect("the entry template is valid")
}

/// Ledger's declaration: each tool and the entry template require one Ledger permission.
pub fn authorization() -> Result<AuthorizationDeclaration, DeclarationError> {
    let tool = |name: &str| veoveo_sdk_rust::LocalToolName::parse(name).unwrap();
    AuthorizationBuilder::<LedgerPermission>::new()
        .tool(tool("inspect"), LedgerPermission::EntriesRead)
        .tool(tool("read_seal"), LedgerPermission::SealsRead)
        .tool(tool("tally"), LedgerPermission::EntriesRead)
        .tool(tool("open_entry"), LedgerPermission::EntriesWrite)
        .resource_template(entry_template(), LedgerPermission::EntriesRead)
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
    state: LedgerTasks,
}

impl Default for LedgerDomain {
    fn default() -> Self {
        Self::new()
    }
}

#[tool_router]
impl LedgerDomain {
    pub fn new() -> Self {
        Self::with_state(LedgerTasks::default())
    }

    /// Ledger over shared state: its Tasks and entries.
    pub fn with_state(state: LedgerTasks) -> Self {
        Self {
            tool_router: Self::tool_router(),
            state,
        }
    }

    #[tool(description = "Open an entry, which closes after the given number of milliseconds.")]
    async fn open_entry(
        &self,
        Parameters(request): Parameters<OpenEntry>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        require(&context, LedgerPermission::EntriesWrite)?;
        let entry = self
            .state
            .open(request.entry, Duration::from_millis(request.close_after_ms))
            .await;
        structured("opened", &entry)
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
        address: DomainAddress<LedgerContract>,
        request: &ReadResourceRequestParams,
        context: &RequestContext<RoleServer>,
    ) -> Result<DomainRead, ErrorData> {
        match address {
            LedgerResource::Host(_) => Err(veoveo_sdk_rust::hosting::served_by_host()),
            LedgerResource::Entry(name) => {
                require(context, LedgerPermission::EntriesRead)?;
                let entry = self
                    .state
                    .entry(&name)
                    .ok_or_else(|| ErrorData::resource_not_found("no such entry", None))?;
                Ok(DomainRead::no_store(json_read(&request.uri, &entry)?))
            }
        }
    }
}

/// Ledger's handler with its in-memory Tasks and entries.
pub fn hosted(tasks: LedgerTasks) -> Hosted<LedgerDomain, LedgerTasks> {
    Hosted::new(LedgerDomain::with_state(tasks.clone())).with_tasks(tasks)
}

struct StoredTask {
    owner: String,
    task: Task,
    payload: TaskPayload,
}

/// Ledger's state, kept in memory: `tally` Tasks, each of which completes shortly after
/// it starts, and entries.
#[derive(Clone)]
pub struct LedgerTasks {
    tasks: Arc<parking_lot::Mutex<HashMap<String, StoredTask>>>,
    changes: broadcast::Sender<String>,
    delay: Duration,
    entries: Arc<parking_lot::Mutex<HashMap<EntryName, EntryState>>>,
    entry_changes: Arc<veoveo_sdk_rust::SubscriptionHub>,
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
            entries: Arc::default(),
            entry_changes: Arc::default(),
        }
    }

    /// The entry named `name`, when it was opened.
    pub fn entry(&self, name: &EntryName) -> Option<Entry> {
        self.entries.lock().get(name).map(|state| Entry {
            entry: name.clone(),
            state: *state,
        })
    }

    /// Opens `name` and closes it `close_after` later, announcing each change.
    async fn open(&self, name: EntryName, close_after: Duration) -> Entry {
        self.set(&name, EntryState::Open).await;
        let state = self.clone();
        let closing = name.clone();
        tokio::spawn(async move {
            tokio::time::sleep(close_after).await;
            state.set(&closing, EntryState::Closed).await;
        });
        Entry {
            entry: name,
            state: EntryState::Open,
        }
    }

    async fn set(&self, name: &EntryName, state: EntryState) {
        self.entries.lock().insert(name.clone(), state);
        if let Ok(uri) = LedgerAddress::Entry(name.clone()).to_uri() {
            self.entry_changes
                .notify_resource_updated(uri.as_str())
                .await;
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
        let mut accepted = SubscriptionFilter::new();
        accepted.task_ids = requested.task_ids.clone().filter(|ids| !ids.is_empty());
        accepted.resource_subscriptions = requested
            .resource_subscriptions
            .clone()
            .filter(|uris| !uris.is_empty());
        (accepted.task_ids.is_some() || accepted.resource_subscriptions.is_some())
            .then_some(accepted)
    }

    async fn listen(&self, context: SubscriptionContext) -> Result<(), ErrorData> {
        let identity = gateway_identity(context.request_context())?;
        // Each subscribed resource must be an entry the caller may read.
        let entries = veoveo_sdk_rust::hosting::requested_addresses::<LedgerAddress>(
            context.accepted().resource_subscriptions.as_deref(),
        )?;
        if !entries.is_empty() {
            require(context.request_context(), LedgerPermission::EntriesRead)?;
        }
        let mut entry_changes = (!entries.is_empty()).then(|| self.entry_changes.listen());
        if entry_changes.is_some() {
            veoveo_sdk_rust::send_resource_update(
                &context,
                veoveo_sdk_rust::ResourceUpdate::Reconcile,
            )
            .await?;
        }
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
                update = veoveo_sdk_rust::receive_resource_update(&mut entry_changes) => {
                    veoveo_sdk_rust::send_resource_update(&context, update).await?;
                }
            }
        }
    }
}
