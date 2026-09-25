//! The command line: dispatch from the parsed arguments to the Gmail, mailbox and message
//! subcommands, and the loopback server.

use crate::{
    db::Database, error::AppError, onboarding, service::AppState, skarbiec::SkarbiecResolver,
};
use axum::http::StatusCode;
use serde_json::json;
use std::path::PathBuf;

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
const LOCAL_CLI_ORGANIZATION: &str = "legacy-local";
/// While the browser authorizes Gmail, the flow is re-read four times a second.
const AUTHORIZATION_POLL_MILLIS: u64 = 250;
/// A one-shot CLI command never polls; the interval only has to satisfy the service's bounds.
const CLI_POLL_INTERVAL_SECONDS: u64 = 60;

pub async fn run(cli: Cli) -> Result<(), AppError> {
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
    let database_path = match cli.database {
        Some(path) => path,
        None => default_database_path()?,
    };
    let database = Database::open(database_path)?;
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
            let status = state()?.status(LOCAL_CLI_ORGANIZATION).await?;
            print_json(&status)
        }
        Command::Mailbox { command } => run_mailbox(state()?, command).await,
        Command::Gmail { command } => match command {
            GmailCommand::Authorize {
                skarbiec_item,
                bind,
            } => authorize_gmail(database, resolver, skarbiec_item, bind).await,
            GmailCommand::Connection { email } => print_json(
                &state()?
                    .gmail_connection_readiness(LOCAL_CLI_ORGANIZATION, email.as_deref())
                    .await?,
            ),
            GmailCommand::Delegate {
                email,
                display_name,
            } => {
                let state = state()?;
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
                let state = state()?;
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
        Command::Message { command } => run_message(state()?, command).await,
        Command::Sync { mailbox } => {
            let state = state()?;
            match mailbox {
                Some(id) => print_json(&state.sync_mailbox(LOCAL_CLI_ORGANIZATION, id).await?),
                None => print_json(&state.sync_all(LOCAL_CLI_ORGANIZATION).await?),
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

pub(super) fn print_json(value: &impl serde::Serialize) -> Result<(), AppError> {
    let output = serde_json::to_string_pretty(value)
        .map_err(|_| AppError::internal("result could not be encoded as JSON"))?;
    println!("{output}");
    Ok(())
}

/// The database under the home directory when `--database` is not given. Without a home
/// directory there is no default, and that is refused rather than a file in the working
/// directory.
fn default_database_path() -> Result<PathBuf, AppError> {
    let home = std::env::var_os("HOME").ok_or_else(|| {
        AppError::invalid(
            "DATABASE_PATH_REQUIRED",
            "HOME is not set, so there is no default database path; pass --database <PATH>",
        )
    })?;
    Ok(PathBuf::from(home).join(".local/share/skrzynka/skrzynka.db"))
}
