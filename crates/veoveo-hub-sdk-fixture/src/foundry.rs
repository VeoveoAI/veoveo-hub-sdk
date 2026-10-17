//! Foundry, the operations fixture: `foundry_cast` runs as a durable Task that records
//! an estimate of the cast's cost, hands the cast to Kiln, a fake provider, as an
//! operation with a callback URL, and parks. Kiln's signed callback settles the
//! operation, and the resumed Task records the billed cost and completes with the piece.
//! A cancelled cast carries the cancellation to Kiln and keeps its receipt.
use std::{
    collections::BTreeSet,
    sync::{
        Arc, LazyLock,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use anyhow::{Context, anyhow};
use rmcp::{
    ErrorData, RoleServer,
    handler::server::router::tool::ToolRouter,
    model::{
        CallToolRequestParams, CallToolResult, CreateTaskResult, GetTaskParams, GetTaskResult,
        ReadResourceRequestParams, ServerCapabilities, ServerConfig, UpdateTaskParams,
    },
    service::RequestContext,
    tool, tool_router,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use veoveo_hub_sdk::{
    HubInternalIdentity, PrincipalKind, ServerSlug,
    durable_tasks::{
        DurableTaskService, DurableTaskSubscription, DurableTasks, TaskContext, TaskWorker,
        cancel_durable_task, get_durable_task, mcp_task_completion, subscribe_durable_tasks,
        task_seed, update_durable_task,
    },
    hosting::{
        DomainAddress, DomainRead, DomainServer, Hosted, hub_identity,
        operations::{ProviderEvent, ProviderEvents, ProviderRejection, ProviderRequest},
        structured_result,
    },
    registration::{
        Access, AuthorizationBuilder, AuthorizationDeclaration, DeclarationError,
        PermissionDefinition, Risk,
    },
    server_contract::{McpServerContract, McpServerSetup},
    usage::{
        Amount, ChargePeriod, Currency, Money, Quantity, ServiceCategory, UsageKind,
        UsageKindDeclaration, UsageKinds, UsageUnit,
    },
};
use veoveo_task::{
    CheckObservation, CreateTask, ExternalJobId, Operation, OperationCalls, OperationEventId,
    OperationKey, OperationSpec, ProviderName, RecoveryClass, StepDispatch, StepOperationId,
    Submission, TaskError, TaskFailure, TaskOwner, TaskRuntime, TaskSurface, TaskTransition,
    WaitOutcome,
};
use veoveo_types::{ResourceScheme, TaskId, TaskTypeName, Vocabulary};

use crate::calls_support::{CallsResource, NoScope, contract_config, docs, require};
use crate::kiln::{EVENT_HEADER, Job, KilnEvent, SIGNATURE_HEADER, Submit, verify};

/// The Task operation of `foundry_cast`.
pub const CAST: TaskTypeName = TaskTypeName::from_static("foundry.cast");
/// What a cast is estimated to cost before Kiln reports it.
pub const ESTIMATE_USD: &str = "0.04";
/// How long a cast waits for Kiln.
pub const CAST_TIMEOUT: Duration = Duration::from_secs(300);

/// Foundry's permissions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, veoveo_types::Vocabulary)]
pub enum FoundryPermission {
    #[vocabulary(rename = "foundry.casts.make")]
    CastsMake,
}

impl PermissionDefinition for FoundryPermission {
    fn resource(self) -> &'static str {
        "foundry.cast"
    }
    fn access(self) -> Access {
        Access::Write
    }
    fn risks(self) -> &'static [Risk] {
        &[Risk::Billed]
    }
}

/// Foundry's usage: one record per cast, in US dollars.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FoundryUsage {
    Casts,
}

impl UsageKinds for FoundryUsage {
    fn all() -> &'static [Self] {
        &[Self::Casts]
    }
    fn declaration(self) -> UsageKindDeclaration {
        match self {
            Self::Casts => UsageKindDeclaration {
                kind: UsageKind::parse("foundry.casts").expect("the fixture kind is valid"),
                service_category: ServiceCategory::Other,
                unit: UsageUnit::units(),
                monetary: true,
            },
        }
    }
}

pub struct FoundryContract;

static DOCS: LazyLock<veoveo_hub_sdk::docs::ServerDocs> = LazyLock::new(|| {
    docs(
        "foundry",
        include_str!("../testdata/foundry-compliance.json"),
    )
});
static SETUP: LazyLock<McpServerSetup<FoundryContract>> =
    LazyLock::new(|| McpServerSetup::new().expect("Foundry setup"));

impl McpServerContract for FoundryContract {
    type Scope = NoScope;
    type Resource = CallsResource<FoundryContract>;
    fn slug() -> ServerSlug {
        ServerSlug::parse("foundry").unwrap()
    }
    fn scheme() -> ResourceScheme {
        ResourceScheme::parse("foundry").unwrap()
    }
    fn scopes() -> &'static [NoScope] {
        NoScope::ALL
    }
    fn documents() -> &'static veoveo_hub_sdk::docs::ServerDocs {
        &DOCS
    }
    fn server_config() -> ServerConfig {
        contract_config(
            "foundry",
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .enable_completions()
                .build(),
        )
    }
    fn resources() -> Result<
        Vec<veoveo_hub_sdk::server_contract::McpResource<Self::Resource>>,
        veoveo_hub_sdk::server_contract::McpSetupError,
    > {
        CallsResource::declared()
    }
    fn resource_templates() -> Result<
        Vec<veoveo_hub_sdk::server_contract::McpResourceTemplate>,
        veoveo_hub_sdk::server_contract::McpSetupError,
    > {
        CallsResource::<Self>::templates()
    }
}

/// Foundry's declaration: `foundry_cast` requires `foundry.casts.make`.
pub fn authorization() -> Result<AuthorizationDeclaration, DeclarationError> {
    let tool = |name: &str| veoveo_hub_sdk::LocalToolName::parse(name).unwrap();
    AuthorizationBuilder::<FoundryPermission>::new()
        .tool(tool("foundry_cast"), FoundryPermission::CastsMake)
        .build(&FoundryContract::slug(), &FoundryContract::scheme())
}

/// The arguments of `foundry_cast`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CastRequest {
    pub mold: String,
}

/// A finished cast.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CastResult {
    pub piece: String,
    pub billed_usd: String,
}

#[derive(Clone)]
pub struct FoundryDomain {
    tool_router: ToolRouter<Self>,
}

impl Default for FoundryDomain {
    fn default() -> Self {
        Self::new()
    }
}

#[tool_router]
impl FoundryDomain {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }

    #[tool(description = "Cast a piece in Kiln; runs as a Task.")]
    async fn foundry_cast(
        &self,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, ErrorData> {
        require(&context, FoundryPermission::CastsMake)?;
        Err(ErrorData::invalid_request(
            "foundry_cast runs as a Task; call it with Task support",
            None,
        ))
    }
}

impl DomainServer for FoundryDomain {
    type Contract = FoundryContract;
    fn setup() -> &'static McpServerSetup<FoundryContract> {
        &SETUP
    }
    fn tool_router(&self) -> &ToolRouter<Self> {
        &self.tool_router
    }
    async fn read(
        &self,
        _address: DomainAddress<FoundryContract>,
        _request: &ReadResourceRequestParams,
        _context: &RequestContext<RoleServer>,
    ) -> Result<DomainRead, ErrorData> {
        Err(veoveo_hub_sdk::hosting::served_by_host())
    }
}

/// Foundry with `foundry_cast` as a durable Task.
pub fn hosted(tasks: FoundryTasks) -> Hosted<FoundryDomain, DurableTasks<FoundryTasks>> {
    Hosted::new(FoundryDomain::new()).with_tasks(DurableTasks::tasks_only(tasks))
}

/// `foundry_cast` Tasks in core's Task runtime.
#[derive(Clone)]
pub struct FoundryTasks {
    runtime: TaskRuntime,
}

impl FoundryTasks {
    pub fn new(runtime: TaskRuntime) -> Self {
        Self { runtime }
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
}

impl DurableTaskService for FoundryTasks {
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
        if request.name != "foundry_cast" {
            return Ok(None);
        }
        let permission = veoveo_types::PermissionName::parse(FoundryPermission::CastsMake.as_str())
            .map_err(|_| ErrorData::internal_error("invalid permission spelling", None))?;
        if !caller.grants(&permission) {
            return Err(ErrorData::invalid_request(
                format!("the call does not carry {permission}"),
                None,
            ));
        }
        let arguments = serde_json::Value::Object(request.arguments.unwrap_or_default());
        let cast: CastRequest = serde_json::from_value(arguments)
            .map_err(|error| ErrorData::invalid_params(error.to_string(), None))?;
        let created = self
            .runtime
            .create(CreateTask {
                task_id: TaskId::new(),
                owner: Self::owner(caller)?,
                server: self.runtime.server().clone(),
                task_type: CAST,
                request: serde_json::to_value(cast)
                    .map_err(|error| ErrorData::internal_error(error.to_string(), None))?,
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

/// Foundry's side of Kiln: the calls it makes and its reading of Kiln's signed events.
#[derive(Clone)]
pub struct Furnace {
    kiln: url::Url,
    secret: Arc<[u8]>,
    http: reqwest::Client,
    crash_after_submit: Arc<AtomicBool>,
}

impl Furnace {
    pub fn new(kiln: url::Url, secret: &[u8]) -> Self {
        Self {
            kiln,
            secret: secret.into(),
            http: reqwest::Client::new(),
            crash_after_submit: Arc::default(),
        }
    }

    /// The next dispatch ends as if the process died once Kiln accepted the job: the
    /// job exists, and its submission is not recorded.
    pub fn crash_after_next_submit(&self) {
        self.crash_after_submit.store(true, Ordering::SeqCst);
    }

    fn url(&self, segments: &[&str]) -> url::Url {
        let mut url = self.kiln.clone();
        url.path_segments_mut()
            .expect("Kiln's base is a hierarchical URL")
            .pop_if_empty()
            .extend(segments);
        url
    }

    async fn submit(
        &self,
        operation: StepOperationId,
        callback: Option<url::Url>,
        mold: &str,
    ) -> anyhow::Result<String> {
        let body = serde_json::to_vec(&Submit {
            operation: operation.to_string(),
            callback: callback.map(String::from),
            mold: mold.to_owned(),
        })?;
        let response = self
            .http
            .post(self.url(&["jobs"]))
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(body)
            .send()
            .await?
            .error_for_status()?;
        Ok(serde_json::from_slice::<Job>(&response.bytes().await?)?.job)
    }

    /// The job Kiln holds for `operation`, if it accepted one.
    async fn find(&self, operation: StepOperationId) -> anyhow::Result<Option<String>> {
        let mut url = self.url(&["jobs"]);
        url.query_pairs_mut()
            .append_pair("operation", &operation.to_string());
        let response = self.http.get(url).send().await?;
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }
        let response = response.error_for_status()?;
        Ok(Some(
            serde_json::from_slice::<Job>(&response.bytes().await?)?.job,
        ))
    }
}

impl ProviderEvents for Furnace {
    async fn parse(
        &self,
        request: ProviderRequest<'_>,
    ) -> Result<ProviderEvent, ProviderRejection> {
        let signature = request
            .headers
            .get(SIGNATURE_HEADER)
            .and_then(|value| value.to_str().ok())
            .ok_or(ProviderRejection::Unauthenticated)?;
        if !verify(&self.secret, request.body, signature) {
            return Err(ProviderRejection::Unauthenticated);
        }
        let event_id = request
            .headers
            .get(EVENT_HEADER)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| OperationEventId::parse(value).ok())
            .ok_or(ProviderRejection::Malformed)?;
        let event: KilnEvent =
            serde_json::from_slice(request.body).map_err(|_| ProviderRejection::Malformed)?;
        Ok(ProviderEvent {
            job: ExternalJobId::parse(event.job.as_str()).ok(),
            event_id,
            is_final: event.is_final(),
            payload: serde_json::to_value(&event).map_err(|_| ProviderRejection::Malformed)?,
        })
    }
}

impl OperationCalls for Furnace {
    type Error = anyhow::Error;

    async fn check(&self, _operation: &Operation) -> Result<CheckObservation, Self::Error> {
        Err(anyhow!("Kiln reports its casts through events"))
    }

    async fn cancel(&self, operation: &Operation) -> Result<serde_json::Value, Self::Error> {
        let job = operation
            .external_id
            .as_ref()
            .context("a cancelled cast names its Kiln job")?;
        let response = self
            .http
            .post(self.url(&["jobs", job.as_str(), "cancel"]))
            .send()
            .await?
            .error_for_status()?;
        Ok(serde_json::from_slice(&response.bytes().await?)?)
    }
}

/// The operation of a cast.
pub fn cast_operation() -> OperationSpec {
    OperationSpec::events(
        OperationKey::parse("cast").expect("a valid operation key"),
        ProviderName::parse("kiln").expect("a valid provider name"),
        CAST_TIMEOUT,
    )
}

fn usd(amount: &str) -> anyhow::Result<Money> {
    Ok(Money {
        amount: Amount::parse(amount)?,
        currency: Currency::parse("USD")?,
    })
}

/// One cast Task's work, from its start or from where it parked.
pub async fn cast(ctx: TaskContext, furnace: Furnace) -> anyhow::Result<TaskTransition> {
    let request: CastRequest = serde_json::from_value(ctx.task().request.clone())?;
    let spec = cast_operation();
    ctx.usage()?
        .estimate(FoundryUsage::Casts, Quantity::whole(1))
        .money(usd(ESTIMATE_USD)?)
        .operation(&spec.key)
        .await?;
    let task_id = ctx.task_id();
    ctx.operation(&spec)
        .dispatch(|dispatch| {
            let furnace = furnace.clone();
            let mold = request.mold.clone();
            async move {
                let job = match dispatch.dispatch {
                    StepDispatch::First => {
                        furnace
                            .submit(dispatch.operation_id, dispatch.callback_url, &mold)
                            .await?
                    }
                    // The earlier attempt may have reached Kiln: find its job by the
                    // operation's identity before submitting under that identity again.
                    StepDispatch::Unsettled { .. } => {
                        match furnace.find(dispatch.operation_id).await? {
                            Some(job) => job,
                            None => furnace.submit(dispatch.operation_id, None, &mold).await?,
                        }
                    }
                };
                if furnace.crash_after_submit.swap(false, Ordering::SeqCst) {
                    return Err(anyhow::Error::from(TaskError::NotOwner(task_id)));
                }
                Ok(Submission::job(ExternalJobId::parse(job)?))
            }
        })
        .await?;
    match ctx.wait::<KilnEvent, _>(&spec.key, &furnace).await? {
        WaitOutcome::Settled(event) if event.state == "done" => {
            let billed = event
                .billed_usd
                .clone()
                .unwrap_or_else(|| ESTIMATE_USD.into());
            let mut actual = ctx
                .usage()?
                .actual(FoundryUsage::Casts, Quantity::whole(1))
                .money(usd(&billed)?)
                .operation(&spec.key);
            if let Some(finished) = event
                .finished_at
                .as_deref()
                .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
            {
                actual = actual.period(ChargePeriod::at(finished.to_utc()));
            }
            actual.await?;
            let result = CastResult {
                piece: event.piece.unwrap_or_default(),
                billed_usd: billed,
            };
            let result = structured_result("cast", &result)
                .map_err(|error| anyhow!(error.message.to_string()))?;
            Ok(mcp_task_completion("cast", result)?)
        }
        WaitOutcome::Settled(_) => Ok(TaskTransition::Failed(TaskFailure::new(
            "cast_cracked",
            "the cast cracked in the kiln",
        ))),
        WaitOutcome::Expired => Ok(TaskTransition::Failed(TaskFailure::new(
            "cast_unresolved",
            "Kiln did not report the cast before its deadline",
        ))),
        WaitOutcome::Cancelled { .. } => Ok(TaskTransition::Cancelled),
    }
}

/// Run Foundry's casts on `worker` until `stop`.
pub async fn work(
    worker: TaskWorker,
    furnace: Furnace,
    stop: CancellationToken,
) -> Result<(), TaskError> {
    worker
        .run(
            move |ctx| {
                let furnace = furnace.clone();
                async move { cast(ctx, furnace).await }
            },
            stop,
        )
        .await
}
