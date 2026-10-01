//! The command line: dispatch from the parsed arguments to the Gmail, mailbox and message
//! subcommands, and the loopback server.

use crate::{
    db::Database, error::AppError, onboarding, service::AppState, skarbiec::SkarbiecResolver,
};
use axum::http::StatusCode;
use serde_json::{Value, json};
use std::sync::OnceLock;

mod arguments;
mod gmail;
mod mailbox;
mod message;

pub use arguments::Cli;
use arguments::{Command, GmailCommand, ServeArgs};
use gmail::{authorize_gmail, read_gmail_app_password};
use mailbox::run_mailbox;
use message::run_message;

const DEFAULT_CALLBACK_BASE_URL: &str = "http://127.0.0.1:8788";
/// While the browser authorizes Gmail, the flow is re-read four times a second.
const AUTHORIZATION_POLL_MILLIS: u64 = 250;
/// A one-shot CLI command never polls; the interval only has to satisfy the service's bounds.
const CLI_POLL_INTERVAL_SECONDS: u64 = 60;

/// Whether this invocation asked for `--text`; set once before any command runs.
static TEXT: OnceLock<bool> = OnceLock::new();

pub async fn run(cli: Cli) -> Result<(), AppError> {
    TEXT.get_or_init(|| cli.text);
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
    let organization = cli.organization.as_str();
    let database = Database::open()?;
    let resolver = SkarbiecResolver::new(cli.skarbiec_bin);
    let state = || {
        AppState::new(
            database.clone(),
            resolver.clone(),
            CLI_POLL_INTERVAL_SECONDS,
            DEFAULT_CALLBACK_BASE_URL,
        )
    };
    match cli.command {
        Command::Serve(args) => serve(database, resolver, args).await,
        Command::Status => {
            let status = state()?.status(organization).await?;
            print_json(&status)
        }
        Command::Mailbox { command } => run_mailbox(state()?, organization, command).await,
        Command::Gmail { command } => match command {
            GmailCommand::Authorize {
                skarbiec_item,
                bind,
            } => authorize_gmail(database, resolver, organization, skarbiec_item, bind).await,
            GmailCommand::Connection { email } => print_json(
                &state()?
                    .gmail_connection_readiness(organization, email.as_deref())
                    .await?,
            ),
            GmailCommand::Delegate {
                email,
                display_name,
            } => {
                let state = state()?;
                print_json(
                    &state
                        .connect_gmail_delegated(organization, &email, display_name)
                        .await?,
                )
            }
            GmailCommand::AppPassword {
                email,
                display_name,
            } => {
                let password = read_gmail_app_password()?;
                let state = state()?;
                print_json(
                    &state
                        .connect_gmail_app_password(
                            organization,
                            &email,
                            &password,
                            display_name,
                        )
                        .await?,
                )
            }
        },
        Command::Message { command } => run_message(state()?, organization, command).await,
        Command::Sync { mailbox } => {
            let state = state()?;
            match mailbox {
                Some(id) => print_json(&state.sync_mailbox(organization, id).await?),
                None => print_json(&state.sync_all(organization).await?),
            }
        }
        Command::Version => unreachable!(),
        Command::Onboarding { .. } => unreachable!(),
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
    axum::serve(listener, crate::api::router(state))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .map_err(|_| AppError::internal("loopback API stopped unexpectedly"))
}

/// Prints one result: pretty JSON for machines, or with `--text` one
/// `path: value` line per field for people, from the same value (cli.md rule 13).
pub(super) fn print_json(value: &impl serde::Serialize) -> Result<(), AppError> {
    let value = serde_json::to_value(value)
        .map_err(|_| AppError::internal("result could not be encoded as JSON"))?;
    if TEXT.get().copied().unwrap_or(false) {
        let mut lines = String::new();
        flatten("", &value, &mut lines);
        print!("{lines}");
        return Ok(());
    }
    let output = serde_json::to_string_pretty(&value)
        .map_err(|_| AppError::internal("result could not be encoded as JSON"))?;
    println!("{output}");
    Ok(())
}

fn flatten(path: &str, value: &Value, lines: &mut String) {
    match value {
        Value::Object(map) if !map.is_empty() => {
            for (key, item) in map {
                let child = if path.is_empty() { key.clone() } else { format!("{path}.{key}") };
                flatten(&child, item, lines);
            }
        }
        Value::Array(items) if !items.is_empty() => {
            for (index, item) in items.iter().enumerate() {
                flatten(&format!("{path}[{index}]"), item, lines);
            }
        }
        _ => {
            let shown = match value {
                Value::String(text) => text.clone(),
                Value::Null | Value::Object(_) | Value::Array(_) => "-".to_owned(),
                other => other.to_string(),
            };
            if !path.is_empty() {
                lines.push_str(path);
                lines.push_str(": ");
            }
            lines.push_str(&shown);
            lines.push('\n');
        }
    }
}
