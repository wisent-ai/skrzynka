mod api;
mod auth;
mod db;
mod error;
mod gmail;
mod mail;
mod models;
mod service;
mod skarbiec;

use crate::{
    db::Database,
    error::AppError,
    models::{CreateMailboxRequest, CreateReplyRequest, SmtpSecurity},
    service::AppState,
    skarbiec::SkarbiecResolver,
};
use axum::http::StatusCode;
use clap::{Args, Parser, Subcommand, ValueEnum};
use serde_json::json;
use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
};
use tracing_subscriber::EnvFilter;
use uuid::Uuid;

const DEFAULT_CALLBACK_BASE_URL: &str = "http://127.0.0.1:8788";
const LOCAL_CLI_ORGANIZATION: &str = "legacy-local";

#[derive(Parser)]
#[command(
    name = "skrzynka",
    version,
    about = "Receive and reply across multiple mailboxes without moving credentials out of Skarbiec",
    after_help = "Safe first result: skrzynka serve; add one Skarbiec mailbox; skrzynka sync; skrzynka message list"
)]
struct Cli {
    #[arg(long, global = true, value_name = "PATH")]
    database: Option<PathBuf>,
    #[arg(long, global = true, default_value = "skarbiec", value_name = "PATH")]
    skarbiec_bin: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Serve(ServeArgs),
    Status,
    Version,
    Mailbox {
        #[command(subcommand)]
        command: MailboxCommand,
    },
    Message {
        #[command(subcommand)]
        command: MessageCommand,
    },
    Sync {
        #[arg(long)]
        mailbox: Option<Uuid>,
    },
}

#[derive(Args)]
struct ServeArgs {
    #[arg(long, default_value = "127.0.0.1:8788")]
    bind: SocketAddr,
    #[arg(long, default_value_t = 60)]
    poll_seconds: u64,
}

#[derive(Subcommand)]
enum MailboxCommand {
    Add(AddMailboxArgs),
    List,
    Show {
        id: Uuid,
    },
    Enable {
        id: Uuid,
    },
    Disable {
        id: Uuid,
    },
    Remove {
        id: Uuid,
        #[arg(long)]
        confirm: bool,
    },
}

#[derive(Args)]
struct AddMailboxArgs {
    #[arg(long)]
    skarbiec_item: String,
    #[arg(long)]
    display_name: Option<String>,
    #[arg(long)]
    email: Option<String>,
    #[arg(long)]
    imap_host: Option<String>,
    #[arg(long)]
    imap_port: Option<u16>,
    #[arg(long)]
    smtp_host: Option<String>,
    #[arg(long)]
    smtp_port: Option<u16>,
    #[arg(long, value_enum)]
    smtp_security: Option<CliSmtpSecurity>,
    #[arg(long)]
    poll_seconds: Option<u64>,
}

#[derive(Clone, Copy, ValueEnum)]
enum CliSmtpSecurity {
    Starttls,
    Tls,
}

impl From<CliSmtpSecurity> for SmtpSecurity {
    fn from(value: CliSmtpSecurity) -> Self {
        match value {
            CliSmtpSecurity::Starttls => Self::Starttls,
            CliSmtpSecurity::Tls => Self::Tls,
        }
    }
}

#[derive(Subcommand)]
enum MessageCommand {
    List {
        #[arg(long)]
        mailbox: Option<Uuid>,
        #[arg(long, default_value_t = 100)]
        limit: u32,
        #[arg(long, default_value_t = 0)]
        offset: u32,
    },
    Show {
        id: Uuid,
    },
    Reply {
        id: Uuid,
        #[arg(long)]
        body_file: PathBuf,
        #[arg(long)]
        idempotency_key: Option<String>,
    },
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();
    if let Err(error) = run(Cli::parse()).await {
        eprintln!(
            "{}",
            serde_json::to_string(&json!({
                "error": {
                    "code": error.code,
                    "message": error.message,
                    "retryable": error.retryable,
                }
            }))
            .unwrap_or_else(|_| "{\"error\":{\"code\":\"INTERNAL_ERROR\"}}".to_string())
        );
        std::process::exit(1);
    }
}

async fn run(cli: Cli) -> Result<(), AppError> {
    if matches!(cli.command, Command::Version) {
        print_json(&json!({
            "product": "skrzynka",
            "version": env!("CARGO_PKG_VERSION"),
            "source": option_env!("SKRZYNKA_SOURCE_REVISION").unwrap_or("source-build"),
        }))?;
        return Ok(());
    }
    let database_path = cli.database.unwrap_or_else(default_database_path);
    let database = Database::open(database_path)?;
    let resolver = SkarbiecResolver::new(cli.skarbiec_bin);
    match cli.command {
        Command::Serve(args) => serve(database, resolver, args).await,
        Command::Status => {
            let state = AppState::new(database, resolver, 60, DEFAULT_CALLBACK_BASE_URL)?;
            print_json(&state.status(LOCAL_CLI_ORGANIZATION).await?)
        }
        Command::Mailbox { command } => {
            let state = AppState::new(database, resolver, 60, DEFAULT_CALLBACK_BASE_URL)?;
            run_mailbox(state, command).await
        }
        Command::Message { command } => {
            let state = AppState::new(database, resolver, 60, DEFAULT_CALLBACK_BASE_URL)?;
            run_message(state, command).await
        }
        Command::Sync { mailbox } => {
            let state = AppState::new(database, resolver, 60, DEFAULT_CALLBACK_BASE_URL)?;
            match mailbox {
                Some(id) => print_json(&state.sync_mailbox(LOCAL_CLI_ORGANIZATION, id).await?),
                None => print_json(&state.sync_all(LOCAL_CLI_ORGANIZATION).await?),
            }
        }
        Command::Version => unreachable!(),
    }
}

async fn serve(
    database: Database,
    resolver: SkarbiecResolver,
    args: ServeArgs,
) -> Result<(), AppError> {
    if !args.bind.ip().is_loopback() {
        return Err(AppError::new(
            StatusCode::BAD_REQUEST,
            "NON_LOOPBACK_BIND_REFUSED",
            "Skrzynka serves only loopback addresses",
            false,
        ));
    }
    let callback_base_url = format!("http://{}", args.bind);
    let state = AppState::new(database, resolver, args.poll_seconds, &callback_base_url)?;
    state.clone().start_polling();
    let listener = tokio::net::TcpListener::bind(args.bind)
        .await
        .map_err(|_| AppError::internal("loopback API address could not be bound"))?;
    tracing::info!(address = %args.bind, "Skrzynka API ready");
    axum::serve(listener, api::router(state))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .map_err(|_| AppError::internal("loopback API stopped unexpectedly"))
}

async fn run_mailbox(state: AppState, command: MailboxCommand) -> Result<(), AppError> {
    match command {
        MailboxCommand::Add(args) => {
            let request = CreateMailboxRequest {
                skarbiec_item_id: args.skarbiec_item,
                display_name: args.display_name,
                email: args.email,
                imap_host: args.imap_host,
                imap_port: args.imap_port,
                smtp_host: args.smtp_host,
                smtp_port: args.smtp_port,
                smtp_security: args.smtp_security.map(Into::into),
                poll_interval_seconds: args.poll_seconds,
            };
            print_json(
                &state
                    .create_mailbox(LOCAL_CLI_ORGANIZATION, request)
                    .await?,
            )
        }
        MailboxCommand::List => print_json(&state.list_mailboxes(LOCAL_CLI_ORGANIZATION)?),
        MailboxCommand::Show { id } => print_json(&state.get_mailbox(LOCAL_CLI_ORGANIZATION, id)?),
        MailboxCommand::Enable { id } => print_json(&state.update_mailbox(
            LOCAL_CLI_ORGANIZATION,
            id,
            crate::models::UpdateMailboxRequest {
                enabled: Some(true),
                ..Default::default()
            },
        )?),
        MailboxCommand::Disable { id } => print_json(&state.update_mailbox(
            LOCAL_CLI_ORGANIZATION,
            id,
            crate::models::UpdateMailboxRequest {
                enabled: Some(false),
                ..Default::default()
            },
        )?),
        MailboxCommand::Remove { id, confirm } => {
            if !confirm {
                return Err(AppError::invalid(
                    "CONFIRMATION_REQUIRED",
                    "mailbox removal requires --confirm",
                ));
            }
            state.delete_mailbox(LOCAL_CLI_ORGANIZATION, id)?;
            print_json(&json!({ "removed": id }))
        }
    }
}

async fn run_message(state: AppState, command: MessageCommand) -> Result<(), AppError> {
    match command {
        MessageCommand::List {
            mailbox,
            limit,
            offset,
        } => print_json(&state.list_messages(LOCAL_CLI_ORGANIZATION, mailbox, limit, offset)?),
        MessageCommand::Show { id } => print_json(&state.get_message(LOCAL_CLI_ORGANIZATION, id)?),
        MessageCommand::Reply {
            id,
            body_file,
            idempotency_key,
        } => {
            let body = read_reply_body(&body_file)?;
            let request = CreateReplyRequest {
                idempotency_key: idempotency_key.unwrap_or_else(|| Uuid::new_v4().to_string()),
                body,
            };
            print_json(&state.reply(LOCAL_CLI_ORGANIZATION, id, request).await?)
        }
    }
}

fn read_reply_body(path: &Path) -> Result<String, AppError> {
    let metadata = std::fs::metadata(path).map_err(|_| {
        AppError::invalid("REPLY_FILE_INVALID", "reply body file could not be read")
    })?;
    if !metadata.is_file() || metadata.len() > 256 * 1024 {
        return Err(AppError::invalid(
            "REPLY_FILE_INVALID",
            "reply body file must be a regular file no larger than 256 KiB",
        ));
    }
    std::fs::read_to_string(path).map_err(|_| {
        AppError::invalid(
            "REPLY_FILE_INVALID",
            "reply body file must contain valid UTF-8 text",
        )
    })
}

fn print_json(value: &impl serde::Serialize) -> Result<(), AppError> {
    let output = serde_json::to_string_pretty(value)
        .map_err(|_| AppError::internal("result could not be encoded as JSON"))?;
    println!("{output}");
    Ok(())
}

fn default_database_path() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".local/share/skrzynka/skrzynka.db")
}
