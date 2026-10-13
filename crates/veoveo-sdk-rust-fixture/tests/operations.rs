//! Operations and usage through the SDK: Foundry casts in Kiln, a fake provider, from
//! durable Tasks on a private PostgreSQL database, reached through the in-process mesh.
//! A cast parks on its operation, Kiln's signed callback settles it on the server's
//! public route, and the resumed Task records its billed cost.
use std::{future::Future, sync::Arc, time::Duration};

use axum::http::StatusCode;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
use veoveo_sdk_rust::{
    durable_tasks::{CallbackUrls, TaskWorker},
    hosting::{
        operations::{operation_callbacks, provider_events},
        testing::{self, RunningMesh, TestMesh},
    },
    usage::{TASK_USAGE_META_KEY, UsageLedger},
};
use veoveo_sdk_rust_fixture::{
    foundry::{self, FoundryDomain, FoundryTasks, FoundryUsage, Furnace, cast_operation},
    kiln::{Kiln, KilnEvent},
};
use veoveo_task_runtime::{Instance, OperationState, ProviderName, TaskRuntime, TaskStatus};
use veoveo_test_fixtures::store::TestDb;
use veoveo_types::{PermissionName, ServerSlug, TaskId};

const SECRET: &[u8] = b"kiln shared secret";
const DEADLINE: Duration = Duration::from_secs(30);

fn slug() -> ServerSlug {
    ServerSlug::parse("foundry").unwrap()
}

/// Run one test case within its deadline.
async fn within<T>(case: impl Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(90), case)
        .await
        .expect("test case exceeded 90 seconds")
}

/// A Foundry worker on its own process instance.
struct Worker {
    runtime: TaskRuntime,
    stop: CancellationToken,
    join: tokio::task::JoinHandle<()>,
}

impl Worker {
    async fn start(db: &TestDb, callbacks: &CallbackUrls, furnace: &Furnace) -> Self {
        let store = db.server_store(&slug()).await;
        let instance = Arc::new(Instance::start(&store, &slug()).await.unwrap());
        let runtime = TaskRuntime::new(store, slug(), instance);
        let stop = CancellationToken::new();
        let worker = TaskWorker::new(runtime.clone())
            .callbacks(callbacks.clone())
            .usage(UsageLedger::new(&runtime));
        let join = tokio::spawn({
            let furnace = furnace.clone();
            let stop = stop.clone();
            async move {
                let _ = foundry::work(worker, furnace, stop).await;
            }
        });
        Self {
            runtime,
            stop,
            join,
        }
    }

    /// Stop claiming and wait for the worker loop to end.
    async fn stop(self) -> TaskRuntime {
        self.stop.cancel();
        let _ = self.join.await;
        self.runtime
    }
}

struct Harness {
    db: TestDb,
    mesh: RunningMesh,
    kiln: Kiln,
    furnace: Furnace,
    callbacks: CallbackUrls,
    worker: Option<Worker>,
}

async fn harness() -> Harness {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let db = TestDb::new().await;
    let kiln = Kiln::start(SECRET).await;
    let furnace = Furnace::new(kiln.base().clone(), SECRET);
    // The route stores events through a runtime of its own, as the server's HTTP process.
    let store = db.server_store(&slug()).await;
    let routes = TaskRuntime::new(
        store.clone(),
        slug(),
        Arc::new(Instance::start(&store, &slug()).await.unwrap()),
    );
    UsageLedger::new(&routes)
        .declare::<FoundryUsage>()
        .await
        .unwrap();
    let mut mesh = TestMesh::bind().await;
    mesh.register(foundry::authorization().unwrap());
    let callbacks = CallbackUrls::new(mesh.base().clone(), slug()).unwrap();
    let tasks = FoundryTasks::new(routes.clone());
    mesh.serve(
        testing::for_domain::<FoundryDomain>()
            .public_routes(operation_callbacks(routes.clone(), furnace.clone()))
            .public_routes(provider_events(
                routes,
                ProviderName::parse("kiln").unwrap(),
                furnace.clone(),
            ))
            .handler(move || foundry::hosted(tasks.clone()))
            .build(),
    );
    let mesh = mesh
        .granting([PermissionName::parse("foundry.casts.make").unwrap()])
        .start();
    let worker = Worker::start(&db, &callbacks, &furnace).await;
    Harness {
        db,
        mesh,
        kiln,
        furnace,
        callbacks,
        worker: Some(worker),
    }
}

impl Harness {
    async fn cast(&self, mold: &str) -> String {
        let started = self
            .mesh
            .start_task("foundry", "foundry_cast", json!({"mold": mold}))
            .await;
        started["result"]["taskId"]
            .as_str()
            .unwrap_or_else(|| panic!("no Task started: {started}"))
            .to_owned()
    }

    async fn task(&self, task: &str) -> Value {
        self.mesh.task("foundry", task).await["result"].clone()
    }

    /// Wait until the Task's MCP view satisfies `done`.
    async fn until(&self, task: &str, done: impl Fn(&Value) -> bool) -> Value {
        tokio::time::timeout(DEADLINE, async {
            loop {
                let view = self.task(task).await;
                if done(&view) {
                    return view;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("Task {task} never reached the expected state"))
    }

    /// Wait until the stored Task satisfies `done`.
    async fn stored_until(
        &self,
        runtime: &TaskRuntime,
        task: &str,
        done: impl Fn(&veoveo_task_runtime::TaskSnapshot) -> bool,
    ) -> veoveo_task_runtime::TaskSnapshot {
        let id = TaskId::parse(task).unwrap();
        tokio::time::timeout(DEADLINE, async {
            loop {
                if let Some(snapshot) = runtime.get(id).await.unwrap()
                    && done(&snapshot)
                {
                    return snapshot;
                }
                // Another instance's reap moves a released Task on.
                self.db.a.reap_instances().await.unwrap();
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
        .await
        .unwrap_or_else(|_| panic!("Task {task} never reached the expected stored state"))
    }

    /// Wait until the cast parks on its submitted operation, and return Kiln's job.
    async fn parked(&self, runtime: &TaskRuntime, task: &str) -> String {
        let snapshot = self
            .stored_until(runtime, task, |snapshot| {
                snapshot.status == TaskStatus::Waiting && snapshot.owner_instance.is_none()
            })
            .await;
        let operation = runtime
            .operation(snapshot.task_id, &cast_operation().key)
            .await
            .unwrap()
            .expect("the cast recorded its operation");
        assert_eq!(operation.state, OperationState::Submitted);
        operation
            .external_id
            .expect("the cast recorded Kiln's job")
            .to_string()
    }

    fn runtime(&self) -> TaskRuntime {
        self.worker.as_ref().unwrap().runtime.clone()
    }
}

fn done(job: &str, billed: &str) -> KilnEvent {
    KilnEvent {
        job: job.to_owned(),
        state: "done".into(),
        piece: Some("a bronze bell".into()),
        billed_usd: Some(billed.into()),
        finished_at: Some("2026-10-13T12:00:00Z".into()),
    }
}

fn usage_of(view: &Value) -> Vec<Value> {
    view["_meta"][TASK_USAGE_META_KEY]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

#[tokio::test]
async fn a_signed_callback_settles_a_parked_cast_and_its_cost_reaches_the_task() {
    within(async {
        let harness = harness().await;
        let task = harness.cast("bell").await;
        let runtime = harness.runtime();
        let job = harness.parked(&runtime, &task).await;
        assert_eq!(harness.kiln.submissions(), 1);

        // Parked, the Task holds no worker and works on, asking its caller for nothing.
        let parked = harness.task(&task).await;
        assert_eq!(parked["status"], "working", "{parked}");
        let estimate = usage_of(&parked);
        assert_eq!(estimate.len(), 1, "{parked}");
        assert_eq!(estimate[0]["phase"], "estimate");
        assert_eq!(estimate[0]["money"]["amount"], foundry::ESTIMATE_USD);

        // A forged signature and a wrong token are refused before anything is stored.
        let event = done(&job, "0.05");
        assert_eq!(
            harness.kiln.send_forged(&event, "evt-forged").await,
            StatusCode::UNAUTHORIZED
        );
        let first = harness.kiln.event_id();
        assert_eq!(
            harness.kiln.send(&event, &first).await,
            StatusCode::NO_CONTENT
        );
        // Kiln repeats its delivery; the repeat changes nothing.
        assert_eq!(
            harness.kiln.send(&event, &first).await,
            StatusCode::NO_CONTENT
        );

        let completed = harness
            .until(&task, |view| view["status"] == "completed")
            .await;
        assert_eq!(
            completed["result"]["structuredContent"],
            json!({"piece": "a bronze bell", "billed_usd": "0.05"}),
            "{completed}"
        );
        // Actual usage replaces the estimate, in tasks/get and in the result itself.
        for usage in [usage_of(&completed), usage_of(&completed["result"])] {
            assert_eq!(usage.len(), 1, "{completed}");
            assert_eq!(usage[0]["kind"], "foundry.casts");
            assert_eq!(usage[0]["phase"], "actual");
            assert_eq!(usage[0]["unit"], "Units");
            assert_eq!(
                usage[0]["money"],
                json!({"amount": "0.05", "currency": "USD"})
            );
        }

        // A later final event is stored as late and leaves the outcome alone.
        let late = harness.kiln.event_id();
        assert_eq!(
            harness.kiln.send(&done(&job, "9.99"), &late).await,
            StatusCode::NO_CONTENT
        );
        let after = harness.task(&task).await;
        assert_eq!(after["result"], completed["result"]);
        assert_eq!(harness.kiln.submissions(), 1);
    })
    .await;
}

#[tokio::test]
async fn a_callback_for_no_operation_answers_not_found() {
    within(async {
        let harness = harness().await;
        let task = harness.cast("bell").await;
        let runtime = harness.runtime();
        harness.parked(&runtime, &task).await;
        let operation = runtime
            .operation(TaskId::parse(task.as_str()).unwrap(), &cast_operation().key)
            .await
            .unwrap()
            .unwrap();
        let body = serde_json::to_vec(&done("job-1", "0.05")).unwrap();
        let signature = veoveo_sdk_rust_fixture::kiln::sign(SECRET, &body);
        let http = reqwest::Client::new();
        let wrong = veoveo_task_runtime::CallbackToken::parse(&"A".repeat(43)).unwrap();
        for url in [
            harness
                .callbacks
                .operation(operation.operation_id, &wrong)
                .to_string(),
            format!("{}foundry/operations/not-an-id/token", harness.mesh.base()),
        ] {
            let response = http
                .post(url)
                .header("x-kiln-event", "evt-x")
                .header("x-kiln-signature", &signature)
                .body(body.clone())
                .send()
                .await
                .unwrap();
            assert_eq!(response.status().as_u16(), 404);
        }
        assert_eq!(harness.task(&task).await["status"], "working");
    })
    .await;
}

#[tokio::test]
async fn a_parked_cast_resumes_on_a_new_worker_after_a_restart() {
    within(async {
        let mut harness = harness().await;
        let task = harness.cast("bell").await;
        let runtime = harness.runtime();
        let job = harness.parked(&runtime, &task).await;

        // The worker's process ends; the parked Task holds no worker and stays parked.
        let old = harness.worker.take().unwrap().stop().await;
        let stopped = old
            .get(TaskId::parse(&task).unwrap())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(stopped.status, TaskStatus::Waiting);
        assert_eq!(stopped.owner_instance, None);
        old.instance().shutdown().await.unwrap();
        let replacement = Worker::start(&harness.db, &harness.callbacks, &harness.furnace).await;
        let snapshot = harness
            .stored_until(&replacement.runtime, &task, |snapshot| {
                snapshot.status == TaskStatus::Waiting
            })
            .await;
        assert_eq!(snapshot.owner_instance, None);
        harness.worker = Some(replacement);

        let event_id = harness.kiln.event_id();
        assert_eq!(
            harness.kiln.send(&done(&job, "0.05"), &event_id).await,
            StatusCode::NO_CONTENT
        );
        let completed = harness
            .until(&task, |view| view["status"] == "completed")
            .await;
        assert_eq!(
            completed["result"]["structuredContent"]["piece"],
            "a bronze bell"
        );
        assert_eq!(harness.kiln.submissions(), 1);
    })
    .await;
}

#[tokio::test]
async fn an_unsettled_dispatch_finds_its_job_instead_of_casting_again() {
    within(async {
        let mut harness = harness().await;
        harness.furnace.crash_after_next_submit();
        let task = harness.cast("bell").await;
        let first = harness.runtime();
        // Kiln accepted the job, and the worker died before it recorded the submission.
        let snapshot = harness
            .stored_until(&first, &task, |snapshot| {
                snapshot.status == TaskStatus::Running && harness.kiln.submissions() == 1
            })
            .await;
        let operation = first
            .operation(snapshot.task_id, &cast_operation().key)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(operation.state, OperationState::Dispatching);
        let old = harness.worker.take().unwrap().stop().await;
        old.instance().shutdown().await.unwrap();

        // Recovery queues the resume Task; the next worker reconciles with Kiln.
        let replacement = Worker::start(&harness.db, &harness.callbacks, &harness.furnace).await;
        let runtime = replacement.runtime.clone();
        harness.worker = Some(replacement);
        let job = harness.parked(&runtime, &task).await;
        assert_eq!(
            harness.kiln.job_of(&operation.operation_id.to_string()),
            Some(job.clone())
        );
        assert_eq!(harness.kiln.submissions(), 1);

        let event_id = harness.kiln.event_id();
        assert_eq!(
            harness.kiln.send(&done(&job, "0.05"), &event_id).await,
            StatusCode::NO_CONTENT
        );
        harness
            .until(&task, |view| view["status"] == "completed")
            .await;
        assert_eq!(harness.kiln.submissions(), 1);
    })
    .await;
}

#[tokio::test]
async fn cancelling_a_parked_cast_carries_it_to_kiln_and_keeps_the_receipt() {
    within(async {
        let harness = harness().await;
        let task = harness.cast("bell").await;
        let runtime = harness.runtime();
        let job = harness.parked(&runtime, &task).await;

        let id = TaskId::parse(task.as_str()).unwrap();
        runtime.cancel(id).await.unwrap();
        let cancelled = harness
            .until(&task, |view| view["status"] == "cancelled")
            .await;
        assert_eq!(cancelled["status"], "cancelled");
        assert_eq!(harness.kiln.cancelled(), std::slice::from_ref(&job));
        let operation = runtime
            .operation(id, &cast_operation().key)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            operation.cancel_receipt,
            Some(json!({"job": job, "cancelled": true}))
        );

        // Kiln's final event after the cancellation is stored late; the Task stays
        // cancelled.
        let event_id = harness.kiln.event_id();
        assert_eq!(
            harness.kiln.send(&done(&job, "0.05"), &event_id).await,
            StatusCode::NO_CONTENT
        );
        assert_eq!(harness.task(&task).await["status"], "cancelled");
    })
    .await;
}

#[tokio::test]
async fn an_event_on_the_providers_fixed_url_settles_the_cast_its_job_names() {
    within(async {
        let harness = harness().await;
        let task = harness.cast("bell").await;
        let runtime = harness.runtime();
        let job = harness.parked(&runtime, &task).await;

        let url = harness
            .mesh
            .base()
            .join("foundry/providers/kiln/events")
            .unwrap();
        // A job no operation submitted is unknown.
        let event_id = harness.kiln.event_id();
        assert_eq!(
            harness
                .kiln
                .send_to(url.clone(), &done("job-404", "0.05"), &event_id)
                .await,
            StatusCode::NOT_FOUND
        );
        let event_id = harness.kiln.event_id();
        assert_eq!(
            harness
                .kiln
                .send_to(url, &done(&job, "0.05"), &event_id)
                .await,
            StatusCode::NO_CONTENT
        );
        let completed = harness
            .until(&task, |view| view["status"] == "completed")
            .await;
        assert_eq!(
            completed["result"]["structuredContent"]["billed_usd"],
            "0.05"
        );
    })
    .await;
}
