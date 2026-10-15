use std::{net::SocketAddr, path::PathBuf, time::Duration};

use anyhow::{Context, Result, anyhow};
use clap::{Args, Parser, Subcommand};
use rmcp::model::JsonObject;
use serde_json::Value;
use url::Url;
use veoveo_sdk_rust_cli::{
    agent::{self, Limits, Step, Stop},
    gateway::GatewayAccess,
    mcp::Session,
    model::ChatModel,
};
use veoveo_types::{OAuthClientId, ScopeName};

/// A command-line MCP client for a Veoveo installation's Gateway.
#[derive(Parser)]
#[command(name = "veoveo-sdk-rust-cli", version)]
struct Cli {
    #[command(flatten)]
    gateway: GatewayArgs,
    #[command(subcommand)]
    command: Command,
}

#[derive(Args)]
struct GatewayArgs {
    /// The Gateway's public origin, as tokens name it.
    #[arg(
        long,
        env = "VEOVEO_CLI_GATEWAY",
        default_value = "http://vv-gateway.internal:8080",
        global = true
    )]
    gateway: Url,
    /// The address the Gateway's host reaches, such as 127.0.0.1:8080 on a host where a
    /// local installation publishes its edge.
    #[arg(long, env = "VEOVEO_CLI_CONNECT", global = true)]
    connect: Option<SocketAddr>,
    /// The directory holding `<caller>.pem` and `<caller>.jwks.json`.
    #[arg(long, env = "VEOVEO_CLI_KEY_DIRECTORY", global = true)]
    keys: Option<PathBuf>,
    /// The installation caller to act as.
    #[arg(
        long,
        env = "VEOVEO_CLI_CALLER",
        default_value = "acceptance-member",
        global = true
    )]
    caller: String,
    #[arg(long, default_value = "local", global = true)]
    profile: String,
    #[arg(long, default_value = "mcp", global = true)]
    scope: String,
    /// Print JSON instead of text.
    #[arg(long, global = true)]
    json: bool,
}

#[derive(Subcommand)]
enum Command {
    /// List the tools the caller sees, with their input schemas.
    Tools,
    /// List the resources and resource templates the caller sees.
    Resources,
    /// List the prompts the caller sees.
    Prompts,
    /// Read one resource.
    Read { uri: String },
    /// Call one tool and print its result; a Task-backed tool runs to its end.
    Call {
        tool: String,
        /// The arguments as a JSON object.
        #[arg(default_value = "{}")]
        arguments: String,
        #[arg(long, default_value_t = 600)]
        timeout_seconds: u64,
    },
    /// Let a chat model work on a request with the caller's tools.
    Ask(AskArgs),
}

#[derive(Args)]
struct AskArgs {
    request: String,
    /// The OpenAI-compatible API's base URL.
    #[arg(
        long,
        env = "VEOVEO_CLI_MODEL_URL",
        default_value = "https://router.huggingface.co/v1"
    )]
    model_url: Url,
    #[arg(
        long,
        env = "VEOVEO_CLI_MODEL",
        default_value = "XiaomiMiMo/MiMo-V2.6-Pro-RL:novita"
    )]
    model: String,
    /// The environment variable holding the model endpoint's token.
    #[arg(long, default_value = "HF_TOKEN")]
    model_token_env: String,
    /// A dotenv file to read the token variable from when the environment lacks it.
    #[arg(long)]
    env_file: Option<PathBuf>,
    #[arg(long, default_value_t = 4096)]
    max_tokens: u32,
    #[arg(long, default_value_t = 16)]
    max_turns: usize,
    #[arg(long, default_value_t = 600)]
    tool_timeout_seconds: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let cli = Cli::parse();
    let json = cli.gateway.json;
    let mut session = Session::open(cli.gateway.access()?).await?;
    let outcome = run(&mut session, cli.command, json).await;
    let _ = session.close().await;
    outcome
}

impl GatewayArgs {
    fn access(&self) -> Result<GatewayAccess> {
        Ok(GatewayAccess {
            issuer: self.gateway.clone(),
            connect: self.connect,
            keys: self.keys.clone().context(
                "name the callers' key directory with --keys or VEOVEO_CLI_KEY_DIRECTORY",
            )?,
            caller: OAuthClientId::parse(&self.caller)?,
            profile: self.profile.clone(),
            scope: ScopeName::parse(&self.scope)?,
        })
    }
}

async fn run(session: &mut Session, command: Command, json: bool) -> Result<()> {
    match command {
        Command::Tools => {
            let tools = session.tools().await?;
            if json {
                return print_json(&tools);
            }
            for tool in tools {
                println!(
                    "{}\n  {}",
                    tool.name,
                    tool.description.as_deref().unwrap_or("")
                );
                if let Some(annotations) = &tool.annotations {
                    println!("  annotations: {}", serde_json::to_string(annotations)?);
                }
                println!("  input: {}", Value::Object((*tool.input_schema).clone()));
            }
            Ok(())
        }
        Command::Resources => {
            let resources = session.resources().await?;
            let templates = session.resource_templates().await?;
            if json {
                return print_json(
                    &serde_json::json!({ "resources": resources, "templates": templates }),
                );
            }
            for resource in resources {
                println!(
                    "{} — {}",
                    resource.uri,
                    resource.description.as_deref().unwrap_or("")
                );
            }
            for template in templates {
                println!(
                    "{} — {}",
                    template.uri_template,
                    template.description.as_deref().unwrap_or("")
                );
            }
            Ok(())
        }
        Command::Prompts => {
            let prompts = session.prompts().await?;
            if json {
                return print_json(&prompts);
            }
            for prompt in prompts {
                println!(
                    "{} — {}",
                    prompt.name,
                    prompt.description.as_deref().unwrap_or("")
                );
            }
            Ok(())
        }
        Command::Read { uri } => print_json(&session.read_resource(&uri).await?),
        Command::Call {
            tool,
            arguments,
            timeout_seconds,
        } => {
            let arguments: JsonObject =
                serde_json::from_str(&arguments).context("the arguments must be a JSON object")?;
            let result = session
                .call(&tool, arguments, Duration::from_secs(timeout_seconds))
                .await?;
            let value = agent::result_value(&result);
            print_json(&value)?;
            if result.is_error == Some(true) {
                return Err(anyhow!("`{tool}` returned an error result"));
            }
            Ok(())
        }
        Command::Ask(args) => ask(session, args, json).await,
    }
}

async fn ask(session: &mut Session, args: AskArgs, json: bool) -> Result<()> {
    let token = model_token(&args.model_token_env, args.env_file.as_deref())?;
    let mut base = args.model_url.clone();
    if !base.path().ends_with('/') {
        base.set_path(&format!("{}/", base.path()));
    }
    let model = ChatModel {
        base,
        model: args.model.clone(),
        token,
        max_tokens: args.max_tokens,
        http: reqwest::Client::new(),
    };
    let limits = Limits {
        max_turns: args.max_turns,
        tool_deadline: Duration::from_secs(args.tool_timeout_seconds),
    };
    let transcript = agent::ask(session, &model, &args.request, limits, |step: &Step| {
        if !json {
            println!("→ {} {}", step.tool, step.arguments);
            let marker = if step.is_error { "✗" } else { "←" };
            let result = step.result.to_string();
            let shown: String = result.chars().take(1_200).collect();
            let more = if result.chars().count() > 1_200 {
                " …"
            } else {
                ""
            };
            println!("{marker} {shown}{more}");
        }
    })
    .await?;
    if json {
        return print_json(&transcript);
    }
    match transcript.stop {
        Stop::Answered => println!(
            "\n{}",
            transcript.answer.as_deref().unwrap_or("(no answer)")
        ),
        Stop::TurnLimit => println!("\nstopped after {} turns without an answer", args.max_turns),
        Stop::Length => println!("\nthe model ran out of output tokens; raise --max-tokens"),
    }
    Ok(())
}

/// The model endpoint's token, from the environment or else from `env_file`.
fn model_token(variable: &str, env_file: Option<&std::path::Path>) -> Result<String> {
    if let Ok(token) = std::env::var(variable)
        && !token.is_empty()
    {
        return Ok(token);
    }
    let path = env_file
        .with_context(|| format!("set {variable} or name a file holding it with --env-file"))?;
    let text = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    text.lines()
        .find_map(|line| line.strip_prefix(&format!("{variable}=")))
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .with_context(|| format!("{} holds no {variable}", path.display()))
}

fn print_json(value: &impl serde::Serialize) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}
