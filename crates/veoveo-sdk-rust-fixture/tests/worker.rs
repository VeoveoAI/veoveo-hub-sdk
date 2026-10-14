//! How a `TaskWorker` drains when its process stops, on a private PostgreSQL database:
//! a stopping process publishes no outcome of a resumable Task and leaves it to its
//! instance's recovery, a Task of another recovery class keeps that class's behavior, and
//! a cancellation requested on another replica reaches the handler at its checkpoint.
//! A worker whose database sessions end keeps working, work a lost connection
//! interrupted goes to its recovery instead of failing, and a worker ends once its
//! login is removed.
use std::{
    collections::BTreeSet,
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use serde_json::json;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use veoveo_platform_store::StoreError;
use veoveo_sdk_rust::durable_tasks::{TaskContext, TaskStop, TaskWorker};
use veoveo_task_runtime::{
    CreateTask, Instance, PrincipalKind, RecoveryClass, ResumeCancellationPolicy, TaskError,
    TaskFailure, TaskOwner, TaskRuntime, TaskSnapshot, TaskStatus, TaskTransition,
};
use veoveo_test_fixtures::store::TestDb;
use veoveo_types::{
    AccessSubject, InvocationAuthority, InvocationProvenance, PolicyVersion, PrincipalId,
    ServerSlug, TaskId, TaskTypeName, TenantId, WorkContextId, WorkContextMembershipLevel,
    WorkContextOutputPolicy,
};

const STRIKE: TaskTypeName = TaskTypeName::from_static("anvil.strike");
const TEMPER: TaskTypeName = TaskTypeName::from_static("anvil.temper");
const DEADLINE: Duration = Duration::from_secs(30);

fn slug() -> ServerSlug {
    ServerSlug::parse("anvil").unwrap()
}

async fn within<T>(case: impl Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(90), case)
        .await
        .expect("test case exceeded 90 seconds")
}

fn owner() -> TaskOwner {
    let principal = PrincipalId::parse("anvil-principal").unwrap();
    TaskOwner {
        principal_key: "anvil-principal".parse().unwrap(),
        principal_kind: PrincipalKind::User,
        issuer: "https://issuer.anvil.example".parse().unwrap(),
        subject: "anvil-subject".parse().unwrap(),
        surface: "anvil-profile".parse().unwrap(),
        tenant_key: Some("anvil-tenant".parse().unwrap()),
        data_labels: BTreeSet::new(),
        authority: InvocationAuthority {
            work_context: WorkContextId::parse("anvil-mission").unwrap(),
            tenant: TenantId::parse("anvil-tenant").unwrap(),
            membership: WorkContextMembershipLevel::Owner,
            policy_revision: PolicyVersion::parse("r1").unwrap(),
            output_policy: WorkContextOutputPolicy {
                owner: AccessSubject::Principal(principal.clone()),
                initial_grants: Vec::new(),
                classification: None,
                data_labels: BTreeSet::new(),
            },
            provenance: InvocationProvenance::Direct {
                initiator: principal,
            },
        },
    }
}

/// A runtime of the server on its own process instance, as one replica runs it.
async fn replica(db: &TestDb) -> TaskRuntime {
    let store = db.server_store(&slug()).await;
    let instance = Instance::start(&store, &slug()).await.unwrap();
    TaskRuntime::new(store, slug(), Arc::new(instance))
}

async fn create(runtime: &TaskRuntime, mode: &str, recovery: RecoveryClass) -> TaskId {
    create_kind(runtime, STRIKE, mode, recovery).await
}

async fn create_kind(
    runtime: &TaskRuntime,
    task_type: TaskTypeName,
    mode: &str,
    recovery: RecoveryClass,
) -> TaskId {
    runtime
        .create(CreateTask {
            task_id: TaskId::new(),
            owner: owner(),
            server: slug(),
            task_type,
            request: json!({ "mode": mode }),
            recovery_class: recovery,
            idempotency_key: None,
            ttl_ms: Some(60 * 60 * 1_000),
            poll_interval_ms: Some(500),
            retention_pins: BTreeSet::new(),
        })
        .await
        .unwrap()
        .snapshot
        .task_id
}

fn struck() -> TaskTransition {
    TaskTransition::Succeeded {
        message: "struck".into(),
        result: json!({ "piece": "a horseshoe" }),
        result_uri: None,
    }
}

/// The work of a strike as its request's mode asks, once the Task has reported that it
/// runs.
async fn hold(
    ctx: TaskContext,
    started: mpsc::UnboundedSender<TaskId>,
) -> anyhow::Result<TaskTransition> {
    let _ = started.send(ctx.task_id());
    let mode = ctx.task().request["mode"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    match mode.as_str() {
        // Finish while the process stops, as work that completes during the drain.
        "publish_on_stop" => {
            ctx.cancelled().await;
            Ok(struck())
        }
        // Report a failure while the process stops.
        "fail_on_stop" => {
            ctx.cancelled().await;
            Ok(TaskTransition::Failed(TaskFailure::new(
                "anvil_cracked",
                "the anvil cracked",
            )))
        }
        // Stop at a checkpoint once the process stops.
        "stop_at_checkpoint" => {
            ctx.cancelled().await;
            ctx.checkpoint().await?;
            Ok(struck())
        }
        // Check in until something stops the work.
        "until_stopped" => loop {
            ctx.checkpoint().await?;
            tokio::time::sleep(Duration::from_millis(20)).await;
        },
        // Report a genuine failure once the cancellation arrives.
        "fail_once_cancelled" => loop {
            match ctx.checkpoint().await {
                Ok(()) => tokio::time::sleep(Duration::from_millis(20)).await,
                Err(TaskStop::Cancelled) => {
                    return Ok(TaskTransition::Failed(TaskFailure::new(
                        "anvil_cracked",
                        "the anvil cracked",
                    )));
                }
                Err(stop) => return Err(stop.into()),
            }
        },
        other => anyhow::bail!("no strike mode {other}"),
    }
}

/// A worker of `runtime` that runs every strike with `handler` until it is stopped.
fn start<H, Fut>(
    runtime: &TaskRuntime,
    handler: H,
) -> (CancellationToken, tokio::task::JoinHandle<()>)
where
    H: Fn(TaskContext) -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = anyhow::Result<TaskTransition>> + Send + 'static,
{
    start_worker(TaskWorker::new(runtime.clone()), handler)
}

fn start_worker<H, Fut>(
    worker: TaskWorker,
    handler: H,
) -> (CancellationToken, tokio::task::JoinHandle<()>)
where
    H: Fn(TaskContext) -> Fut + Clone + Send + Sync + 'static,
    Fut: Future<Output = anyhow::Result<TaskTransition>> + Send + 'static,
{
    let stop = CancellationToken::new();
    let worker = worker.drain(Duration::from_secs(10));
    let join = tokio::spawn({
        let stop = stop.clone();
        async move { worker.run(handler, stop).await.unwrap() }
    });
    (stop, join)
}

async fn started(receiver: &mut mpsc::UnboundedReceiver<TaskId>, tasks: usize) {
    tokio::time::timeout(DEADLINE, async {
        for _ in 0..tasks {
            receiver.recv().await.unwrap();
        }
    })
    .await
    .expect("the worker never started its Tasks");
}

/// Wait until the stored Task satisfies `done`, reaping dead instances as a replica would.
async fn stored_until(
    db: &TestDb,
    runtime: &TaskRuntime,
    task: TaskId,
    done: impl Fn(&TaskSnapshot) -> bool,
) -> TaskSnapshot {
    tokio::time::timeout(DEADLINE, async {
        loop {
            if let Some(snapshot) = runtime.get(task).await.unwrap()
                && done(&snapshot)
            {
                return snapshot;
            }
            db.a.reap_instances().await.unwrap();
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("Task {task} never reached the expected stored state"))
}

/// The Task as a stopped worker left it: claimed by its instance, with no outcome and no
/// cancellation invented for it.
async fn left_running(runtime: &TaskRuntime, task: TaskId) {
    let snapshot = runtime.get(task).await.unwrap().unwrap();
    assert!(!snapshot.is_terminal(), "{task} settled: {snapshot:?}");
    assert_eq!(snapshot.owner_instance, Some(runtime.instance().id()));
    assert!(snapshot.cancel_requested_at.is_none());
    assert!(snapshot.result.is_none());
    assert!(snapshot.error.is_none());
}

#[tokio::test]
async fn a_stopping_worker_leaves_resumable_tasks_for_another_worker_to_finish() {
    within(async {
        let db = TestDb::new().await;
        let first = replica(&db).await;
        let tasks = [
            create(&first, "publish_on_stop", RecoveryClass::Resume).await,
            create(&first, "fail_on_stop", RecoveryClass::Resume).await,
            create(&first, "stop_at_checkpoint", RecoveryClass::Resume).await,
        ];
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let (stop, join) = start(&first, move |ctx| hold(ctx, sender.clone()));
        started(&mut receiver, tasks.len()).await;

        // The process stops: success, failure and an interrupted handler all wait for
        // recovery instead of settling.
        stop.cancel();
        join.await.unwrap();
        for task in tasks {
            left_running(&first, task).await;
        }

        // The process ends, its instance's recovery queues the Tasks, and a worker of
        // another process finishes each one.
        first.instance().shutdown().await.unwrap();
        let second = replica(&db).await;
        let (stop, join) = start(&second, |_ctx| async { Ok(struck()) });
        for task in tasks {
            let finished =
                stored_until(&db, &second, task, |snapshot| snapshot.is_terminal()).await;
            assert_eq!(finished.status, TaskStatus::Succeeded, "{finished:?}");
            assert_eq!(finished.result, Some(json!({ "piece": "a horseshoe" })));
        }
        stop.cancel();
        join.await.unwrap();
    })
    .await;
}

#[tokio::test]
async fn a_stopping_worker_keeps_the_behavior_of_a_non_resumable_recovery_class() {
    within(async {
        let db = TestDb::new().await;
        let first = replica(&db).await;
        let finished = create(
            &first,
            "publish_on_stop",
            RecoveryClass::InterruptedIndeterminate,
        )
        .await;
        let interrupted = create(
            &first,
            "stop_at_checkpoint",
            RecoveryClass::InterruptedIndeterminate,
        )
        .await;
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let (stop, join) = start(&first, move |ctx| hold(ctx, sender.clone()));
        started(&mut receiver, 2).await;
        stop.cancel();
        join.await.unwrap();

        // Work that finished during the drain is settled: the outcome is real.
        let snapshot = first.get(finished).await.unwrap().unwrap();
        assert_eq!(snapshot.status, TaskStatus::Succeeded);
        // Interrupted work is left to its recovery class, which fails it as indeterminate
        // once the process ends.
        left_running(&first, interrupted).await;
        first.instance().shutdown().await.unwrap();
        let other = replica(&db).await;
        let recovered =
            stored_until(&db, &other, interrupted, |snapshot| snapshot.is_terminal()).await;
        assert_eq!(recovered.status, TaskStatus::Failed);
        assert_eq!(
            recovered.error.map(|error| error.code),
            Some("interrupted_indeterminate".to_owned())
        );
    })
    .await;
}

#[tokio::test]
async fn a_cancellation_requested_on_another_replica_reaches_the_handler_at_its_checkpoint() {
    within(async {
        let db = TestDb::new().await;
        let first = replica(&db).await;
        let task = create(&first, "until_stopped", RecoveryClass::Resume).await;
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let (stop, join) = start(&first, move |ctx| hold(ctx, sender.clone()));
        started(&mut receiver, 1).await;

        // Another replica records the cancellation; this worker's token never hears it.
        let other = replica(&db).await;
        let requested = other.cancel(task).await.unwrap();
        assert_eq!(requested.status, TaskStatus::CancelRequested);
        let settled = stored_until(&db, &other, task, |snapshot| snapshot.is_terminal()).await;
        assert_eq!(settled.status, TaskStatus::Cancelled);
        assert!(settled.result.is_none());
        stop.cancel();
        join.await.unwrap();
    })
    .await;
}

#[tokio::test]
async fn each_task_kind_settles_a_pending_cancellation_by_its_own_policy() {
    within(async {
        let db = TestDb::new().await;
        let first = replica(&db).await;
        // Strikes keep the default policy; tempering names cancellation as the winner.
        let kept = create_kind(&first, STRIKE, "fail_once_cancelled", RecoveryClass::Resume).await;
        let replaced =
            create_kind(&first, TEMPER, "fail_once_cancelled", RecoveryClass::Resume).await;
        let (sender, mut receiver) = mpsc::unbounded_channel();
        let worker = TaskWorker::new(first.clone())
            .cancellation(TEMPER, ResumeCancellationPolicy::CancellationWins);
        let (stop, join) = start_worker(worker, move |ctx| hold(ctx, sender.clone()));
        started(&mut receiver, 2).await;

        let other = replica(&db).await;
        other.cancel(kept).await.unwrap();
        other.cancel(replaced).await.unwrap();
        let failed = stored_until(&db, &other, kept, |snapshot| snapshot.is_terminal()).await;
        assert_eq!(failed.status, TaskStatus::Failed);
        assert_eq!(
            failed.error.map(|error| error.code),
            Some("anvil_cracked".to_owned())
        );
        let cancelled =
            stored_until(&db, &other, replaced, |snapshot| snapshot.is_terminal()).await;
        assert_eq!(cancelled.status, TaskStatus::Cancelled);
        assert!(cancelled.error.is_none());
        stop.cancel();
        join.await.unwrap();
    })
    .await;
}

#[tokio::test]
async fn a_worker_keeps_working_after_its_database_sessions_end() {
    within(async {
        let db = TestDb::new().await;
        let runtime = replica(&db).await;
        let (stop, join) = start(&runtime, |_ctx| async { Ok(struck()) });
        let first = create(&runtime, "strike", RecoveryClass::Resume).await;
        stored_until(&db, &runtime, first, |snapshot| {
            snapshot.status == TaskStatus::Succeeded
        })
        .await;

        // The network between the server and the database fails for a moment. A replica's
        // reaps may delete the instance's row while its lock is free, so the instance may
        // come back as a new generation; either way the worker claims the next Task.
        assert!(db.end_sessions(runtime.platform_store()).await > 0);
        let second = create(&runtime, "strike", RecoveryClass::Resume).await;
        stored_until(&db, &runtime, second, |snapshot| {
            snapshot.status == TaskStatus::Succeeded
        })
        .await;
        assert!(!join.is_finished(), "the worker kept running");
        stop.cancel();
        join.await.unwrap();
    })
    .await;
}

/// A database error that a lost connection causes: no pooled connection in time.
fn lost_connection() -> anyhow::Error {
    anyhow::Error::from(TaskError::Store(StoreError::Timeout {
        operation: "acquiring a pooled database connection",
        seconds: 5,
    }))
    .context("striking the anvil")
}

#[tokio::test]
async fn work_a_lost_connection_interrupts_goes_to_its_recovery_instead_of_failing() {
    within(async {
        let db = TestDb::new().await;
        let runtime = replica(&db).await;
        let runs = Arc::new(AtomicUsize::new(0));
        let (stop, join) = start(&runtime, {
            let runs = runs.clone();
            move |ctx| {
                let runs = runs.clone();
                async move {
                    let mode = ctx.task().request["mode"].as_str().unwrap_or_default();
                    match mode {
                        // The first run loses its connection; the second finishes.
                        "interrupt_once" if runs.fetch_add(1, Ordering::SeqCst) == 0 => {
                            Err(lost_connection())
                        }
                        "interrupt_always" => Err(lost_connection()),
                        _ => Ok(struck()),
                    }
                }
            }
        });

        // A resumable Task runs again from its recovery and finishes.
        let resumable = create(&runtime, "interrupt_once", RecoveryClass::Resume).await;
        let finished =
            stored_until(&db, &runtime, resumable, |snapshot| snapshot.is_terminal()).await;
        assert_eq!(finished.status, TaskStatus::Succeeded, "{finished:?}");
        assert_eq!(runs.load(Ordering::SeqCst), 2);

        // A Task of the other recovery class ends as interrupted, not as failed work.
        let indeterminate = create(
            &runtime,
            "interrupt_always",
            RecoveryClass::InterruptedIndeterminate,
        )
        .await;
        let interrupted = stored_until(&db, &runtime, indeterminate, |snapshot| {
            snapshot.is_terminal()
        })
        .await;
        assert_eq!(interrupted.status, TaskStatus::Failed);
        assert_eq!(
            interrupted.error.map(|error| error.code),
            Some("interrupted_indeterminate".to_owned())
        );
        stop.cancel();
        join.await.unwrap();
    })
    .await;
}

#[tokio::test]
async fn a_worker_ends_once_its_login_is_removed() {
    within(async {
        let db = TestDb::new().await;
        let runtime = replica(&db).await;
        let stop = CancellationToken::new();
        let worker = TaskWorker::new(runtime.clone());
        let join = tokio::spawn({
            let stop = stop.clone();
            async move {
                worker
                    .run(|_ctx| async { Ok::<_, anyhow::Error>(struck()) }, stop)
                    .await
            }
        });
        let task = create(&runtime, "strike", RecoveryClass::Resume).await;
        stored_until(&db, &runtime, task, |snapshot| {
            snapshot.status == TaskStatus::Succeeded
        })
        .await;

        // The Gateway removed the server's registration and dropped its login.
        db.remove_login(runtime.platform_store()).await;
        let ended = tokio::time::timeout(DEADLINE, join)
            .await
            .expect("the worker ended once its login was removed")
            .unwrap();
        assert!(
            matches!(ended, Err(TaskError::Store(StoreError::InstanceLost))),
            "{ended:?}"
        );
        assert!(runtime.instance().is_lost());
    })
    .await;
}
