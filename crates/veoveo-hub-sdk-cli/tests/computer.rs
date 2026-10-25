//! The CLI as a Computer runs it: the Computer template's environment names the Hub,
//! the `computer` profile and the variable holding the owner's credential placeholder,
//! which the sandbox's proxy replaces. A local server records every request it gets.

use std::sync::{Arc, Mutex};

use axum::{
    Router,
    extract::{Request, State},
    http::{StatusCode, header},
};

#[derive(Debug, Clone, PartialEq, Eq)]
struct Seen {
    path: String,
    authorization: Option<String>,
}

type Ledger = Arc<Mutex<Vec<Seen>>>;

async fn record(State(ledger): State<Ledger>, request: Request) -> StatusCode {
    ledger.lock().unwrap().push(Seen {
        path: request.uri().path().to_owned(),
        authorization: request
            .headers()
            .get(header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned),
    });
    StatusCode::FORBIDDEN
}

async fn hub() -> (String, Ledger) {
    let ledger = Ledger::default();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let app = Router::new().fallback(record).with_state(ledger.clone());
    tokio::spawn(async move { axum::serve(listener, app).await });
    (origin, ledger)
}

/// The CLI with the Computer template's environment and nothing else of the caller's.
fn in_a_computer(origin: &str, arguments: &[&str]) -> tokio::process::Command {
    let home = std::env::temp_dir().join(format!(
        "veoveo-cli-computer-{}",
        uuid::Uuid::new_v4().simple()
    ));
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_veoveo-hub-sdk-cli"));
    command
        .args(arguments)
        .env_remove("VEOVEO_CLI_KEY_DIRECTORY")
        .env_remove("VEOVEO_CLI_GRANT_DIRECTORY")
        .env_remove("VEOVEO_CLI_CONNECT")
        .env_remove("XDG_CONFIG_HOME")
        .env("HOME", home)
        .env("VEOVEO_CLI_HUB", origin)
        .env("VEOVEO_CLI_PROFILE", "computer")
        .env("VEOVEO_CLI_BEARER_ENV", "COMPUTER_HUB_REFERENCE")
        .env(
            "COMPUTER_HUB_REFERENCE",
            "openshell:resolve:env:COMPUTER_HUB_REFERENCE",
        )
        .env("HF_TOKEN", "hf-token");
    command
}

#[tokio::test]
async fn hub_commands_use_the_computer_profile_with_the_placeholder_as_bearer() {
    let (origin, ledger) = hub().await;
    let output = in_a_computer(&origin, &["tools"]).output().await.unwrap();
    // The recording server refuses every request, so the listing fails after its attempt.
    assert!(!output.status.success());
    let seen = ledger.lock().unwrap().clone();
    assert!(!seen.is_empty(), "the CLI sent no request");
    for request in &seen {
        assert_eq!(request.path, "/mcp/computer", "{request:?}");
        assert_eq!(
            request.authorization.as_deref(),
            Some("Bearer openshell:resolve:env:COMPUTER_HUB_REFERENCE"),
            "{request:?}"
        );
    }
}

#[tokio::test]
async fn login_logout_and_ask_say_they_do_not_apply_and_contact_nothing() {
    let (origin, ledger) = hub().await;
    for arguments in [
        &["login", "--no-browser"][..],
        &["logout"],
        &["ask", "What time is it in Tokyo?"],
    ] {
        let output = in_a_computer(&origin, arguments).output().await.unwrap();
        assert!(!output.status.success(), "{arguments:?}");
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(
            error.contains(&format!("`{}` does not apply here", arguments[0]))
                && error.contains("COMPUTER_HUB_REFERENCE"),
            "{error}"
        );
    }
    assert_eq!(*ledger.lock().unwrap(), Vec::<Seen>::new());
}
