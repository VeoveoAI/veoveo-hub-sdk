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

use std::{collections::BTreeMap, net::SocketAddr, path::PathBuf, time::Duration};

use anyhow::{Context, Result, bail, ensure};
use rmcp::model::ResourceContents;
use serde_json::Value;
use url::Url;
use veoveo_sdk_rust::{GatewayProfileId, client::GatewayClient};
use veoveo_sdk_rust_cli::{
    agent::{self, Limits, Stop, Transcript},
    gateway::GatewayAccess,
    model::ChatModel,
};
use veoveo_types::{OAuthClientId, ResourceTemplateUri, ResourceUri, ScopeName};

const LIMITS: Limits = Limits {
    max_turns: 12,
    tool_deadline: Duration::from_secs(600),
};

/// The template Time declares for one event.
const EVENT_TEMPLATE: &str = "time://events/{event_id}";

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

async fn client(caller: &str) -> Result<GatewayClient> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    GatewayAccess {
        gateway: Url::parse(
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
        profile: GatewayProfileId::parse("local")?,
        scope: ScopeName::parse("mcp")?,
    }
    .client()
    .await
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
async fn ask(caller: &str, request: &str) -> Result<(GatewayClient, Transcript)> {
    let client = client(caller).await?;
    let transcript = agent::ask(&client, &model()?, request, LIMITS, |_| {}).await?;
    Ok((client, transcript))
}

/// A resource whose content is JSON text, as Time and Artifact serve their records.
async fn read_json(client: &GatewayClient, uri: &ResourceUri) -> Result<Value> {
    let read = client.read_resource(uri).await?;
    let Some(ResourceContents::TextResourceContents { text, .. }) = read.contents.first() else {
        bail!("{uri} served no text");
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
    let (client, transcript) = ask(
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
    let event_id = created["eventId"].as_str().context("eventId")?;
    // Address the event through the template Time advertises for it.
    let advertised = client.resource_templates().await?.items;
    ensure!(
        advertised
            .iter()
            .any(|template| template.uri_template == EVENT_TEMPLATE),
        "Time does not advertise {EVENT_TEMPLATE}"
    );
    let uri = ResourceTemplateUri::new(EVENT_TEMPLATE)?.expand_scalars(&BTreeMap::from([(
        "event_id".to_owned(),
        event_id.to_owned(),
    )]))?;
    let event = read_json(&client, &uri).await?;
    ensure!(
        event["eventId"] == created["eventId"] && event["name"] == title,
        "Time serves another event at {uri}: {event}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "needs a running installation, its fake media provider and a model endpoint"]
async fn an_admin_generates_an_image_that_artifact_stores() -> Result<()> {
    let (client, transcript) = ask(
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
    let stored = read_json(&client, &ResourceUri::new(artifact)?).await?;
    ensure!(
        stored["artifact"]["mediaType"] == "image/png",
        "Artifact does not hold the generated image as image/png: {stored}"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "needs a running installation and a model endpoint"]
async fn a_member_cannot_run_a_billed_generation() -> Result<()> {
    let (_, transcript) = ask(
        "acceptance-member",
        "Generate an image of a red circle with the fake text-to-image model.",
    )
    .await?;
    ensure!(
        results(&transcript, "media__run").next().is_none(),
        "the member ran a billed generation"
    );
    Ok(())
}
