//! `tasks get` and `tasks cancel` as a Computer runs them, against a real MCP server that
//! runs one Task of the caller's and refuses every other identifier, as the Hub refuses
//! a Task the caller does not reach. Its one tool is a write that names that Task in its
//! result, as a write whose follow-up work runs as the caller's Task does.

use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use axum::Router;
use rmcp::{
    ServerHandler,
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, CancelTaskParams, ContentBlock,
        DetailedTask, ErrorData, GetTaskParams, GetTaskResult, MetaObject, ServerCapabilities,
        ServerConfig, Task, TaskPayload, TaskStatus,
    },
    service::{RequestContext, RoleServer},
    transport::streamable_http_server::StreamableHttpService,
};
use serde_json::{Value, json};

const OWN_TASK: &str = "own-task";

#[derive(Clone, Default)]
struct OneTask {
    cancellations: Arc<AtomicUsize>,
}

impl OneTask {
    fn status(&self) -> DetailedTask {
        let cancelled = self.cancellations.load(Ordering::SeqCst) > 0;
        let (status, payload) = if cancelled {
            (TaskStatus::Cancelled, TaskPayload::Cancelled)
        } else {
            (TaskStatus::Working, TaskPayload::Working)
        };
        DetailedTask::new(
            Task::new(
                OWN_TASK,
                status,
                "2026-11-03T00:00:00Z",
                "2026-11-03T00:00:00Z",
            ),
            payload,
        )
    }
}

impl ServerHandler for OneTask {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_tasks()
                .build(),
        )
    }
    async fn call_tool(
        &self,
        _: CallToolRequestParams,
        _: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let mut result = CallToolResult::success(vec![ContentBlock::text("written")]);
        let mut meta = MetaObject::default();
        meta.0.insert(
            veoveo_hub_sdk::RELATED_TASK_META_KEY.to_owned(),
            json!({"taskId": OWN_TASK}),
        );
        result.meta = Some(meta);
        Ok(CallToolResponse::Complete(result))
    }
    async fn get_task(
        &self,
        request: GetTaskParams,
        _: RequestContext<RoleServer>,
    ) -> Result<GetTaskResult, ErrorData> {
        if request.task_id != OWN_TASK {
            return Err(ErrorData::invalid_params("no such Task", None));
        }
        Ok(GetTaskResult::new(self.status()))
    }
    async fn cancel_task(
        &self,
        request: CancelTaskParams,
        _: RequestContext<RoleServer>,
    ) -> Result<(), ErrorData> {
        if request.task_id != OWN_TASK {
            return Err(ErrorData::invalid_params("no such Task", None));
        }
        self.cancellations.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

/// The server at `{origin}/mcp/computer`, which answers only the Computer's placeholder.
async fn hub(server: OneTask) -> String {
    let served = server.clone();
    let service = StreamableHttpService::new(
        move || Ok(served.clone()),
        veoveo_hub_sdk::stateless_session_manager(),
        veoveo_hub_sdk::canonical_streamable_http_server_config(),
    );
    let router =
        Router::new()
            .nest_service("/mcp/computer", service)
            .layer(axum::middleware::from_fn(
                |request: axum::extract::Request, next: axum::middleware::Next| async move {
                    assert_eq!(
                        request.headers().get("authorization").unwrap(),
                        "Bearer openshell:resolve:env:COMPUTER_HUB_REFERENCE"
                    );
                    next.run(request).await
                },
            ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, router).await });
    origin
}

/// The CLI with the Computer template's environment, and its exit status and JSON output.
async fn cli(origin: &str, arguments: &[&str]) -> (bool, Value) {
    let output = tokio::process::Command::new(env!("CARGO_BIN_EXE_veoveo-hub-sdk-cli"))
        .args(arguments)
        .env_remove("VEOVEO_CLI_KEY_DIRECTORY")
        .env_remove("VEOVEO_CLI_GRANT_DIRECTORY")
        .env_remove("VEOVEO_CLI_CONNECT")
        .env("VEOVEO_CLI_HUB", origin)
        .env("VEOVEO_CLI_PROFILE", "computer")
        .env("VEOVEO_CLI_BEARER_ENV", "COMPUTER_HUB_REFERENCE")
        .env(
            "COMPUTER_HUB_REFERENCE",
            "openshell:resolve:env:COMPUTER_HUB_REFERENCE",
        )
        .output()
        .await
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    let printed = serde_json::from_str(&stdout).unwrap_or_else(|_| {
        panic!(
            "{arguments:?} printed no JSON: {stdout}{}",
            String::from_utf8_lossy(&output.stderr)
        )
    });
    (output.status.success(), printed)
}

#[tokio::test]
async fn a_computer_follows_the_task_a_write_names_and_an_unknown_one_is_refused() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let server = OneTask::default();
    let origin = hub(server.clone()).await;

    // A write names the caller's Task in its result's `_meta`, which `call` prints.
    let (ok, written) = cli(&origin, &["--json", "call", "probe__write", "{}"]).await;
    assert!(ok, "{written}");
    let task = written["_meta"][veoveo_hub_sdk::RELATED_TASK_META_KEY]["taskId"]
        .as_str()
        .unwrap_or_else(|| panic!("the call printed no related Task: {written}"));
    assert_eq!(task, OWN_TASK);

    // The caller's own Task: its status, then its cancellation and the status after.
    let (ok, read) = cli(&origin, &["--json", "tasks", "get", task]).await;
    assert!(ok, "{read}");
    assert_eq!(read["taskId"], OWN_TASK);
    assert_eq!(read["status"], "working");
    let (ok, cancelled) = cli(&origin, &["--json", "tasks", "cancel", OWN_TASK]).await;
    assert!(ok, "{cancelled}");
    assert_eq!(cancelled["status"], "cancelled");
    assert_eq!(server.cancellations.load(Ordering::SeqCst), 1);

    // An identifier the server refuses prints the JSON-RPC error and fails the command.
    for command in ["get", "cancel"] {
        let (ok, refused) = cli(&origin, &["--json", "tasks", command, "unknown"]).await;
        assert!(!ok, "{refused}");
        assert_eq!(
            refused,
            json!({"refused": {"code": -32602, "message": "no such Task"}})
        );
    }
    assert_eq!(server.cancellations.load(Ordering::SeqCst), 1);
}
