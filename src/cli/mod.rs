//! The command line: dispatch from the parsed arguments to the Gmail, mailbox and message
//! subcommands, and the loopback server.

use crate::{
    db::Database, error::AppError, onboarding, service::AppState, skarbiec::SkarbiecResolver,
};
use axum::http::StatusCode;
use serde_json::{json, Value};
use std::{net::SocketAddr, path::PathBuf, sync::OnceLock};

mod actions;
mod arguments;
mod attachment;
mod auto_replies;
mod drafts;
mod export;
mod follow_ups;
mod forward;
mod gmail;
mod listing;
mod mailbox;
mod message;
mod output;
mod reading;
mod rules;
mod sending;
mod star;
mod suppressed;
mod templates;
mod views;
mod watch;

use arguments::{AccountCommand, Command};

#[derive(clap::Parser)]
#[command(
    name = "skrzynka",
    version,
    about = "Receive and reply across multiple mailboxes without moving credentials out of Skarbiec",
    after_help = "Mailboxes are the Skarbiec items tagged skrzynka:mailbox. Safe first result: skrzynka mailbox declare --skarbiec-item <ITEM_ID>, which imports every message the INBOX holds"
)]
pub struct Cli {
    /// The organization a command acts for in the fleet database. Every
    /// command except `serve`, `version` and `onboarding` needs one; none is assumed.
    #[arg(long, global = true, value_name = "ID")]
    organization: Option<String>,
    /// The `skarbiec` executable that reads and tags mailbox items.
    #[arg(long, global = true, default_value = "skarbiec", value_name = "PATH")]
    skarbiec_bin: PathBuf,
    /// Print results as `path: value` lines for people instead of JSON.
    #[arg(long, global = true)]
    text: bool,
    #[command(subcommand)]
    command: Command,
}
use gmail::{authorize_gmail, read_gmail_app_password, MailProvider};
use mailbox::run_mailbox;
use message::run_message;

/// Whether this invocation asked for `--text`; set once before any command runs.
static TEXT: OnceLock<bool> = OnceLock::new();

pub async fn run(cli: Cli) -> Result<(), AppError> {
    TEXT.get_or_init(|| cli.text);
    if matches!(cli.command, Command::Version) {
        // A build that was not given its source revision reports itself as a
        // source build, as `skrzynka version` documents.
        let source = match option_env!("SKRZYNKA_SOURCE_REVISION") {
            Some(revision) => revision,
            None => "source-build",
        };
        print_json(&json!({
            "product": "skrzynka",
            "version": env!("CARGO_PKG_VERSION"),
            "source": source,
        }))?;
        return Ok(());
    }
    if let Command::Onboarding { reset } = &cli.command {
        onboarding::run(*reset)?;
        return Ok(());
    }
    // The star request is kept per person, not per organization, and
    // touches no database.
    if let Command::Star { command } = &cli.command {
        return star::run(command);
    }
    let database = Database::open()?;
    let resolver = SkarbiecResolver::new(cli.skarbiec_bin);
    let command = match cli.command {
        Command::Serve(args) => return serve(database, resolver, args).await,
        other => other,
    };
    let organization = cli.organization.as_deref().ok_or_else(|| {
        AppError::invalid(
            "ORGANIZATION_REQUIRED",
            "--organization <ID> is required: the CLI acts for one organization and assumes none; `serve` alone watches every organization",
        )
    })?;
    let state =
        |callback: Option<&str>| AppState::new(database.clone(), resolver.clone(), callback);
    match command {
        Command::Serve(_) => unreachable!(),
        Command::Status => {
            let status = state(None)?.status(organization).await?;
            print_json(&status)
        }
        Command::Mailbox { command } => run_mailbox(state(None)?, organization, command).await,
        Command::Account { provider, command } => match (provider, command) {
            (
                MailProvider::Gmail,
                AccountCommand::Authorize {
                    skarbiec_item,
                    bind,
                },
            ) => authorize_gmail(database, resolver, organization, skarbiec_item, bind).await,
            (MailProvider::Gmail, AccountCommand::Connection { email, bind }) => {
                let callback = loopback_callback(bind)?;
                print_json(
                    &state(Some(&callback))?
                        .gmail_connection_readiness(organization, email.as_deref())
                        .await?,
                )
            }
            (
                MailProvider::Gmail,
                AccountCommand::Delegate(gmail::ConnectedAccount {
                    email,
                    display_name,
                }),
            ) => {
                let state = state(None)?;
                print_json(
                    &state
                        .connect_gmail_delegated(organization, &email, display_name)
                        .await?,
                )
            }
            (
                MailProvider::Gmail,
                AccountCommand::Password(gmail::ConnectedAccount {
                    email,
                    display_name,
                }),
            ) => {
                let password = read_gmail_app_password()?;
                let state = state(None)?;
                print_json(
                    &state
                        .connect_gmail_app_password(organization, &email, &password, display_name)
                        .await?,
                )
            }
            (
                MailProvider::Gmail,
                AccountCommand::Disconnect(gmail::DisconnectedAccount { email }),
            ) => print_json(&state(None)?.disconnect_gmail(&email).await?),
        },
        Command::Message { command } => run_message(state(None)?, organization, command).await,
        Command::Sync(mailbox::SyncArgs {
            mailbox,
            inbound_domain,
        }) => {
            let state = state(None)?;
            match (mailbox, inbound_domain) {
                (_, Some(id)) => print_json(&state.sync_inbound_domain(organization, id).await?),
                (Some(selector), None) => {
                    let id = state.resolve_mailbox(organization, &selector)?.id;
                    print_json(&state.sync_mailbox(organization, id).await?)
                }
                (None, None) => print_json(&state.sync_all(organization).await?),
            }
        }
        Command::Version => unreachable!(),
        Command::Onboarding { .. } => unreachable!(),
        Command::Star { .. } => unreachable!(),
    }
}

/// What `serve` takes.
#[derive(clap::Args)]
pub(super) struct ServeArgs {
    /// Loopback address the API listens on; a non-loopback address is refused.
    /// No address is assumed: the service declaration that runs `serve` names it.
    #[arg(long)]
    pub(super) bind: SocketAddr,
}

async fn serve(
    database: Database,
    resolver: SkarbiecResolver,
    args: ServeArgs,
) -> Result<(), AppError> {
    let callback_base_url = loopback_callback(args.bind)?;
    let state = AppState::new(database, resolver, Some(&callback_base_url))?;
    let auth_verifier = crate::auth::AuthVerifier::from_environment()?;
    state.clone().start_watching()?;
    let listener = tokio::net::TcpListener::bind(args.bind)
        .await
        .map_err(|error| {
            AppError::internal(format!(
                "loopback API address {} could not be bound: {error}",
                args.bind
            ))
        })?;
    tracing::info!(address = %args.bind, "Skrzynka API ready");
    axum::serve(listener, crate::api::router(state, auth_verifier))
        .with_graceful_shutdown(async {
            // Ctrl-C stops the server gracefully. A listener that cannot be
            // installed is said, and the server keeps serving until its
            // process is ended: read as a Ctrl-C, it stopped the server the
            // moment it had announced its address.
            if let Err(error) = tokio::signal::ctrl_c().await {
                tracing::error!(
                    %error,
                    "Ctrl-C cannot be listened for; the server stops only when its process is ended"
                );
                std::future::pending::<()>().await;
            }
        })
        .await
        .map_err(|error| AppError::internal(format!("loopback API stopped unexpectedly: {error}")))
}

/// The callback base URL for a loopback socket, refusing any other address:
/// Skrzynka serves its API and Google's callback only on loopback.
pub(super) fn loopback_callback(bind: SocketAddr) -> Result<String, AppError> {
    if !bind.ip().is_loopback() {
        return Err(AppError::new(
            StatusCode::BAD_REQUEST,
            "NON_LOOPBACK_BIND_REFUSED",
            format!("Skrzynka listens only on loopback addresses; {bind} is not one"),
            false,
        ));
    }
    Ok(format!("http://{bind}"))
}

/// Prints one result: pretty JSON for machines, or with `--text` one
/// `path: value` line per field for people, from the same value (cli.md rule 13).
pub(super) fn print_json(value: &impl serde::Serialize) -> Result<(), AppError> {
    let value = serde_json::to_value(value).map_err(|cause| {
        AppError::internal(format!("result could not be encoded as JSON: {cause}"))
    })?;
    if TEXT.get().copied().ok_or_else(|| {
        AppError::internal("a result was printed before the command chose JSON or --text")
    })? {
        let mut lines = String::new();
        flatten("", &value, &mut lines);
        print!("{lines}");
        return Ok(());
    }
    // A JSON value always renders; its pretty form is the machine output.
    println!("{value:#}");
    Ok(())
}

fn flatten(path: &str, value: &Value, lines: &mut String) {
    match value {
        Value::Object(map) if !map.is_empty() => {
            for (key, item) in map {
                let child = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{path}.{key}")
                };
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
