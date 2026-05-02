use std::{io, time::Instant};

use anyhow::{Context, Result, anyhow};
use clap::{Args, Parser, Subcommand};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use tracing::{debug, info};
use tracing_subscriber::EnvFilter;

use crate::{
    config::AppConfig,
    daemon,
    ipc::{IpcRequest, RequestEnvelope, ResponseEnvelope},
    transport::{TransportClient, unix::UnixSocketClient},
    types::new_id,
};

#[derive(Parser)]
#[command(name = "claw", version, about = "Assistant runtime")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Daemon {
        #[command(subcommand)]
        command: DaemonCommand,
    },
    Chat(ChatArgs),
    Chats {
        #[command(subcommand)]
        command: ChatsCommand,
    },
    Providers {
        #[command(subcommand)]
        command: ProvidersCommand,
    },
}

#[derive(Subcommand)]
enum DaemonCommand {
    Run,
}

#[derive(Args)]
struct ChatArgs {
    #[arg(long)]
    new: bool,
    #[arg(long)]
    chat: Option<String>,
    #[arg(long)]
    provider: Option<String>,
    prompt: Vec<String>,
}

#[derive(Subcommand)]
enum ChatsCommand {
    List,
    Cleanup,
}

#[derive(Subcommand)]
enum ProvidersCommand {
    List,
}

pub async fn run() -> Result<()> {
    let cli = Cli::parse();
    init_logging();
    let config = AppConfig::load()?;

    match cli.command {
        Command::Daemon {
            command: DaemonCommand::Run,
        } => daemon::run(config).await?,
        Command::Chat(args) => chat_command(&config, args).await?,
        Command::Chats { command } => chats_command(&config, command).await?,
        Command::Providers { command } => providers_command(&config, command).await?,
    }

    Ok(())
}

async fn chat_command(config: &AppConfig, args: ChatArgs) -> Result<()> {
    let prompt = args.prompt.join(" ");
    if prompt.trim().is_empty() {
        return Err(anyhow!("chat prompt is required"));
    }

    if args.new {
        let provider_id = args.provider.as_deref().unwrap_or(&config.default_provider);
        info!(provider_id = %provider_id, prompt_len = prompt.len(), "starting new chat");

        let value: Value = send(
            config,
            IpcRequest::ChatNew {
                prompt,
                provider: args.provider,
            },
        )
        .await?;
        log_chat_response(&value, "new");
        print_json(&value)?;
        return Ok(());
    }

    let chat_id = args
        .chat
        .ok_or_else(|| anyhow!("pass --new or --chat <id>"))?;
    info!(chat_id = %chat_id, prompt_len = prompt.len(), "resuming chat");

    let value: Value = send(config, IpcRequest::ChatResume { chat_id, prompt }).await?;
    log_chat_response(&value, "resume");
    print_json(&value)
}

async fn chats_command(config: &AppConfig, command: ChatsCommand) -> Result<()> {
    let request = match command {
        ChatsCommand::List => IpcRequest::ChatsList,
        ChatsCommand::Cleanup => IpcRequest::ChatsCleanup,
    };
    let value: serde_json::Value = send(config, request).await?;
    print_json(&value)
}

async fn providers_command(config: &AppConfig, command: ProvidersCommand) -> Result<()> {
    let request = match command {
        ProvidersCommand::List => IpcRequest::ProvidersList,
    };
    let value: serde_json::Value = send(config, request).await?;
    print_json(&value)
}

async fn send<T: DeserializeOwned>(config: &AppConfig, request: IpcRequest) -> Result<T> {
    let client = UnixSocketClient::new(config.socket_path.clone());
    let request_kind = request.kind();
    let request_id = new_id();
    let envelope = RequestEnvelope {
        id: request_id.clone(),
        request,
    };
    let started = Instant::now();

    debug!(
        request_id = %request_id,
        request_kind = %request_kind,
        socket_path = %config.socket_path.display(),
        "sending daemon request"
    );

    let envelope: ResponseEnvelope = client
        .send(envelope)
        .await
        .with_context(|| format!("sending request to {}", config.socket_path.display()))?;
    info!(
        request_id = %request_id,
        request_kind = %request_kind,
        ok = envelope.ok,
        elapsed_ms = started.elapsed().as_millis(),
        "daemon request completed"
    );
    if !envelope.ok {
        return Err(anyhow!(
            "{}",
            envelope
                .error
                .unwrap_or_else(|| "daemon request failed".to_string())
        ));
    }

    let result = envelope.result.unwrap_or(serde_json::Value::Null);
    Ok(serde_json::from_value(result)?)
}

fn print_json(value: &impl Serialize) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

fn init_logging() {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("claw=info,warn"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(io::stderr)
        .with_target(false)
        .compact()
        .try_init();
}

fn log_chat_response(value: &Value, action: &'static str) {
    let chat_id = value
        .get("chat_id")
        .and_then(Value::as_str)
        .unwrap_or("<missing>");
    let provider_id = value
        .get("provider_id")
        .and_then(Value::as_str)
        .unwrap_or("<missing>");
    let provider_conversation_id = value
        .pointer("/provider_conversation_ref/id")
        .and_then(Value::as_str)
        .unwrap_or("<none>");
    let assistant_messages = value
        .get("assistant_messages")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let tool_results = value
        .get("tool_results")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);

    info!(
        action = %action,
        chat_id = %chat_id,
        provider_id = %provider_id,
        provider_conversation_id = %provider_conversation_id,
        assistant_messages,
        tool_results,
        "chat completed"
    );
}
