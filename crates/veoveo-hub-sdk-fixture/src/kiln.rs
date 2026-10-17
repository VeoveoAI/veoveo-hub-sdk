//! Kiln, the fake provider Foundry hands its casts to. It accepts a job with the
//! operation's identity and callback URL, finds a job by that identity, cancels a job,
//! and on the test's command calls the job's callback URL with an event signed by
//! HMAC-SHA256 over its body, as a webhook provider does. It runs on a loopback port and
//! reaches no outside service.
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
};
use serde::{Deserialize, Serialize};

/// The header that carries an event's identifier.
pub const EVENT_HEADER: &str = "x-kiln-event";
/// The header that carries the lowercase hexadecimal HMAC-SHA256 of the body.
pub const SIGNATURE_HEADER: &str = "x-kiln-signature";

/// A job submission.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Submit {
    pub operation: String,
    pub callback: Option<String>,
    pub mold: String,
}

/// The job Kiln accepted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Job {
    pub job: String,
}

/// The body of an event Kiln sends to a job's callback URL.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KilnEvent {
    pub job: String,
    /// `firing`, `done` or `cracked`; `done` and `cracked` are final.
    pub state: String,
    pub piece: Option<String>,
    /// What the job cost, as a decimal amount in US dollars.
    pub billed_usd: Option<String>,
    /// When the job finished, RFC 3339.
    pub finished_at: Option<String>,
}

impl KilnEvent {
    pub fn is_final(&self) -> bool {
        matches!(self.state.as_str(), "done" | "cracked")
    }
}

#[derive(Default)]
struct Jobs {
    by_job: BTreeMap<String, Submit>,
    by_operation: BTreeMap<String, String>,
    cancelled: Vec<String>,
}

struct Shared {
    secret: Vec<u8>,
    jobs: parking_lot::Mutex<Jobs>,
    submissions: AtomicU64,
    events: AtomicU64,
}

/// A running Kiln.
pub struct Kiln {
    base: url::Url,
    shared: Arc<Shared>,
    http: reqwest::Client,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for Kiln {
    fn drop(&mut self) {
        self.task.abort();
    }
}

impl Kiln {
    /// Starts Kiln on a loopback port, signing its events with `secret`.
    pub async fn start(secret: &[u8]) -> Self {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("Kiln binds a loopback port");
        let base = url::Url::parse(&format!(
            "http://{}",
            listener.local_addr().expect("a bound address")
        ))
        .expect("a loopback URL");
        let shared = Arc::new(Shared {
            secret: secret.to_vec(),
            jobs: parking_lot::Mutex::default(),
            submissions: AtomicU64::new(0),
            events: AtomicU64::new(0),
        });
        let router = Router::new()
            .route("/jobs", post(submit).get(find))
            .route("/jobs/{job}/cancel", post(cancel))
            .with_state(shared.clone());
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });
        Self {
            base,
            shared,
            http: reqwest::Client::new(),
            task,
        }
    }

    /// Where Foundry reaches Kiln.
    pub fn base(&self) -> &url::Url {
        &self.base
    }

    /// How many jobs Kiln accepted.
    pub fn submissions(&self) -> u64 {
        self.shared.submissions.load(Ordering::SeqCst)
    }

    /// The jobs Kiln was asked to cancel.
    pub fn cancelled(&self) -> Vec<String> {
        self.shared.jobs.lock().cancelled.clone()
    }

    /// The job Kiln accepted for `operation`.
    pub fn job_of(&self, operation: &str) -> Option<String> {
        self.shared.jobs.lock().by_operation.get(operation).cloned()
    }

    /// A fresh event identifier.
    pub fn event_id(&self) -> String {
        format!(
            "evt-{}",
            self.shared.events.fetch_add(1, Ordering::SeqCst) + 1
        )
    }

    /// Send `event` with `event_id` to its job's callback URL, signed, and return the
    /// HTTP status Foundry answered.
    pub async fn send(&self, event: &KilnEvent, event_id: &str) -> StatusCode {
        self.send_signed(event, event_id, &self.shared.secret).await
    }

    /// Send `event` signed with another secret.
    pub async fn send_forged(&self, event: &KilnEvent, event_id: &str) -> StatusCode {
        self.send_signed(event, event_id, b"not the shared secret")
            .await
    }

    /// Send `event` with `event_id` to a fixed `url`, as a provider configured with one
    /// webhook URL does, and return the HTTP status Foundry answered.
    pub async fn send_to(&self, url: url::Url, event: &KilnEvent, event_id: &str) -> StatusCode {
        self.post(url.to_string(), event, event_id, &self.shared.secret)
            .await
    }

    async fn send_signed(&self, event: &KilnEvent, event_id: &str, secret: &[u8]) -> StatusCode {
        let callback = self
            .shared
            .jobs
            .lock()
            .by_job
            .get(&event.job)
            .and_then(|submit| submit.callback.clone())
            .expect("the job has a callback URL");
        self.post(callback, event, event_id, secret).await
    }

    async fn post(
        &self,
        url: String,
        event: &KilnEvent,
        event_id: &str,
        secret: &[u8],
    ) -> StatusCode {
        let body = serde_json::to_vec(event).expect("an event serializes");
        let response = self
            .http
            .post(url)
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .header(EVENT_HEADER, event_id)
            .header(SIGNATURE_HEADER, sign(secret, &body))
            .body(body)
            .send()
            .await
            .expect("Foundry answers Kiln's callback");
        StatusCode::from_u16(response.status().as_u16()).expect("an HTTP status")
    }
}

/// The lowercase hexadecimal HMAC-SHA256 of `body` under `secret`.
pub fn sign(secret: &[u8], body: &[u8]) -> String {
    let key = aws_lc_rs::hmac::Key::new(aws_lc_rs::hmac::HMAC_SHA256, secret);
    hex::encode(aws_lc_rs::hmac::sign(&key, body).as_ref())
}

/// Whether `signature` is the HMAC-SHA256 of `body` under `secret`, compared in
/// constant time.
pub fn verify(secret: &[u8], body: &[u8], signature: &str) -> bool {
    let Ok(tag) = hex::decode(signature) else {
        return false;
    };
    let key = aws_lc_rs::hmac::Key::new(aws_lc_rs::hmac::HMAC_SHA256, secret);
    aws_lc_rs::hmac::verify(&key, body, &tag).is_ok()
}

async fn submit(State(shared): State<Arc<Shared>>, Json(submit): Json<Submit>) -> Response {
    let mut jobs = shared.jobs.lock();
    if let Some(job) = jobs.by_operation.get(&submit.operation) {
        // The operation identity is the idempotency key: a repeat names the same job.
        return Json(Job { job: job.clone() }).into_response();
    }
    let job = format!(
        "job-{}",
        shared.submissions.fetch_add(1, Ordering::SeqCst) + 1
    );
    jobs.by_operation
        .insert(submit.operation.clone(), job.clone());
    jobs.by_job.insert(job.clone(), submit);
    Json(Job { job }).into_response()
}

#[derive(Deserialize)]
struct FindQuery {
    operation: String,
}

async fn find(State(shared): State<Arc<Shared>>, Query(query): Query<FindQuery>) -> Response {
    match shared.jobs.lock().by_operation.get(&query.operation) {
        Some(job) => Json(Job { job: job.clone() }).into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

async fn cancel(State(shared): State<Arc<Shared>>, Path(job): Path<String>) -> Response {
    let mut jobs = shared.jobs.lock();
    if !jobs.by_job.contains_key(&job) {
        return StatusCode::NOT_FOUND.into_response();
    }
    jobs.cancelled.push(job.clone());
    Json(serde_json::json!({ "job": job, "cancelled": true })).into_response()
}
