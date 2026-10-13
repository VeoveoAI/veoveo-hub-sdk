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
use std::{fmt, future::Future, sync::Arc};

use futures::StreamExt;
use tokio_util::sync::{CancellationToken, WaitForCancellationFuture};
use veoveo_audit_contract::AuditRequest;
use veoveo_task_runtime::{
    CallbackToken, Operation, OperationCalls, OperationKey, OperationSpec, RecoveryClass,
    ResumeCancellationPolicy, StepAttempt, StepDispatch, StepKey, StepOperationId, Submission,
    TaskError, TaskFailure, TaskRuntime, TaskSnapshot, TaskTransition, WaitOutcome,
};
use veoveo_types::{ServerSlug, TaskId};

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
    cancellation: CancellationToken,
}

impl TaskContext {
    fn new(worker: &WorkerParts, snapshot: TaskSnapshot, cancellation: CancellationToken) -> Self {
        let usage = worker
            .usage
            .as_ref()
            .map(|ledger| ledger.for_task(&snapshot));
        Self {
            inner: Arc::new(ContextInner {
                runtime: worker.runtime.clone(),
                snapshot,
                calls: worker.calls.clone(),
                callbacks: worker.callbacks.clone(),
                usage,
                cancellation,
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

    /// Resolves when the Task's cancellation reaches this worker.
    pub fn cancelled(&self) -> WaitForCancellationFuture<'_> {
        self.inner.cancellation.cancelled()
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

impl TaskWorkError for anyhow::Error {
    fn is_parked(&self) -> bool {
        self.downcast_ref::<TaskError>()
            .is_some_and(TaskError::is_parked)
    }

    fn is_lost(&self) -> bool {
        self.downcast_ref::<TaskError>()
            .is_some_and(TaskWorkError::is_lost)
    }
}

struct WorkerParts {
    runtime: TaskRuntime,
    calls: Option<ServerCalls>,
    callbacks: Option<CallbackUrls>,
    usage: Option<UsageLedger>,
}

/// Claims a server's Tasks and runs a handler for each.
pub struct TaskWorker {
    parts: WorkerParts,
}

impl TaskWorker {
    pub fn new(runtime: TaskRuntime) -> Self {
        Self {
            parts: WorkerParts {
                runtime,
                calls: None,
                callbacks: None,
                usage: None,
            },
        }
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
    /// released without settlement.
    pub async fn run<H, Fut, E>(self, handler: H, stop: CancellationToken) -> Result<(), TaskError>
    where
        H: Fn(TaskContext) -> Fut + Clone + Send + Sync + 'static,
        Fut: Future<Output = Result<TaskTransition, E>> + Send + 'static,
        E: TaskWorkError,
    {
        let parts = Arc::new(self.parts);
        let mut claimed = parts.runtime.work().await?;
        loop {
            let next = tokio::select! {
                () = stop.cancelled() => return Ok(()),
                next = claimed.next() => next,
            };
            let snapshot = next.ok_or(TaskError::ListenerStopped)??;
            let task_id = snapshot.task_id;
            let cancellation = stop.child_token();
            let context = TaskContext::new(&parts, snapshot, cancellation.clone());
            let handler = handler.clone();
            let runtime = parts.runtime.clone();
            let join = tokio::spawn(async move { settle(&runtime, context, handler).await });
            parts
                .runtime
                .register_worker(task_id, cancellation, join)
                .await?;
            parts.runtime.reap_workers().await;
        }
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
    let recovery = context.task().recovery_class;
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
        Err(error) => {
            tracing::warn!(%task_id, %error, "a Task handler failed");
            TaskTransition::Failed(TaskFailure::new(
                "task_error",
                "the Task could not finish its work",
            ))
        }
    };
    let settled = match recovery {
        RecoveryClass::Resume => {
            runtime
                .transition_resumable(
                    task_id,
                    transition,
                    ResumeCancellationPolicy::PreserveFailure,
                    None,
                )
                .await
        }
        RecoveryClass::InterruptedIndeterminate => runtime.transition(task_id, transition).await,
    };
    if let Err(error) = settled {
        tracing::warn!(%task_id, %error, "a Task did not settle");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
