//! Public client observations over real, stateless MCP HTTP. No domain server,
//! database, token service or mutating-call retry is part of this fixture.
use super::*;
use axum::{Router, http::StatusCode, response::IntoResponse};
use rmcp::{
    ServerHandler,
    model::{
        CreateTaskResult, ErrorData, ReadResourceResponse, ServerCapabilities, ServerConfig, Task,
        TaskStatus,
    },
    service::{RequestContext, RoleServer, SubscriptionContext},
    transport::streamable_http_server::StreamableHttpService,
};
use serde::{Deserialize, Serialize};
use std::{
    io,
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
};
use tokio::sync::mpsc;
use veoveo_types::ResourceAddress;

const URI: &str = "observation://value";

struct Provider;
impl ProviderContract for Provider {
    fn slug() -> ServerSlug {
        "observation".parse().unwrap()
    }
}
struct Address;
impl ResourceAddress for Address {
    type Error = io::Error;
    fn parse(uri: &ResourceUri) -> Result<Self, Self::Error> {
        if uri.as_str() == URI {
            Ok(Self)
        } else {
            Err(io::Error::other("unknown fixture address"))
        }
    }
    fn to_uri(&self) -> Result<ResourceUri, Self::Error> {
        Ok(ResourceUri::new(URI).unwrap())
    }
}
#[derive(Debug, Serialize, Deserialize, schemars::JsonSchema, PartialEq)]
struct Value {
    value: usize,
}
impl crate::definition::SingleOutput for Value {}
struct ReadValue;
impl ResourceRead for ReadValue {
    type Provider = Provider;
    type Address = Address;
    type Content = Value;
    fn address(&self) -> Address {
        Address
    }
}
#[derive(Serialize)]
struct Run {}
impl ToolCall for Run {
    type Provider = Provider;
    type Output = Value;
    const TOOL: &'static str = "run";
}
impl TaskToolCall for Run {}

#[derive(Clone, Copy, Debug)]
enum Directive {
    End,
    Push,
    Refuse,
}
#[derive(Clone, Copy, PartialEq)]
enum Fault {
    None,
    Read,
    Decode,
    Content,
    Ack,
    Admission,
}
#[derive(Clone)]
struct Source {
    value: Arc<AtomicUsize>,
    finished: Arc<AtomicBool>,
    calls: Arc<AtomicUsize>,
    cancellations: Arc<AtomicUsize>,
    listens: Arc<AtomicUsize>,
    opened: mpsc::UnboundedSender<mpsc::UnboundedSender<Directive>>,
    reads: mpsc::UnboundedSender<()>,
    fault: Fault,
}
impl Source {
    fn detailed(&self) -> DetailedTask {
        let payload = if self.finished.load(Ordering::SeqCst) {
            let mut result = CallToolResult::success(vec![]);
            result.structured_content =
                Some(serde_json::json!({"value": self.value.load(Ordering::SeqCst)}));
            TaskPayload::Completed {
                result: serde_json::to_value(result)
                    .unwrap()
                    .as_object()
                    .unwrap()
                    .clone(),
            }
        } else {
            TaskPayload::Working
        };
        DetailedTask::new(
            Task::new(
                "fixture-task",
                TaskStatus::Working,
                "2026-07-28T00:00:00Z",
                "2026-07-28T00:00:00Z",
            ),
            payload,
        )
    }
}
impl ServerHandler for Source {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_tasks()
                .enable_resources()
                .enable_resources_subscribe()
                .build(),
        )
    }
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        assert_eq!(request.name, "observation__run");
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(CreateTaskResult::new(self.detailed().task).into())
    }
    async fn cancel_task(
        &self,
        _: CancelTaskParams,
        _: RequestContext<RoleServer>,
    ) -> Result<(), ErrorData> {
        self.cancellations.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, ErrorData> {
        assert_eq!(request.uri, URI);
        if self.fault == Fault::Read {
            return Err(ErrorData::invalid_params("fixture read refused", None));
        }
        let text = if self.fault == Fault::Decode {
            "[]".to_owned()
        } else {
            serde_json::json!({"value": self.value.load(Ordering::SeqCst)}).to_string()
        };
        self.reads.send(()).unwrap();
        let uri = if self.fault == Fault::Content {
            "observation://wrong"
        } else {
            URI
        };
        Ok(ReadResourceResult::new(vec![
            ResourceContents::text(text, uri).with_mime_type("application/json"),
        ])
        .into())
    }
    fn accepted_subscription_filter(
        &self,
        requested: &SubscriptionFilter,
    ) -> Option<SubscriptionFilter> {
        Some(if self.fault == Fault::Ack {
            SubscriptionFilter::new()
        } else {
            requested.clone()
        })
    }
    async fn listen(&self, context: SubscriptionContext) -> Result<(), ErrorData> {
        let task = context.accepted().task_ids.is_some();
        let (send, mut receive) = mpsc::unbounded_channel();
        self.opened.send(send).unwrap();
        if task {
            context
                .sink()
                .notify_task_status(self.detailed())
                .await
                .unwrap();
        }
        loop {
            tokio::select! {
                () = context.cancelled() => return Ok(()),
                command = receive.recv() => match command {
                    Some(Directive::End) | None => return Ok(()),
                    Some(Directive::Refuse) => return Err(ErrorData::invalid_params("fixture listen refused", None)),
                    Some(Directive::Push) if task => context.sink().notify_task_status(self.detailed()).await.unwrap(),
                    Some(Directive::Push) => context.sink().notify_resource_updated(URI).await.unwrap(),
                }
            }
        }
    }
}

struct Fixture {
    client: HubClient,
    source: Source,
    opened: mpsc::UnboundedReceiver<mpsc::UnboundedSender<Directive>>,
    reads: mpsc::UnboundedReceiver<()>,
    server: tokio::task::JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
    }
}
impl Fixture {
    async fn new(fault: Fault) -> Self {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let (opened, receive) = mpsc::unbounded_channel();
        let (reads, read) = mpsc::unbounded_channel();
        let source = Source {
            value: Arc::default(),
            finished: Arc::default(),
            calls: Arc::default(),
            cancellations: Arc::default(),
            listens: Arc::default(),
            opened,
            reads,
            fault,
        };
        let served = source.clone();
        let count = source.listens.clone();
        let service = StreamableHttpService::new(
            move || Ok(served.clone()),
            crate::stateless_session_manager(),
            crate::canonical_streamable_http_server_config(),
        );
        let router = Router::new()
            .nest_service("/mcp", service)
            .layer(axum::middleware::from_fn(
                move |request: axum::extract::Request, next: axum::middleware::Next| {
                    let count = count.clone();
                    async move {
                        assert_eq!(
                            request.headers().get("authorization").unwrap(),
                            "Bearer fixture-token"
                        );
                        if request
                            .headers()
                            .get("mcp-method")
                            .is_some_and(|method| method == "subscriptions/listen")
                        {
                            count.fetch_add(1, Ordering::SeqCst);
                            if fault == Fault::Admission {
                                return StatusCode::FORBIDDEN.into_response();
                            }
                        }
                        next.run(request).await
                    }
                },
            ));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/mcp", listener.local_addr().unwrap())
            .parse()
            .unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        Self {
            client: HubClient::new(
                endpoint,
                reqwest::Client::builder().no_proxy().build().unwrap(),
                StaticBearer::new("fixture-token"),
            ),
            source,
            opened: receive,
            reads: read,
            server,
        }
    }
}

async fn bounded<T>(work: impl Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(60), work)
        .await
        .expect("observation fixture exceeded one minute")
}

#[tokio::test]
async fn resources_survive_four_healthy_ends_reconcile_a_gap_and_receive_a_later_push() {
    bounded(async {
        let mut fixture = Fixture::new(Fault::None).await;
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let capture = seen.clone();
        let observing = fixture
            .client
            .read(ReadValue)
            .until(move |value| {
                capture.lock().unwrap().push(value.value);
                value.value == 9
            })
            .timeout(Duration::from_secs(40))
            .into_future();
        let mut observing = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(observing));
        let mut control = fixture.opened.recv().await.unwrap();
        fixture.reads.recv().await.unwrap();
        for _ in 0..4 {
            // This is a wire regression with actual socket scheduling, separate
            // from the paused-time tests of exact backoff below.
            tokio::time::sleep(Duration::from_millis(5100)).await;
            control.send(Directive::End).unwrap();
            fixture.source.value.store(1, Ordering::SeqCst);
            control = tokio::select! {
                control = fixture.opened.recv() => control.unwrap(),
                result = &mut observing => panic!("resource observation ended before reopening: {result:?}"),
            };
            fixture.reads.recv().await.unwrap();
        }
        fixture.source.value.store(9, Ordering::SeqCst);
        control.send(Directive::Push).unwrap();
        assert_eq!(observing.await.unwrap().unwrap(), Value { value: 9 });
        assert_eq!(*seen.lock().unwrap(), [0, 1, 9]);
        assert_eq!(fixture.source.listens.load(Ordering::SeqCst), 5);
        assert_eq!(fixture.source.calls.load(Ordering::SeqCst), 0);
    })
    .await;
}

#[tokio::test]
async fn task_reopening_never_repeats_creation_and_timeout_does_not_cancel_the_task() {
    bounded(async {
        let mut fixture = Fixture::new(Fault::None).await;
        let mut observing = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(
            fixture
                .client
                .call(Run {})
                .timeout(Duration::from_secs(15))
                .into_future(),
        ));
        let mut control = fixture.opened.recv().await.unwrap();
        for _ in 0..4 {
            control.send(Directive::End).unwrap();
            control = tokio::select! {
                control = fixture.opened.recv() => control.unwrap(),
                result = &mut observing => panic!("Task observation ended before reopening: {result:?}"),
            };
        }
        fixture.source.value.store(7, Ordering::SeqCst);
        fixture.source.finished.store(true, Ordering::SeqCst);
        control.send(Directive::Push).unwrap();
        assert_eq!(observing.await.unwrap().unwrap(), Value { value: 7 });
        assert_eq!(fixture.source.calls.load(Ordering::SeqCst), 1);
        assert_eq!(fixture.source.listens.load(Ordering::SeqCst), 5);

        fixture.source.finished.store(false, Ordering::SeqCst);
        let task = fixture.client.start(Run {}).await.unwrap();
        assert!(matches!(
            task.timeout(Duration::from_millis(100)).await,
            Err(CallError::Timeout(_))
        ));
        assert_eq!(fixture.source.calls.load(Ordering::SeqCst), 2);
        assert_eq!(fixture.source.cancellations.load(Ordering::SeqCst), 0);
        assert!(!fixture.source.finished.load(Ordering::SeqCst));
    })
    .await;
}

#[tokio::test]
async fn admission_acknowledgment_read_and_decode_errors_are_terminal() {
    bounded(async {
        for fault in [
            Fault::Admission,
            Fault::Ack,
            Fault::Read,
            Fault::Decode,
            Fault::Content,
        ] {
            let fixture = Fixture::new(fault).await;
            let result = fixture
                .client
                .read(ReadValue)
                .until(|_| false)
                .timeout(Duration::from_secs(5))
                .await;
            assert!(match fault {
                Fault::Admission => matches!(result, Err(CallError::Transport(_))),
                Fault::Ack => matches!(result, Err(CallError::Unsubscribed { .. })),
                Fault::Read => matches!(result, Err(CallError::Rejected { code: -32602, .. })),
                Fault::Decode => matches!(result, Err(CallError::Decode(_))),
                Fault::Content => matches!(result, Err(CallError::Content { .. })),
                Fault::None => unreachable!(),
            });
            assert_eq!(fixture.source.listens.load(Ordering::SeqCst), 1);
        }
        let mut fixture = Fixture::new(Fault::None).await;
        let observing = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(
            fixture
                .client
                .read(ReadValue)
                .until(|_| false)
                .timeout(Duration::from_secs(5))
                .into_future(),
        ));
        let control = fixture.opened.recv().await.unwrap();
        fixture.reads.recv().await.unwrap();
        control.send(Directive::Refuse).unwrap();
        assert!(matches!(
            observing.await.unwrap(),
            Err(CallError::Rejected { code: -32602, .. })
        ));
        assert_eq!(fixture.source.listens.load(Ordering::SeqCst), 1);
    })
    .await;
}

#[tokio::test]
async fn rapid_http_closures_are_rate_limited_until_deadline_and_stream_drop_stops_reopening() {
    bounded(async {
        let mut fixture = Fixture::new(Fault::None).await;
        // Establish the connection outside the observation's recovery deadline.
        fixture.client.read(ReadValue).await.unwrap();
        let mut observing = tokio_util::task::AbortOnDropHandle::new(tokio::spawn(
            fixture
                .client
                .read(ReadValue)
                .until(|_| false)
                .timeout(Duration::from_millis(500))
                .into_future(),
        ));
        let result = loop {
            tokio::select! {
                result = &mut observing => break result.unwrap(),
                Some(control) = fixture.opened.recv() => { control.send(Directive::End).unwrap(); }
            }
        };
        assert!(matches!(result, Err(CallError::Timeout(_))));
        let count = fixture.source.listens.load(Ordering::SeqCst);
        // Initial opening, then 100ms and 200ms. The next 400ms wait is
        // interrupted by the caller's total deadline, not a lifetime attempt cap.
        assert!((1..=3).contains(&count), "rapid opening count: {count}");

        drop(fixture);
        let mut fixture = Fixture::new(Fault::None).await;
        let mut updates = fixture.client.read(ReadValue).updates();
        assert_eq!(updates.next().await.unwrap().unwrap(), Value { value: 0 });
        let control = fixture.opened.recv().await.unwrap();
        control.send(Directive::End).unwrap();
        // Drive next() while the source ends, then drop the lazy observation.
        // The paused-time test below separately covers cancellation during delay.
        assert!(futures::FutureExt::now_or_never(updates.next()).is_none());
        drop(updates);
        let count = fixture.source.listens.load(Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(fixture.source.listens.load(Ordering::SeqCst), count);
    })
    .await;
}
