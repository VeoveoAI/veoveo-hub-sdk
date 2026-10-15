//! Real requests a person would make, worked by a chat model through a running
//! installation's Gateway and checked by their outcome, never by the model's wording.
//!
//! The tests are ignored by default. Run them against a running installation with the
//! callers' keys in `VEOVEO_CLI_KEY_DIRECTORY`, the Gateway's origin in
//! `VEOVEO_CLI_GATEWAY` (default `http://vv-gateway.internal:8080`), `VEOVEO_CLI_CONNECT`
//! on a host that reaches the Gateway through a published port, the model in
//! `VEOVEO_CLI_MODEL` and `VEOVEO_CLI_MODEL_URL`, and its token in `HF_TOKEN`:
//!
//! ```sh
//! cargo nextest run -p veoveo-sdk-rust-cli --test use_cases --run-ignored only
//! ```

use std::{net::SocketAddr, path::PathBuf, time::Duration};

use anyhow::{Context, Result, ensure};
use rmcp::model::ResourceContents;
use serde_json::Value;
use url::Url;
use veoveo_sdk_rust_cli::{
    agent::{self, Limits, Stop, Transcript},
    gateway::GatewayAccess,
    mcp::Session,
    model::ChatModel,
};
use veoveo_types::{OAuthClientId, ScopeName};

const LIMITS: Limits = Limits {
    max_turns: 12,
    tool_deadline: Duration::from_secs(600),
};

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

fn access(caller: &str) -> Result<GatewayAccess> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    Ok(GatewayAccess {
        issuer: Url::parse(
            &env("VEOVEO_CLI_GATEWAY")
                .unwrap_or_else(|| "http://vv-gateway.internal:8080".to_owned()),
        )?,
        connect: env("VEOVEO_CLI_CONNECT")
            .map(|address| address.parse::<SocketAddr>())
            .transpose()?,
        keys: PathBuf::from(
            env("VEOVEO_CLI_KEY_DIRECTORY")
                .context("VEOVEO_CLI_KEY_DIRECTORY names the callers' keys")?,
        ),
        caller: OAuthClientId::parse(caller)?,
        profile: "local".to_owned(),
        scope: ScopeName::parse("mcp")?,
    })
}

fn model() -> Result<ChatModel> {
    let mut base = Url::parse(
        &env("VEOVEO_CLI_MODEL_URL")
            .unwrap_or_else(|| "https://router.huggingface.co/v1/".to_owned()),
    )?;
    if !base.path().ends_with('/') {
        base.set_path(&format!("{}/", base.path()));
    }
    Ok(ChatModel {
        base,
        model: env("VEOVEO_CLI_MODEL")
            .unwrap_or_else(|| "XiaomiMiMo/MiMo-V2.6-Pro-RL:novita".to_owned()),
        token: env("HF_TOKEN").context("HF_TOKEN holds the model endpoint's token")?,
        max_tokens: 4096,
        http: reqwest::Client::new(),
    })
}

/// Work on `request` as `caller` and return what happened.
async fn ask(caller: &str, request: &str) -> Result<(Session, Transcript)> {
    let mut session = Session::open(access(caller)?).await?;
    let transcript = agent::ask(&mut session, &model()?, request, LIMITS, |_| {}).await?;
    Ok((session, transcript))
}

/// A resource whose first content is JSON text, as Time and Artifact serve their records.
async fn read_json(session: &Session, uri: &str) -> Result<Value> {
    let read = session.read_resource(uri).await?;
    let Some(ResourceContents::TextResourceContents { text, .. }) = read.contents.first() else {
        anyhow::bail!("`{uri}` served no text");
    };
    Ok(serde_json::from_str(text)?)
}

/// The structured results of every successful call to `tool`.
fn results<'a>(transcript: &'a Transcript, tool: &'a str) -> impl Iterator<Item = &'a Value> {
    transcript
        .steps
        .iter()
        .filter(move |step| step.tool == tool && !step.is_error)
        .filter_map(|step| step.result.get("structuredContent"))
}

#[tokio::test]
#[ignore = "needs a running installation and a model endpoint"]
async fn a_member_schedules_an_event_that_time_then_serves() -> Result<()> {
    let title = format!("Use case review {}", uuid::Uuid::now_v7().simple());
    let (session, transcript) = ask(
        "acceptance-member",
        &format!(
            "Create an event named '{title}' for tomorrow at 09:00 UTC, then tell me what time that is in Tokyo."
        ),
    )
    .await?;
    ensure!(transcript.stop == Stop::Answered, "{:?}", transcript.stop);
    let created = results(&transcript, "time__create_event")
        .find(|event| event["name"] == title)
        .context("the model created no event with the requested name")?;
    // Time lists each event with the address it serves the event at.
    let listing = read_json(&session, "time://events").await?;
    let uri = listing["items"]
        .as_array()
        .context("the event listing holds items")?
        .iter()
        .find(|item| item["title"] == title)
        .and_then(|item| item["uri"].as_str())
        .context("Time does not list the event the model created")?;
    let event = read_json(&session, uri).await?;
    ensure!(
        event["eventId"] == created["eventId"] && event["name"] == title,
        "Time serves another event at {uri}: {event}"
    );
    session.close().await
}

#[tokio::test]
#[ignore = "needs a running installation, its fake media provider and a model endpoint"]
async fn an_admin_generates_an_image_that_artifact_stores() -> Result<()> {
    let (session, transcript) = ask(
        "acceptance-admin",
        "Generate an image of a red circle on a white background with the fake text-to-image model, and tell me where it is stored.",
    )
    .await?;
    ensure!(transcript.stop == Stop::Answered, "{:?}", transcript.stop);
    let generation = results(&transcript, "media__run")
        .next()
        .context("the model ran no generation")?;
    let artifact = generation["artifacts"][0]["artifactUri"]
        .as_str()
        .context("the generation stored no artifact")?;
    let stored = read_json(&session, artifact).await?;
    ensure!(
        stored["artifact"]["mediaType"] == "image/png",
        "Artifact does not hold the generated image as image/png: {stored}"
    );
    session.close().await
}

#[tokio::test]
#[ignore = "needs a running installation and a model endpoint"]
async fn a_member_cannot_run_a_billed_generation() -> Result<()> {
    let (session, transcript) = ask(
        "acceptance-member",
        "Generate an image of a red circle with the fake text-to-image model.",
    )
    .await?;
    ensure!(
        results(&transcript, "media__run").next().is_none(),
        "the member ran a billed generation"
    );
    session.close().await
}
