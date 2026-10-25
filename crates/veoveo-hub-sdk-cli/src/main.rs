use std::{net::SocketAddr, path::PathBuf, time::Duration};

use anyhow::{Context, Result, anyhow};
use clap::{Args, Parser, Subcommand};
use rmcp::model::JsonObject;
use serde_json::Value;
use url::Url;
use veoveo_hub_sdk::{
    HubProfileId,
    client::{CatalogListing, HubClient},
    hub::HubToolName,
};
use veoveo_hub_sdk_cli::{
    agent::{self, Limits, Step, Stop},
    grant::GrantStore,
    hub::{HubAccess, HubAuthentication, USER_AGENT},
    model::ChatModel,
    sign_in::{self, Revocation, SignIn, SignedOut},
};
use veoveo_types::{OAuthClientId, ResourceUri, ScopeName};

/// A command-line MCP client for a Veoveo installation's Hub.
#[derive(Parser)]
#[command(name = "veoveo-hub-sdk-cli", version)]
struct Cli {
    #[command(flatten)]
    hub: HubArgs,
    #[command(subcommand)]
    command: Command,
}

#[derive(Args)]
struct HubArgs {
    /// The Hub's public origin, as tokens name it.
    #[arg(
        long,
        env = "VEOVEO_CLI_HUB",
        default_value = "http://vv-hub.internal:8080",
        global = true
    )]
    hub: Url,
    /// The address the Hub's host reaches, such as 127.0.0.1:8080 on a host where a
    /// local installation publishes its edge.
    #[arg(long, env = "VEOVEO_CLI_CONNECT", global = true)]
    connect: Option<SocketAddr>,
    /// The directory holding `<caller>.pem` and `<caller>.jwks.json`.
    #[arg(long, env = "VEOVEO_CLI_KEY_DIRECTORY", global = true)]
    keys: Option<PathBuf>,
    /// The directory holding the grants `login` stores (default
    /// `$XDG_CONFIG_HOME/veoveo-hub-sdk-cli/grants`, or `~/.config/...`).
    #[arg(long, env = "VEOVEO_CLI_GRANT_DIRECTORY", global = true)]
    grants: Option<PathBuf>,
    /// Environment variable holding an existing bearer or opaque credential reference.
    #[arg(
        long,
        env = "VEOVEO_CLI_BEARER_ENV",
        conflicts_with = "keys",
        global = true
    )]
    bearer_env: Option<String>,
    /// The installation caller to act as.
    #[arg(
        long,
        env = "VEOVEO_CLI_CALLER",
        default_value = "acceptance-member",
        global = true
    )]
    caller: String,
    /// The Hub profile whose MCP endpoint, `{hub}/mcp/{profile}`, the caller uses.
    #[arg(
        long,
        env = "VEOVEO_CLI_PROFILE",
        default_value = "local",
        global = true
    )]
    profile: String,
    #[arg(long, default_value = "mcp", global = true)]
    scope: String,
    /// Print JSON instead of text.
    #[arg(long, global = true)]
    json: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Sign in through the browser and store the grant the other commands use when
    /// neither --keys nor --bearer-env is given.
    Login(LoginArgs),
    /// Revoke the stored grant at the Hub and delete it.
    Logout,
    #[command(flatten)]
    Hub(HubCommand),
}

/// The commands that act as the caller through the Hub.
#[derive(Subcommand)]
enum HubCommand {
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
struct LoginArgs {
    /// The public OAuth client the installation registers for the redirect URI.
    #[arg(long, env = "VEOVEO_CLI_CLIENT", default_value = "veoveo-hub-sdk-cli")]
    client: String,
    /// The client's registered loopback redirect; the CLI listens on its address.
    #[arg(
        long,
        env = "VEOVEO_CLI_REDIRECT_URI",
        default_value = "http://127.0.0.1:3211/callback"
    )]
    redirect_uri: Url,
    /// Print the sign-in address instead of opening a browser.
    #[arg(long)]
    no_browser: bool,
    /// How long to wait for the browser to return.
    #[arg(long, default_value_t = 300)]
    timeout_seconds: u64,
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
    cli.hub.admits(&cli.command)?;
    let json = cli.hub.json;
    match cli.command {
        Command::Login(args) => login(&cli.hub, args).await,
        Command::Logout => logout(&cli.hub).await,
        Command::Hub(command) => {
            let client = cli.hub.access()?.client().await?;
            run(&client, command, json).await
        }
    }
}

impl HubArgs {
    /// Refuses, before anything is contacted, the commands that rest on credentials of
    /// the CLI's own when a trusted runtime, such as a Computer, supplies the caller's
    /// bearer: that runtime signs the caller in, renews and revokes the credential and
    /// runs its own agent, so the CLI has no grant to store or revoke and no chat model.
    fn admits(&self, command: &Command) -> Result<()> {
        let Some(variable) = &self.bearer_env else {
            return Ok(());
        };
        let (name, reason) = match command {
            Command::Login(_) => (
                "login",
                "that runtime signs the caller in and renews the credential",
            ),
            Command::Logout => ("logout", "that runtime holds and revokes the credential"),
            Command::Hub(HubCommand::Ask(_)) => (
                "ask",
                "that runtime runs its own agent and gives the CLI no chat model; use `tools` and `call`",
            ),
            Command::Hub(_) => return Ok(()),
        };
        Err(anyhow!(
            "`{name}` does not apply here: the Hub credential comes from {variable} \
             (--bearer-env or VEOVEO_CLI_BEARER_ENV), which a trusted runtime such as a \
             Computer supplies, and {reason}"
        ))
    }

    fn access(&self) -> Result<HubAccess> {
        let authentication = match (&self.bearer_env, &self.keys) {
            (Some(variable), _) => HubAuthentication::BearerFromEnvironment {
                variable: variable.clone(),
            },
            (None, Some(keys)) => HubAuthentication::ClientCredentials {
                keys: keys.clone(),
                caller: OAuthClientId::parse(&self.caller)?,
                scope: ScopeName::parse(&self.scope)?,
            },
            (None, None) => HubAuthentication::StoredGrant {
                grants: self.grants()?,
            },
        };
        self.with(authentication)
    }

    /// Access through the stored grant, which `login` and `logout` act on whatever
    /// --keys says; with --bearer-env they do not run.
    fn grant_access(&self) -> Result<(HubAccess, GrantStore)> {
        let grants = self.grants()?;
        let access = self.with(HubAuthentication::StoredGrant {
            grants: grants.clone(),
        })?;
        Ok((access, grants))
    }

    fn with(&self, authentication: HubAuthentication) -> Result<HubAccess> {
        Ok(HubAccess {
            hub: self.hub.clone(),
            connect: self.connect,
            authentication,
            profile: HubProfileId::parse(&self.profile)?,
        })
    }

    fn grants(&self) -> Result<GrantStore> {
        let directory = match &self.grants {
            Some(directory) => directory.clone(),
            None => GrantStore::default_directory(
                std::env::var_os("XDG_CONFIG_HOME"),
                std::env::var_os("HOME"),
            )?,
        };
        Ok(GrantStore::new(directory))
    }
}

async fn login(hub: &HubArgs, args: LoginArgs) -> Result<()> {
    let (access, grants) = hub.grant_access()?;
    let request = SignIn {
        client: OAuthClientId::parse(&args.client)?,
        redirect_uri: args.redirect_uri,
        scope: ScopeName::parse(&hub.scope)?,
        timeout: Duration::from_secs(args.timeout_seconds),
    };
    let signed_in = sign_in::sign_in(&access, &grants, &request, |address| {
        if args.no_browser || !open_browser(address) {
            eprintln!("Open this address in a browser to sign in:\n\n  {address}\n");
        } else {
            eprintln!(
                "Signing in through the browser. If no window opened, open:\n\n  {address}\n"
            );
        }
    })
    .await?;
    let origin = access.hub.origin().ascii_serialization();
    println!(
        "Signed in to {origin} profile {} as client {}. The grant is in {}.",
        access.profile,
        request.client,
        signed_in.path.display()
    );
    match signed_in.refresh_token_expires_at {
        Some(expiry) => println!(
            "Its access token renews until {}.",
            expiry.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
        ),
        None => println!(
            "The Hub issued no refresh token; the sign-in ends at {}.",
            signed_in
                .access_token_expires_at
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
        ),
    }
    Ok(())
}

/// Opens `address` in the person's browser through the platform's opener. The address
/// carries the PKCE challenge and state, never a token.
fn open_browser(address: &Url) -> bool {
    let opener = if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(opener)
        .arg(address.as_str())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .is_ok()
}

async fn logout(hub: &HubArgs) -> Result<()> {
    let (access, grants) = hub.grant_access()?;
    let origin = access.hub.origin().ascii_serialization();
    match sign_in::sign_out(&access, &grants).await? {
        SignedOut::NotSignedIn => {
            println!(
                "There is no sign-in to {origin} for profile {}.",
                access.profile
            );
        }
        SignedOut::Deleted { path, revocation } => {
            println!(
                "Signed out of {origin} profile {}: deleted {}.",
                access.profile,
                path.display()
            );
            match revocation {
                Revocation::Revoked => println!("The Hub revoked the grant."),
                Revocation::Unavailable => {}
                Revocation::Failed(reason) => eprintln!(
                    "The Hub did not confirm the revocation ({reason}); the deleted refresh token stays valid until it expires."
                ),
            }
        }
    }
    Ok(())
}

async fn run(client: &HubClient, command: HubCommand, json: bool) -> Result<()> {
    match command {
        HubCommand::Tools => {
            let tools = listed(client.tools().await?);
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
        HubCommand::Resources => {
            let resources = listed(client.resources().await?);
            let templates = listed(client.resource_templates().await?);
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
        HubCommand::Prompts => {
            let prompts = listed(client.prompts().await?);
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
        HubCommand::Read { uri } => {
            print_json(&client.read_resource(&ResourceUri::new(uri)?).await?)
        }
        HubCommand::Call {
            tool,
            arguments,
            timeout_seconds,
        } => {
            let arguments: JsonObject =
                serde_json::from_str(&arguments).context("the arguments must be a JSON object")?;
            let result = client
                .call_tool(HubToolName::parse(tool.as_str())?, arguments)
                .timeout(Duration::from_secs(timeout_seconds))
                .await?;
            let value = agent::result_value(&result);
            print_json(&value)?;
            if result.is_error == Some(true) {
                return Err(anyhow!("`{tool}` returned an error result"));
            }
            Ok(())
        }
        HubCommand::Ask(args) => ask(client, args, json).await,
    }
}

/// A listing's items, after telling the reader which servers it misses.
fn listed<T>(listing: CatalogListing<T>) -> Vec<T> {
    for failure in &listing.degradation.failures {
        eprintln!(
            "the Hub could not list the {:?} of `{}`: {:?}",
            failure.surface, failure.server, failure.code
        );
    }
    listing.items
}

async fn ask(client: &HubClient, args: AskArgs, json: bool) -> Result<()> {
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
        http: reqwest::Client::builder().user_agent(USER_AGENT).build()?,
    };
    let limits = Limits {
        max_turns: args.max_turns,
        tool_deadline: Duration::from_secs(args.tool_timeout_seconds),
    };
    let transcript = agent::ask(client, &model, &args.request, limits, |step: &Step| {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn environment_bearer_needs_no_key_or_client_credentials_inputs() {
        let cli = Cli::try_parse_from([
            "veoveo-hub-sdk-cli",
            "--bearer-env",
            "HUB_TOKEN",
            "--caller",
            "",
            "--scope",
            "",
            "tools",
        ])
        .unwrap();
        assert!(matches!(cli.hub.access().unwrap().authentication,
            HubAuthentication::BearerFromEnvironment { variable } if variable == "HUB_TOKEN"));
    }

    #[test]
    fn key_and_environment_modes_are_exclusive() {
        assert!(
            Cli::try_parse_from([
                "veoveo-hub-sdk-cli",
                "--bearer-env",
                "HUB_TOKEN",
                "--keys",
                "/keys",
                "tools",
            ])
            .is_err()
        );
    }

    #[test]
    fn without_keys_or_an_environment_bearer_commands_use_the_stored_grant() {
        let cli =
            Cli::try_parse_from(["veoveo-hub-sdk-cli", "--grants", "/grants", "tools"]).unwrap();
        assert!(matches!(cli.hub.access().unwrap().authentication,
            HubAuthentication::StoredGrant { grants } if grants.directory() == std::path::Path::new("/grants")));
        let cli = Cli::try_parse_from([
            "veoveo-hub-sdk-cli",
            "--grants",
            "/grants",
            "--keys",
            "/keys",
            "tools",
        ])
        .unwrap();
        assert!(matches!(
            cli.hub.access().unwrap().authentication,
            HubAuthentication::ClientCredentials { .. }
        ));
    }

    #[test]
    fn a_runtime_bearer_refuses_the_commands_that_need_the_clis_own_credentials() {
        for command in [&["login"][..], &["logout"], &["ask", "What time is it?"]] {
            let cli = Cli::try_parse_from(
                [
                    "veoveo-hub-sdk-cli",
                    "--bearer-env",
                    "COMPUTER_HUB_REFERENCE",
                ]
                .iter()
                .chain(command),
            )
            .unwrap();
            let error = cli.hub.admits(&cli.command).unwrap_err().to_string();
            assert!(
                error.starts_with(&format!("`{}` does not apply here", command[0]))
                    && error.contains("COMPUTER_HUB_REFERENCE"),
                "{error}"
            );
        }
        for command in [
            &["tools"][..],
            &["call", "console__computer"],
            &["read", "x://y"],
        ] {
            let cli = Cli::try_parse_from(
                [
                    "veoveo-hub-sdk-cli",
                    "--bearer-env",
                    "COMPUTER_HUB_REFERENCE",
                ]
                .iter()
                .chain(command),
            )
            .unwrap();
            cli.hub.admits(&cli.command).unwrap();
        }
        let cli =
            Cli::try_parse_from(["veoveo-hub-sdk-cli", "--grants", "/grants", "login"]).unwrap();
        cli.hub.admits(&cli.command).unwrap();
    }

    #[test]
    fn the_profile_names_the_mcp_endpoint() {
        let cli = Cli::try_parse_from([
            "veoveo-hub-sdk-cli",
            "--hub",
            "https://veoveo.example.com",
            "--bearer-env",
            "COMPUTER_HUB_REFERENCE",
            "--profile",
            "computer",
            "tools",
        ])
        .unwrap();
        assert_eq!(
            cli.hub.access().unwrap().endpoint().unwrap().as_str(),
            "https://veoveo.example.com/mcp/computer"
        );
    }

    #[test]
    fn login_defaults_to_the_registered_loopback_client() {
        let cli = Cli::try_parse_from(["veoveo-hub-sdk-cli", "login"]).unwrap();
        let Command::Login(args) = cli.command else {
            panic!("login parses as login");
        };
        assert_eq!(args.client, "veoveo-hub-sdk-cli");
        assert_eq!(args.redirect_uri.as_str(), "http://127.0.0.1:3211/callback");
    }
}
