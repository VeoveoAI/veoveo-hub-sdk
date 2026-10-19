//! Relay, the consumer of the calls fixture: its tools call Ledger through the Hub
//! for Relay's own caller, inline with `ctx.client()` and from a durable Task with
//! `ServerCalls::for_task`, and read and follow Ledger's entries. It declares the
//! Ledger permissions it uses, and reports the identity calls reach it with.
use std::{collections::BTreeSet, sync::LazyLock};

use futures::StreamExt;
use rmcp::{
    ErrorData, RoleServer,
    model::{
        CallToolRequestParams, CreateTaskResult, GetTaskParams, GetTaskResult, ServerCapabilities,
        ServerConfig, UpdateTaskParams,
    },
    service::RequestContext,
};
use schemars::JsonSchema;
use serde::Deserialize;
use tokio_util::sync::CancellationToken;
use veoveo_hub_sdk::{
    HubInternalIdentity, PrincipalKind, ServerSlug,
    client::CallError,
    durable_tasks::{
        DurableTaskService, DurableTaskSubscription, DurableTasks, cancel_durable_task,
        get_durable_task, mcp_task_completion, subscribe_durable_tasks, task_seed,
        update_durable_task,
    },
    hosting::{ClientContext, DomainServer, Hosted, NoTasks, ServerCalls, hub_identity},
    registration::{AuthorizationDeclaration, DeclarationError},
    server_contract::{McpServerContract, McpServerSetup},
};
use veoveo_task::{
    CreateTask, RecoveryClass, ResumeCancellationPolicy, TaskFailure, TaskOwner, TaskRuntime,
    TaskSnapshot, TaskSurface, TaskTransition,
};
use veoveo_types::{ResourceScheme, TaskId, TaskTypeName, Vocabulary};

use crate::calls::{
    EntryName, EntryState, FollowedEntry, Inspect, OpenEntry, ReadEntry, ReadSeal, RelayPermission,
    Tally,
};
use crate::calls_support::{NoScope, contract_config, docs, require};

/// The Task operation of `relay_later`.
pub const LATER: TaskTypeName = TaskTypeName::from_static("relay.later");

pub struct RelayContract;

static DOCS: LazyLock<veoveo_hub_sdk::docs::ServerDocs> = LazyLock::new(|| docs("relay"));
static SETUP: LazyLock<McpServerSetup<RelayContract>> =
    LazyLock::new(|| McpServerSetup::new().expect("Relay setup"));

impl McpServerContract for RelayContract {
    type Server = RelayDomain;
    type Scope = NoScope;
    type Provider = RelayContract;
    fn scheme() -> ResourceScheme {
        ResourceScheme::parse("relay").unwrap()
    }
    fn scopes() -> &'static [NoScope] {
        NoScope::ALL
    }
    fn documents() -> &'static veoveo_hub_sdk::docs::ServerDocs {
        &DOCS
    }
    fn server_config() -> ServerConfig {
        contract_config(
            "relay",
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .enable_completions()
                .build(),
        )
    }
}

/// Relay's declaration: every tool requires `relay.calls.make`, and Relay uses Ledger's
/// two permissions.
pub fn authorization() -> Result<AuthorizationDeclaration, DeclarationError> {
    let declaration = ROUTES.authorization();
    declaration.check(
        &veoveo_hub_sdk::server_contract::provider_slug::<RelayContract>(),
        &RelayContract::scheme(),
    )?;
    Ok(declaration)
}

/// The tool result of a call to Ledger: its result, or the refusal as an error result.
fn relayed<T>(outcome: Result<T, CallError>) -> Result<T, veoveo_hub_sdk::hosting::ToolError> {
    outcome.map_err(Into::into)
}

#[derive(Debug, serde::Serialize, Deserialize, JsonSchema)]
struct TallyRequest {
    entries: u32,
}

#[derive(Debug, serde::Serialize, Deserialize, JsonSchema)]
struct FollowRequest {
    entry: EntryName,
}

/// How long Relay follows an entry before it gives up.
const FOLLOW_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);
/// How long an entry Relay opens stays open.
const ENTRY_OPEN_MS: u64 = 1_500;

#[derive(Clone)]
pub struct RelayDomain;

impl Default for RelayDomain {
    fn default() -> Self {
        Self::new()
    }
}

impl RelayDomain {
    pub fn new() -> Self {
        Self
    }

    async fn relay_inspect(
        &self,
        context: RequestContext<RoleServer>,
    ) -> Result<crate::calls::Inspection, veoveo_hub_sdk::hosting::ToolError> {
        require(&context, RelayPermission::CallsMake)?;
        relayed(context.client()?.call(Inspect {}).await)
    }

    async fn relay_seal(
        &self,
        context: RequestContext<RoleServer>,
    ) -> Result<crate::calls::Seal, veoveo_hub_sdk::hosting::ToolError> {
        require(&context, RelayPermission::CallsMake)?;
        relayed(context.client()?.call(ReadSeal {}).await)
    }

    async fn relay_tally(
        &self,
        request: TallyRequest,
        context: RequestContext<RoleServer>,
    ) -> Result<crate::calls::Tallied, veoveo_hub_sdk::hosting::ToolError> {
        require(&context, RelayPermission::CallsMake)?;
        let client = context.client()?;
        let outcome = async {
            let handle = client
                .start(Tally {
                    entries: request.entries,
                })
                .await?;
            handle.await
        }
        .await;
        relayed(outcome)
    }

    async fn relay_whoami(
        &self,
        context: RequestContext<RoleServer>,
    ) -> Result<crate::calls::Inspection, veoveo_hub_sdk::hosting::ToolError> {
        let identity = require(&context, RelayPermission::CallsMake)?;
        Ok(crate::calls_support::inspection(&identity))
    }

    async fn relay_follow_entry(
        &self,
        request: FollowRequest,
        context: RequestContext<RoleServer>,
    ) -> Result<FollowedEntry, veoveo_hub_sdk::hosting::ToolError> {
        require(&context, RelayPermission::CallsMake)?;
        let client = context.client()?;
        let entry = request.entry;
        let outcome = async {
            // The entry stays open long enough for the subscription to see it open.
            client
                .call(OpenEntry {
                    entry: entry.clone(),
                    close_after_ms: ENTRY_OPEN_MS,
                })
                .await?;
            let mut updates = client
                .read(ReadEntry {
                    entry: entry.clone(),
                })
                .updates();
            let mut states = Vec::new();
            let follow = async {
                while let Some(content) = updates.next().await {
                    let content = content?;
                    states.push(content.state);
                    if content.state == EntryState::Closed {
                        return Ok(());
                    }
                }
                Err(CallError::Transport("the entry's updates ended".into()))
            };
            tokio::time::timeout(FOLLOW_TIMEOUT, follow)
                .await
                .unwrap_or(Err(CallError::Timeout(FOLLOW_TIMEOUT)))?;
            let waited = client
                .read(ReadEntry {
                    entry: entry.clone(),
                })
                .until(|content| content.state == EntryState::Closed)
                .timeout(FOLLOW_TIMEOUT)
                .await?;
            let read = client.read(ReadEntry { entry }).await?;
            Ok::<_, CallError>(FollowedEntry {
                states,
                waited,
                read,
            })
        }
        .await;
        relayed(outcome)
    }

    async fn relay_later(
        &self,
        context: RequestContext<RoleServer>,
    ) -> Result<crate::calls::Inspection, veoveo_hub_sdk::hosting::ToolError> {
        require(&context, RelayPermission::CallsMake)?;
        Err(ErrorData::invalid_request(
            "relay_later runs as a Task; call it with Task support",
            None,
        )
        .into())
    }
}

#[derive(serde::Serialize, Deserialize, JsonSchema)]
#[veoveo_hub_sdk::tool(provider=RelayContract,output=crate::calls::Inspection,name="relay_inspect")]
struct InspectRelay {}
#[derive(serde::Serialize, Deserialize, JsonSchema)]
#[veoveo_hub_sdk::tool(provider=RelayContract,output=crate::calls::Seal,name="relay_seal")]
struct SealRelay {}
#[derive(serde::Serialize, Deserialize, JsonSchema)]
#[veoveo_hub_sdk::tool(provider=RelayContract,output=crate::calls::Inspection,name="relay_whoami")]
struct WhoamiRelay {}
#[derive(serde::Serialize, Deserialize, JsonSchema)]
#[veoveo_hub_sdk::tool(provider=RelayContract,output=crate::calls::Inspection,name="relay_later",task=true)]
struct LaterRelay {}
impl veoveo_hub_sdk::calls::ProviderContract for RelayContract {
    fn slug() -> ServerSlug {
        ServerSlug::parse("relay").unwrap()
    }
}
impl veoveo_hub_sdk::calls::ToolCall for TallyRequest {
    type Provider = RelayContract;
    type Output = crate::calls::Tallied;
    const TOOL: &'static str = "relay_tally";
}
impl veoveo_hub_sdk::calls::ToolCall for FollowRequest {
    type Provider = RelayContract;
    type Output = FollowedEntry;
    const TOOL: &'static str = "relay_follow_entry";
}
static ROUTES: LazyLock<veoveo_hub_sdk::hosting::Routes<RelayDomain>> = LazyLock::new(|| {
    use veoveo_hub_sdk::hosting::{RoutePolicy, Routes};
    let policy =
        || RoutePolicy::permission(RelayPermission::CallsMake).expect("fixture permission");
    Routes::<RelayDomain>::builder(RoutePolicy::Authenticated)
        .tool::<InspectRelay,_,_>("Relay identity",policy(),|domain,_,context|async move{domain.relay_inspect(context).await})
        .tool::<SealRelay,_,_>("Relay seal",policy(),|domain,_,context|async move{domain.relay_seal(context).await})
        .tool::<WhoamiRelay,_,_>("Inspect Relay identity",policy(),|domain,_,context|async move{domain.relay_whoami(context).await})
        .task::<LaterRelay,_,_>("Relay from a Task",policy(),|domain,_,context|async move{domain.relay_later(context).await})
        .tool::<TallyRequest,_,_>("Relay tally",policy(),|domain,request,context|async move{domain.relay_tally(request,context).await})
        .tool::<FollowRequest,_,_>("Follow entry",policy(),|domain,request,context|async move{domain.relay_follow_entry(request,context).await})
        .uses(veoveo_types::PermissionName::parse("ledger.entries.read").unwrap())
        .uses(veoveo_types::PermissionName::parse("ledger.entries.write").unwrap())
        .uses(veoveo_types::PermissionName::parse("ledger.seals.read").unwrap())
        .build()
});
impl DomainServer for RelayDomain {
    type Contract = RelayContract;
    fn setup() -> &'static McpServerSetup<RelayContract> {
        &SETUP
    }
    fn routes() -> &'static veoveo_hub_sdk::hosting::Routes<Self> {
        &ROUTES
    }
}

/// Relay without Tasks: its inline tools only.
pub fn hosted() -> Hosted<RelayDomain, NoTasks> {
    Hosted::new(RelayDomain::new())
}

/// Relay with `relay_later` as a durable Task.
pub fn hosted_with_tasks(tasks: RelayTasks) -> Hosted<RelayDomain, DurableTasks<RelayTasks>> {
    Hosted::new(RelayDomain::new()).with_tasks(DurableTasks::tasks_only(tasks))
}

/// `relay_later` Tasks in core's Task runtime, and the calls their worker makes.
#[derive(Clone)]
pub struct RelayTasks {
    runtime: TaskRuntime,
    calls: ServerCalls,
}

impl RelayTasks {
    pub fn new(runtime: TaskRuntime, calls: ServerCalls) -> Self {
        Self { runtime, calls }
    }

    fn owner(identity: &HubInternalIdentity) -> Result<TaskOwner, ErrorData> {
        Ok(TaskOwner {
            principal_key: identity.actor.id.clone(),
            principal_kind: match identity.actor.kind {
                PrincipalKind::User => veoveo_task::PrincipalKind::User,
                PrincipalKind::Service => veoveo_task::PrincipalKind::Service,
            },
            issuer: identity.actor.issuer.clone(),
            subject: identity.actor.subject.clone(),
            surface: TaskSurface::parse(identity.profile.as_str())
                .map_err(|_| ErrorData::invalid_request("the profile is no Task surface", None))?,
            tenant_key: identity.actor.tenant.clone(),
            data_labels: identity.actor.data_labels.clone(),
            authority: identity.authority.clone(),
        })
    }

    /// Run each claimed `relay_later` Task until `stop`: it calls Ledger's `inspect` for
    /// the caller that created it and completes with Ledger's answer.
    pub async fn work(self, stop: CancellationToken) -> anyhow::Result<()> {
        let mut claimed = self.runtime.work().await?;
        loop {
            let next = tokio::select! {
                () = stop.cancelled() => return Ok(()),
                next = claimed.next() => next,
            };
            let snapshot = next.ok_or_else(|| anyhow::anyhow!("the Task claim stream ended"))??;
            let tasks = self.clone();
            tokio::spawn(async move {
                if let Err(error) = tasks.run(snapshot).await {
                    tracing::warn!(%error, "a relay_later Task did not settle");
                }
            });
        }
    }

    async fn run(&self, snapshot: TaskSnapshot) -> anyhow::Result<()> {
        let outcome = match self.calls.for_task(&snapshot) {
            Ok(client) => client.call(Inspect {}).await,
            Err(error) => Err(error),
        };
        let transition = match outcome {
            Ok(inspection) => mcp_task_completion(
                "relayed",
                veoveo_hub_sdk::hosting::task_result::<LaterRelay>(&inspection)
                    .map_err(|error| anyhow::anyhow!(error.message.to_string()))?,
            )?,
            Err(error) => {
                TaskTransition::Failed(TaskFailure::new("relay_failed", error.to_string()))
            }
        };
        self.runtime
            .transition_resumable(
                snapshot.task_id,
                transition,
                ResumeCancellationPolicy::PreserveFailure,
                None,
            )
            .await?;
        Ok(())
    }
}

impl DurableTaskService for RelayTasks {
    type Caller = HubInternalIdentity;

    fn authenticate(
        &self,
        context: &RequestContext<RoleServer>,
    ) -> Result<Self::Caller, ErrorData> {
        hub_identity(context)
    }

    async fn start_tool_task(
        &self,
        caller: &Self::Caller,
        request: CallToolRequestParams,
    ) -> Result<Option<CreateTaskResult>, ErrorData> {
        if request.name != "relay_later" {
            return Ok(None);
        }
        let permission =
            veoveo_types::PermissionName::parse(RelayPermission::CallsMake.as_str())
                .map_err(|_| ErrorData::internal_error("invalid permission spelling", None))?;
        if !caller.grants(&permission) {
            return Err(ErrorData::invalid_request(
                format!("the call does not carry {permission}"),
                None,
            ));
        }
        let created = self
            .runtime
            .create(CreateTask {
                task_id: TaskId::new(),
                owner: Self::owner(caller)?,
                server: self.runtime.server().clone(),
                task_type: LATER,
                request: serde_json::json!({}),
                recovery_class: RecoveryClass::Resume,
                idempotency_key: None,
                ttl_ms: Some(60 * 60 * 1_000),
                poll_interval_ms: Some(500),
                retention_pins: BTreeSet::new(),
            })
            .await
            .map_err(|error| ErrorData::internal_error(error.to_string(), None))?;
        Ok(Some(CreateTaskResult::new(task_seed(&created.snapshot))))
    }

    async fn get_task(
        &self,
        caller: &Self::Caller,
        request: GetTaskParams,
    ) -> Result<GetTaskResult, ErrorData> {
        get_durable_task(
            &self.runtime,
            &self.runtime.for_owner(&Self::owner(caller)?),
            request,
        )
        .await
    }

    async fn update_task(
        &self,
        caller: &Self::Caller,
        request: UpdateTaskParams,
    ) -> Result<(), ErrorData> {
        update_durable_task(&self.runtime.for_owner(&Self::owner(caller)?), request).await
    }

    async fn cancel_task(&self, caller: &Self::Caller, task_id: String) -> Result<(), ErrorData> {
        cancel_durable_task(&self.runtime.for_owner(&Self::owner(caller)?), task_id).await
    }

    async fn subscribe_tasks(
        &self,
        caller: &Self::Caller,
        task_ids: Vec<String>,
    ) -> Result<DurableTaskSubscription, ErrorData> {
        subscribe_durable_tasks(
            &self.runtime,
            &self.runtime.for_owner(&Self::owner(caller)?),
            task_ids,
        )
        .await
    }
}
