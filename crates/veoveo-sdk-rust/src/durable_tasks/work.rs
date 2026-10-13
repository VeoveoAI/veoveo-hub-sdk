//! Task work for a hosted server: a [`TaskWorker`] claims the server's Tasks and runs a
//! handler for each with a [`TaskContext`], the `ctx` of the Task's work.
//!
//! The context runs durable steps (`ctx.step`), records and dispatches operations that
//! hand work to a provider (`ctx.operation(..).dispatch(..)`), waits for their outcomes
//! (`ctx.wait`), calls other servers for the Task's caller (`ctx.client()`) and records
//! the Task's usage (`ctx.usage()`). A Task that waits on an operation parks: the wait
//! fails with [`TaskError::Parked`], the handler propagates it, and the worker releases
//! the Task without settling it. A provider event, a due status check or the deadline
//! queues the Task again, and the handler runs from its start: finished steps and
//! dispatched operations return what they recorded, so it resumes where it parked.
//!
//! A process that stops drains its Tasks without settling them wrongly. Its stop
//! reaches each handler through `ctx.cancelled()` and `ctx.checkpoint()`. A `resume`
//! Task whose process is stopping publishes no outcome and stays with its instance,
//! whose recovery queues it for another worker; a requested cancellation still
//! settles. A handler that stops with an error leaves its Task to its recovery class.
use std::{collections::BTreeMap, fmt, future::Future, sync::Arc, time::Duration};

use futures::StreamExt;
use tokio_util::{
    sync::{CancellationToken, WaitForCancellationFuture},
    task::TaskTracker,
};
use veoveo_audit_contract::AuditRequest;
use veoveo_task_runtime::{
    CallbackToken, Operation, OperationCalls, OperationKey, OperationSpec, RecoveryClass,
    ResumeCancellationPolicy, StepAttempt, StepDispatch, StepKey, StepOperationId, Submission,
    TaskError, TaskFailure, TaskRuntime, TaskSnapshot, TaskStatus, TaskTransition, WaitOutcome,
};
use veoveo_types::{ServerSlug, TaskId, TaskTypeName};

use crate::{
    client::{CallError, GatewayClient},
    hosting::ServerCalls,
    usage::{TaskUsageRecorder, UsageLedger, UsageRecordError},
};

/// Where a hosted server's operation callbacks are reachable: its public base URL. A
/// provider calls `{base}/{slug}/operations/{operation_id}/{token}`, the route the
/// hosted server serves for the operation.
#[derive(Clone)]
pub struct CallbackUrls {
    base: url::Url,
    server: ServerSlug,
}

impl fmt::Debug for CallbackUrls {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CallbackUrls")
            .field("base", &self.base.as_str())
            .field("server", &self.server)
            .finish()
    }
}

/// A public base URL that cannot carry callback paths.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("a callback base must be an http or https URL without a query or fragment")]
pub struct CallbackUrlError;

impl CallbackUrls {
    /// Callbacks under `base` for `server`'s operations.
    pub fn new(base: url::Url, server: ServerSlug) -> Result<Self, CallbackUrlError> {
        let usable = matches!(base.scheme(), "http" | "https")
            && base.host_str().is_some()
            && !base.cannot_be_a_base()
            && base.query().is_none()
            && base.fragment().is_none();
        if !usable {
            return Err(CallbackUrlError);
        }
        Ok(Self { base, server })
    }

    /// The URL a provider calls back for `operation_id` with `token`. The URL library
    /// encodes each path segment.
    pub fn operation(&self, operation_id: StepOperationId, token: &CallbackToken) -> url::Url {
        let mut url = self.base.clone();
        url.path_segments_mut()
            .expect("a callback base is a hierarchical URL")
            .pop_if_empty()
            .push(self.server.as_str())
            .push("operations")
            .push(&operation_id.to_string())
            .push(token.expose());
        url
    }

    /// The server whose operations these callbacks reach.
    pub fn server(&self) -> &ServerSlug {
        &self.server
    }
}

/// Why a Task's work has no client of other servers.
#[derive(Debug, thiserror::Error)]
pub enum TaskClientError {
    #[error("the Task worker was built without the server's calls")]
    NoCalls,
    #[error(transparent)]
    Call(#[from] CallError),
}

/// What a dispatch closure receives before it calls the provider.
#[derive(Clone)]
pub struct OperationDispatch {
    /// The operation's identity, the same on every attempt: the provider call's
    /// idempotency key.
    pub operation_id: StepOperationId,
    /// [`StepDispatch::Unsettled`] when an earlier attempt dispatched the operation and
    /// recorded no submission. That call may have reached the provider, so this attempt
    /// reconciles with the provider and never resubmits blindly.
    pub dispatch: StepDispatch,
    /// The URL the provider calls back, on the first attempt only and when the worker
    /// knows the server's public base. It carries the operation's bearer token.
    pub callback_url: Option<url::Url>,
}

impl fmt::Debug for OperationDispatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OperationDispatch")
            .field("operation_id", &self.operation_id)
            .field("dispatch", &self.dispatch)
            .field(
                "callback_url",
                &self.callback_url.as_ref().map(|_| "redacted"),
            )
            .finish()
    }
}

/// The work context of one claimed Task.
#[derive(Clone)]
pub struct TaskContext {
    inner: Arc<ContextInner>,
}

struct ContextInner {
    runtime: TaskRuntime,
    snapshot: TaskSnapshot,
    calls: Option<ServerCalls>,
    callbacks: Option<CallbackUrls>,
    usage: Option<TaskUsageRecorder>,
    /// How a pending cancellation settles this Task's outcome, when it is `resume`.
    policy: ResumeCancellationPolicy,
    /// Cancelled when the Task's cancellation reaches this worker or the process stops.
    cancellation: CancellationToken,
    /// Cancelled when the process stops.
    stop: CancellationToken,
}

/// Why a Task's work stopped before its outcome.
#[derive(Debug, thiserror::Error)]
pub enum TaskStop {
    /// The Task's cancellation was requested, on this worker or on another replica.
    #[error("the Task's cancellation was requested")]
    Cancelled,
    /// This process is stopping; the Task's instance recovery takes it on.
    #[error("the process running the Task is stopping")]
    Stopping,
    #[error(transparent)]
    Runtime(#[from] TaskError),
}

impl TaskContext {
    fn new(
        worker: &WorkerParts,
        snapshot: TaskSnapshot,
        cancellation: CancellationToken,
        stop: CancellationToken,
    ) -> Self {
        let usage = worker
            .usage
            .as_ref()
            .map(|ledger| ledger.for_task(&snapshot));
        let policy = worker.policy(&snapshot.task_type);
        Self {
            inner: Arc::new(ContextInner {
                runtime: worker.runtime.clone(),
                snapshot,
                calls: worker.calls.clone(),
                callbacks: worker.callbacks.clone(),
                usage,
                policy,
                cancellation,
                stop,
            }),
        }
    }

    /// The Task as the worker claimed it.
    pub fn task(&self) -> &TaskSnapshot {
        &self.inner.snapshot
    }

    pub fn task_id(&self) -> TaskId {
        self.inner.snapshot.task_id
    }

    pub fn runtime(&self) -> &TaskRuntime {
        &self.inner.runtime
    }

    /// Resolves when the Task's cancellation reaches this worker or the process stops.
    /// A cancellation requested on another replica reaches the handler at its next
    /// [`TaskContext::checkpoint`].
    pub fn cancelled(&self) -> WaitForCancellationFuture<'_> {
        self.inner.cancellation.cancelled()
    }

    /// A token cancelled with [`TaskContext::cancelled`], for work that runs outside the
    /// handler's future, such as a blocking calculation.
    pub fn cancellation_token(&self) -> CancellationToken {
        self.inner.cancellation.clone()
    }

    /// Whether the process running the Task is stopping.
    pub fn stopping(&self) -> bool {
        self.inner.stop.is_cancelled()
    }

    /// Whether the Task's work may go on: it fails with [`TaskStop`] when the process is
    /// stopping, when the Task's cancellation was requested on any replica, or when this
    /// instance no longer owns the Task. The handler propagates the error, and the worker
    /// settles a requested cancellation and leaves a stopping Task to its recovery.
    ///
    /// A Task that dispatched an operation reaches [`TaskContext::wait`] before any
    /// checkpoint, so the wait carries a cancellation to the provider.
    pub async fn checkpoint(&self) -> Result<(), TaskStop> {
        if self.stopping() {
            return Err(TaskStop::Stopping);
        }
        if self.inner.cancellation.is_cancelled() {
            return Err(TaskStop::Cancelled);
        }
        let id = self.task_id();
        let current = self
            .inner
            .runtime
            .get(id)
            .await?
            .ok_or(TaskError::NotFound(id))?;
        if current.is_terminal() || current.status == TaskStatus::CancelRequested {
            return Err(TaskStop::Cancelled);
        }
        self.inner.runtime.ensure_owner(&current)?;
        Ok(())
    }

    /// Record a transition of the Task while its work runs, such as its progress. A
    /// `resume` Task records it through resumable settlement with the Task's
    /// cancellation and its kind's cancellation policy, so a stopping process records
    /// no success.
    pub async fn update(&self, transition: TaskTransition) -> Result<TaskSnapshot, TaskError> {
        record(
            &self.inner.runtime,
            &self.inner.snapshot,
            transition,
            &self.inner.cancellation,
            self.inner.policy,
        )
        .await
    }

    /// Run one durable step of the Task: a finished step returns its stored result
    /// without running `run` again, and a step an earlier run dispatched without a
    /// result passes `run` an unsettled attempt with the same operation ID.
    pub async fn step<T, E, F, Fut>(&self, key: &StepKey, run: F) -> Result<T, E>
    where
        T: serde::Serialize + serde::de::DeserializeOwned,
        E: From<TaskError>,
        F: FnOnce(StepAttempt) -> Fut,
        Fut: Future<Output = Result<T, E>>,
    {
        self.inner.runtime.step(self.task_id(), key, run).await
    }

    /// One operation of the Task, to dispatch or wait for.
    pub fn operation<'a>(&'a self, spec: &'a OperationSpec) -> OperationStep<'a> {
        OperationStep {
            context: self,
            spec,
        }
    }

    /// Wait for the outcome of the Task's operation `key`. A pending operation parks the
    /// Task: the wait fails with [`TaskError::Parked`] in `C::Error`, which the handler
    /// propagates.
    pub async fn wait<T, C>(
        &self,
        key: &OperationKey,
        calls: &C,
    ) -> Result<WaitOutcome<T>, C::Error>
    where
        T: serde::de::DeserializeOwned,
        C: OperationCalls,
    {
        self.inner
            .runtime
            .wait_operation(self.task_id(), key, calls)
            .await
    }

    /// A client of other servers for the caller that created this Task, while it runs.
    pub fn client(&self) -> Result<GatewayClient, TaskClientError> {
        Ok(self
            .inner
            .calls
            .as_ref()
            .ok_or(TaskClientError::NoCalls)?
            .for_task(&self.inner.snapshot)?)
    }

    /// The Task's usage records, for the caller it runs for.
    pub fn usage(&self) -> Result<&TaskUsageRecorder, UsageRecordError> {
        self.inner.usage.as_ref().ok_or(UsageRecordError::NoLedger)
    }

    /// The audit request of work in this Task: a background request that names it.
    pub fn audit_request(&self) -> AuditRequest {
        let mut request = AuditRequest::background();
        request.task = Some(self.task_id());
        request
    }
}

/// One operation of a Task: its dispatch and its wait.
pub struct OperationStep<'a> {
    context: &'a TaskContext,
    spec: &'a OperationSpec,
}

impl OperationStep<'_> {
    /// Record the operation and dispatch it once. `run` calls the provider and reports
    /// its submission; a recorded submission returns the operation without running
    /// `run`, and an error from `run` records nothing, so the next attempt is unsettled.
    pub async fn dispatch<E, F, Fut>(self, run: F) -> Result<Operation, E>
    where
        E: From<TaskError>,
        F: FnOnce(OperationDispatch) -> Fut,
        Fut: Future<Output = Result<Submission, E>>,
    {
        let callbacks = self.context.inner.callbacks.clone();
        self.context
            .inner
            .runtime
            .dispatch_operation(self.context.task_id(), self.spec, |attempt| {
                let callback_url = match (&callbacks, &attempt.callback) {
                    (Some(urls), Some(token)) => Some(urls.operation(attempt.operation_id, token)),
                    _ => None,
                };
                run(OperationDispatch {
                    operation_id: attempt.operation_id,
                    dispatch: attempt.dispatch,
                    callback_url,
                })
            })
            .await
    }

    /// Wait for the operation's outcome, parking the Task while it is pending.
    pub async fn wait<T, C>(self, calls: &C) -> Result<WaitOutcome<T>, C::Error>
    where
        T: serde::de::DeserializeOwned,
        C: OperationCalls,
    {
        self.context.wait(&self.spec.key, calls).await
    }
}

/// How a handler's error reaches the worker: a parked Task is released, a Task this
/// instance no longer owns is left alone, and any other error fails the Task.
pub trait TaskWorkError: fmt::Display + Send + 'static {
    /// The Task parked on an operation.
    fn is_parked(&self) -> bool;
    /// The instance lost the Task to recovery or another worker.
    fn is_lost(&self) -> bool;
}

impl TaskWorkError for TaskError {
    fn is_parked(&self) -> bool {
        TaskError::is_parked(self)
    }

    fn is_lost(&self) -> bool {
        matches!(self, Self::NotOwner(_) | Self::Conflict(_))
    }
}

impl TaskWorkError for TaskStop {
    fn is_parked(&self) -> bool {
        matches!(self, Self::Runtime(error) if error.is_parked())
    }

    fn is_lost(&self) -> bool {
        matches!(self, Self::Runtime(error) if TaskWorkError::is_lost(error))
    }
}

impl TaskWorkError for anyhow::Error {
    fn is_parked(&self) -> bool {
        self.downcast_ref::<TaskError>()
            .is_some_and(TaskError::is_parked)
            || self
                .downcast_ref::<TaskStop>()
                .is_some_and(TaskWorkError::is_parked)
    }

    fn is_lost(&self) -> bool {
        self.downcast_ref::<TaskError>()
            .is_some_and(TaskWorkError::is_lost)
            || self
                .downcast_ref::<TaskStop>()
                .is_some_and(TaskWorkError::is_lost)
    }
}

struct WorkerParts {
    runtime: TaskRuntime,
    calls: Option<ServerCalls>,
    callbacks: Option<CallbackUrls>,
    usage: Option<UsageLedger>,
    policies: BTreeMap<TaskTypeName, ResumeCancellationPolicy>,
}

impl WorkerParts {
    fn policy(&self, task_type: &TaskTypeName) -> ResumeCancellationPolicy {
        self.policies
            .get(task_type)
            .copied()
            .unwrap_or(DEFAULT_CANCELLATION)
    }
}

/// How long a stopping worker waits for its running Tasks by default.
pub const DEFAULT_DRAIN: Duration = Duration::from_secs(30);

/// How a pending cancellation settles a `resume` Task's outcome unless its kind names
/// another policy: the cancellation replaces a success, and a genuine failure stays.
pub const DEFAULT_CANCELLATION: ResumeCancellationPolicy =
    ResumeCancellationPolicy::PreserveFailure;

/// Claims a server's Tasks and runs a handler for each.
pub struct TaskWorker {
    parts: WorkerParts,
    drain: Duration,
}

impl TaskWorker {
    pub fn new(runtime: TaskRuntime) -> Self {
        Self {
            parts: WorkerParts {
                runtime,
                calls: None,
                callbacks: None,
                usage: None,
                policies: BTreeMap::new(),
            },
            drain: DEFAULT_DRAIN,
        }
    }

    /// How a pending cancellation settles the outcome of `resume` Tasks of `task_type`,
    /// [`DEFAULT_CANCELLATION`] for kinds not named. `CancellationWins` turns any
    /// outcome into `Cancelled`; `PreserveFailure` keeps a genuine failure.
    pub fn cancellation(
        mut self,
        task_type: TaskTypeName,
        policy: ResumeCancellationPolicy,
    ) -> Self {
        self.parts.policies.insert(task_type, policy);
        self
    }

    /// How long [`TaskWorker::run`] waits, once stopped, for its running Tasks to settle
    /// or release, [`DEFAULT_DRAIN`] by default. A Task still running after it stays
    /// with its instance, whose recovery takes it on when the process ends.
    pub fn drain(mut self, drain: Duration) -> Self {
        self.drain = drain;
        self
    }

    /// Calls to other servers, for `ctx.client()`.
    pub fn calls(mut self, calls: ServerCalls) -> Self {
        self.parts.calls = Some(calls);
        self
    }

    /// The server's public base, for the callback URLs of its operations.
    pub fn callbacks(mut self, callbacks: CallbackUrls) -> Self {
        self.parts.callbacks = Some(callbacks);
        self
    }

    /// The usage ledger, for `ctx.usage()`.
    pub fn usage(mut self, usage: UsageLedger) -> Self {
        self.parts.usage = Some(usage);
        self
    }

    /// Run `handler` for every Task this instance claims until `stop`. Each Task runs on
    /// its own task, registered for local cancellation. A handler's transition settles
    /// its Task, a `resume` Task through resumable settlement, and a parked Task is
    /// released without settlement. Once `stop` is cancelled the worker claims nothing
    /// more and waits up to its drain time for its running Tasks.
    pub async fn run<H, Fut, E>(self, handler: H, stop: CancellationToken) -> Result<(), TaskError>
    where
        H: Fn(TaskContext) -> Fut + Clone + Send + Sync + 'static,
        Fut: Future<Output = Result<TaskTransition, E>> + Send + 'static,
        E: TaskWorkError,
    {
        let parts = Arc::new(self.parts);
        let running = TaskTracker::new();
        let mut claimed = parts.runtime.work().await?;
        loop {
            let next = tokio::select! {
                () = stop.cancelled() => break,
                next = claimed.next() => next,
            };
            let snapshot = next.ok_or(TaskError::ListenerStopped)??;
            let task_id = snapshot.task_id;
            let cancellation = stop.child_token();
            let context = TaskContext::new(&parts, snapshot, cancellation.clone(), stop.clone());
            let handler = handler.clone();
            let runtime = parts.runtime.clone();
            let join = tokio::spawn(
                running.track_future(async move { settle(&runtime, context, handler).await }),
            );
            parts
                .runtime
                .register_worker(task_id, cancellation, join)
                .await?;
            parts.runtime.reap_workers().await;
        }
        running.close();
        if tokio::time::timeout(self.drain, running.wait())
            .await
            .is_err()
        {
            tracing::warn!(
                running = running.len(),
                "Tasks still ran when the worker's drain ended; their instance's recovery takes them on"
            );
        }
        Ok(())
    }
}

/// Run one claimed Task to its settlement or its release.
async fn settle<H, Fut, E>(runtime: &TaskRuntime, context: TaskContext, handler: H)
where
    H: Fn(TaskContext) -> Fut,
    Fut: Future<Output = Result<TaskTransition, E>>,
    E: TaskWorkError,
{
    let task_id = context.task_id();
    let claimed = context.task().clone();
    let policy = context.inner.policy;
    let cancellation = context.inner.cancellation.clone();
    let stop = context.inner.stop.clone();
    let transition = match handler(context).await {
        Ok(transition) => transition,
        Err(error) if error.is_parked() => {
            tracing::debug!(%task_id, "the Task parked on an operation");
            return;
        }
        Err(error) if error.is_lost() => {
            tracing::debug!(%task_id, %error, "the worker no longer owns the Task");
            return;
        }
        Err(error) => match stopped(runtime, task_id, &stop).await {
            Stopped::Finished => return,
            Stopped::Cancelled => TaskTransition::Cancelled,
            Stopped::Stopping => {
                tracing::debug!(%task_id, %error, "the Task stopped with its process");
                return;
            }
            Stopped::No => {
                tracing::warn!(%task_id, %error, "a Task handler failed");
                TaskTransition::Failed(TaskFailure::new(
                    "task_error",
                    "the Task could not finish its work",
                ))
            }
        },
    };
    if claimed.recovery_class == RecoveryClass::Resume
        && stop.is_cancelled()
        && matches!(
            stopped(runtime, task_id, &stop).await,
            Stopped::Stopping | Stopped::Finished
        )
    {
        // A stopping process publishes no outcome of a resumable Task: its instance's
        // recovery queues it, and another worker runs it to the same outcome.
        tracing::debug!(%task_id, "the Task's outcome waits for its recovery");
        return;
    }
    if let Err(error) = record(runtime, &claimed, transition, &cancellation, policy).await {
        tracing::warn!(%task_id, %error, "a Task did not settle");
    }
}

/// Record `transition` of the claimed Task: a `resume` Task through resumable
/// settlement with its cancellation and `policy`, any other through a plain transition.
async fn record(
    runtime: &TaskRuntime,
    claimed: &TaskSnapshot,
    transition: TaskTransition,
    cancellation: &CancellationToken,
    policy: ResumeCancellationPolicy,
) -> Result<TaskSnapshot, TaskError> {
    match claimed.recovery_class {
        RecoveryClass::Resume => {
            runtime
                .transition_resumable(claimed.task_id, transition, policy, Some(cancellation))
                .await
        }
        RecoveryClass::InterruptedIndeterminate => {
            runtime.transition(claimed.task_id, transition).await
        }
    }
}

/// Why a Task's work ended without an outcome.
enum Stopped {
    /// The Task already finished.
    Finished,
    /// Its cancellation was requested.
    Cancelled,
    /// The process is stopping and nobody asked to cancel the Task.
    Stopping,
    /// Neither: the work failed.
    No,
}

async fn stopped(runtime: &TaskRuntime, task_id: TaskId, stop: &CancellationToken) -> Stopped {
    match runtime.get(task_id).await {
        Ok(None) => Stopped::Finished,
        Ok(Some(current)) if current.is_terminal() => Stopped::Finished,
        Ok(Some(current)) if current.status == TaskStatus::CancelRequested => Stopped::Cancelled,
        _ if stop.is_cancelled() => Stopped::Stopping,
        _ => Stopped::No,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stopped_task_is_parked_or_lost_only_when_its_runtime_says_so() {
        let id = TaskId::new();
        for stop in [TaskStop::Cancelled, TaskStop::Stopping] {
            assert!(!stop.is_parked() && !stop.is_lost());
            let error = anyhow::Error::from(stop);
            assert!(!error.is_parked() && !error.is_lost());
        }
        let lost = anyhow::Error::from(TaskStop::Runtime(TaskError::NotOwner(id)));
        assert!(lost.is_lost() && !lost.is_parked());
        let parked = anyhow::Error::from(TaskStop::Runtime(TaskError::Parked(id)));
        assert!(parked.is_parked() && !parked.is_lost());
    }

    #[test]
    fn callback_urls_place_the_operation_and_its_token_under_the_server() {
        let server = ServerSlug::parse("media").unwrap();
        let token = CallbackToken::parse(&"A".repeat(43)).unwrap();
        let operation = StepOperationId::new();
        for base in [
            "https://media.example",
            "https://media.example/",
            "https://example/edge/",
        ] {
            let urls = CallbackUrls::new(url::Url::parse(base).unwrap(), server.clone()).unwrap();
            let url = urls.operation(operation, &token);
            let path = url.path();
            assert!(
                path.ends_with(&format!("/media/operations/{operation}/{}", "A".repeat(43))),
                "{path}"
            );
            assert!(!path.contains("//"), "{path}");
        }
        for base in [
            "https://media.example/?q=1",
            "ftp://media.example",
            "mailto:a@b",
        ] {
            assert!(CallbackUrls::new(url::Url::parse(base).unwrap(), server.clone()).is_err());
        }
        let urls =
            CallbackUrls::new(url::Url::parse("https://m.example").unwrap(), server).unwrap();
        assert!(!format!("{urls:?}").contains("operations"));
        let dispatch = OperationDispatch {
            operation_id: operation,
            dispatch: StepDispatch::First,
            callback_url: Some(urls.operation(operation, &token)),
        };
        assert!(!format!("{dispatch:?}").contains(&"A".repeat(43)));
    }
}
