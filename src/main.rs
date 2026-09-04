mod api;
mod auth;
mod db;
mod error;
mod gmail;
mod mail;
mod models;
mod onboarding;
mod service;
mod skarbiec;

use crate::{
    db::Database,
    error::AppError,
    gmail::StartGmailOAuthRequest,
    models::{CreateMailboxRequest, CreateOutboundRequest, CreateReplyRequest, SmtpSecurity},
    service::AppState,
    skarbiec::SkarbiecResolver,
};
use axum::http::StatusCode;
use clap::{Args, Parser, Subcommand, ValueEnum};
use serde_json::json;
use std::{
    io::{self, Read},
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
    Onboarding {
        #[arg(long)]
        reset: bool,
    },
    Version,
    Mailbox {
        #[command(subcommand)]
        command: MailboxCommand,
    },
    Gmail {
        #[command(subcommand)]
        command: GmailCommand,
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
enum GmailCommand {
    /// Report whether the delegated-mail service account is configured, and
    /// which client ID the Workspace admin must grant.
    Delegation,
    /// Authorize one Google identity through the loopback OAuth callback.
    Authorize {
        #[arg(long)]
        skarbiec_item: String,
        #[arg(long, default_value = "127.0.0.1:8790")]
        bind: SocketAddr,
    },
    /// Connect one Workspace mailbox through domain-wide delegation.
    Delegate {
        #[arg(long)]
        email: String,
        #[arg(long)]
        display_name: Option<String>,
    },
    /// Connect one Gmail account using an app-specific password read from stdin.
    AppPassword {
        #[arg(long)]
        email: String,
        #[arg(long)]
        display_name: Option<String>,
    },
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
    Send {
        #[arg(long)]
        mailbox: String,
        #[arg(long = "to", required = true)]
        to: Vec<String>,
        #[arg(long = "cc")]
        cc: Vec<String>,
        #[arg(long)]
        subject: String,
        #[arg(long)]
        body_file: PathBuf,
        #[arg(long)]
        idempotency_key: Option<String>,
    },
    Outbound {
        #[arg(long)]
        mailbox: Option<String>,
        #[arg(long, default_value_t = 100)]
        limit: u32,
        #[arg(long, default_value_t = 0)]
        offset: u32,
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
    if let Command::Onboarding { reset } = &cli.command {
        onboarding::run(*reset)?;
        return Ok(());
    }
    let database_path = cli.database.unwrap_or_else(default_database_path);
    let database = Database::open(database_path)?;
    let resolver = SkarbiecResolver::new(cli.skarbiec_bin);
    match cli.command {
        Command::Serve(args) => serve(database, resolver, args).await,
        Command::Status => {
            let state = AppState::new(database, resolver, 60, DEFAULT_CALLBACK_BASE_URL)?;
            let status = state.status(LOCAL_CLI_ORGANIZATION).await?;
            print_json(&status)?;
            onboarding::record_status_report_rendered()
        }
        Command::Mailbox { command } => {
            let state = AppState::new(database, resolver, 60, DEFAULT_CALLBACK_BASE_URL)?;
            run_mailbox(state, command).await
        }
        Command::Gmail { command } => match command {
            GmailCommand::Authorize {
                skarbiec_item,
                bind,
            } => authorize_gmail(database, resolver, skarbiec_item, bind).await,
            GmailCommand::Delegation => {
                let state = AppState::new(database, resolver, 60, DEFAULT_CALLBACK_BASE_URL)?;
                print_json(&state.gmail_delegation_status().await)
            }
            GmailCommand::Delegate {
                email,
                display_name,
            } => {
                let state = AppState::new(database, resolver, 60, DEFAULT_CALLBACK_BASE_URL)?;
                print_json(
                    &state
                        .connect_gmail_delegated(LOCAL_CLI_ORGANIZATION, &email, display_name)
                        .await?,
                )
            }
            GmailCommand::AppPassword {
                email,
                display_name,
            } => {
                let password = read_gmail_app_password()?;
                let state = AppState::new(database, resolver, 60, DEFAULT_CALLBACK_BASE_URL)?;
                print_json(
                    &state
                        .connect_gmail_app_password(
                            LOCAL_CLI_ORGANIZATION,
                            &email,
                            &password,
                            display_name,
                        )
                        .await?,
                )
            }
        },
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
        Command::Onboarding { .. } => unreachable!(),
    }
}

async fn authorize_gmail(
    database: Database,
    resolver: SkarbiecResolver,
    skarbiec_item: String,
    bind: SocketAddr,
) -> Result<(), AppError> {
    if !bind.ip().is_loopback() {
        return Err(AppError::new(
            StatusCode::BAD_REQUEST,
            "NON_LOOPBACK_BIND_REFUSED",
            "Skrzynka serves OAuth callbacks only on loopback addresses",
            false,
        ));
    }
    let callback_base_url = format!("http://{bind}");
    let state = AppState::new(database, resolver, 60, &callback_base_url)?;
    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .map_err(|_| AppError::internal("loopback OAuth callback address could not be bound"))?;
    let flow = state
        .start_gmail_oauth(
            LOCAL_CLI_ORGANIZATION,
            StartGmailOAuthRequest {
                skarbiec_item_id: skarbiec_item,
            },
        )
        .await?;
    print_json(&flow)?;

    let callback_state = state.clone();
    let server =
        tokio::spawn(async move { axum::serve(listener, api::router(callback_state)).await });
    loop {
        let status = state
            .gmail_oauth_status(LOCAL_CLI_ORGANIZATION, flow.flow_id)
            .await?;
        if status.status == "completed" {
            server.abort();
            print_json(&status)?;
            return Ok(());
        }
        if status.status == "failed" {
            server.abort();
            let error = status.error.as_ref();
            // A flow that expired without a callback is the shape an
            // unregistered redirect URI takes here: Google refuses inside the
            // browser, nothing ever reaches this listener, and the ten-minute
            // lifetime runs out saying only that it expired. Ask Google why
            // before reporting that, so the operator gets the cause instead of
            // the symptom.
            if error.map(|error| error.code) == Some("GMAIL_OAUTH_FLOW_EXPIRED") {
                if gmail::diagnose_authorization(&flow.authorization_url)
                    .await
                    .as_deref()
                    == Some("redirect_uri_mismatch")
                {
                    if let Some((client_id, redirect_uri)) =
                        gmail::authorization_operands(&flow.authorization_url)
                    {
                        return Err(gmail::redirect_not_registered(&client_id, &redirect_uri));
                    }
                }
            }
            return Err(AppError::dependency(
                "GMAIL_OAUTH_FAILED",
                error
                    .map(|error| format!("{}: {}", error.code, error.message))
                    .unwrap_or_else(|| "Gmail authorization failed".to_string()),
                error.map(|error| error.retryable).unwrap_or(false),
            ));
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
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
            let body = read_body_file(&body_file, "REPLY_FILE_INVALID")?;
            let request = CreateReplyRequest {
                idempotency_key: idempotency_key.unwrap_or_else(|| Uuid::new_v4().to_string()),
                body,
            };
            print_json(&state.reply(LOCAL_CLI_ORGANIZATION, id, request).await?)
        }
        MessageCommand::Send {
            mailbox,
            to,
            cc,
            subject,
            body_file,
            idempotency_key,
        } => {
            let mailbox = state.resolve_mailbox(LOCAL_CLI_ORGANIZATION, &mailbox)?;
            let body = read_body_file(&body_file, "OUTBOUND_FILE_INVALID")?;
            let request = CreateOutboundRequest {
                idempotency_key: idempotency_key.unwrap_or_else(|| Uuid::new_v4().to_string()),
                to,
                cc,
                subject,
                body,
            };
            print_json(
                &state
                    .send_outbound(LOCAL_CLI_ORGANIZATION, mailbox.id, request)
                    .await?,
            )
        }
        MessageCommand::Outbound {
            mailbox,
            limit,
            offset,
        } => {
            let mailbox_id = match mailbox {
                Some(selector) => {
                    Some(state.resolve_mailbox(LOCAL_CLI_ORGANIZATION, &selector)?.id)
                }
                None => None,
            };
            print_json(&state.list_outbound(LOCAL_CLI_ORGANIZATION, mailbox_id, limit, offset)?)
        }
    }
}

fn read_gmail_app_password() -> Result<String, AppError> {
    const MAX_APP_PASSWORD_BYTES: u64 = 4 * 1024;
    let mut input = Vec::new();
    io::stdin()
        .lock()
        .take(MAX_APP_PASSWORD_BYTES + 1)
        .read_to_end(&mut input)
        .map_err(|_| {
            AppError::invalid(
                "GMAIL_APP_PASSWORD_INPUT_INVALID",
                "Google app-specific password could not be read from stdin",
            )
        })?;
    if input.len() as u64 > MAX_APP_PASSWORD_BYTES {
        return Err(AppError::invalid(
            "GMAIL_APP_PASSWORD_INPUT_INVALID",
            "Google app-specific password supplied through stdin is too long",
        ));
    }
    let input = String::from_utf8(input).map_err(|_| {
        AppError::invalid(
            "GMAIL_APP_PASSWORD_INPUT_INVALID",
            "Google app-specific password supplied through stdin must be valid UTF-8",
        )
    })?;
    let password = input
        .trim_end_matches(|character| character == '\r' || character == '\n')
        .to_string();
    if password.is_empty() {
        return Err(AppError::invalid(
            "GMAIL_APP_PASSWORD_INPUT_INVALID",
            "Google app-specific password supplied through stdin must not be empty",
        ));
    }
    Ok(password)
}

fn read_body_file(path: &Path, code: &'static str) -> Result<String, AppError> {
    let metadata = std::fs::metadata(path)
        .map_err(|_| AppError::invalid(code, "message body file could not be read"))?;
    if !metadata.is_file() || metadata.len() > 256 * 1024 {
        return Err(AppError::invalid(
            code,
            "message body file must be a regular file no larger than 256 KiB",
        ));
    }
    std::fs::read_to_string(path)
        .map_err(|_| AppError::invalid(code, "message body file must contain valid UTF-8 text"))
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
