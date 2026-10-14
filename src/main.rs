mod api;
mod auth;
mod cli;
mod db;
mod error;
mod gmail;
mod mail;
mod models;
mod onboarding;
mod service;
mod skarbiec;

use clap::Parser;
use serde_json::json;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    // An unset RUST_LOG logs at info; one that does not parse is the
    // operator's filter unread, refused like any other failed command.
    let outcome = match log_filter() {
        Ok(filter) => {
            tracing_subscriber::fmt()
                .with_env_filter(filter)
                .with_writer(std::io::stderr)
                .init();
            cli::run(cli::Cli::parse()).await
        }
        Err(error) => Err(error),
    };
    if let Err(error) = outcome {
        eprintln!(
            "{}",
            // A json! value always renders, so the error is never replaced.
            json!({
                "error": {
                    "code": error.code,
                    "message": error.message,
                    "retryable": error.retryable,
                }
            })
        );
        // cli.md rule 10: 69 (EX_UNAVAILABLE) when the same command may succeed
        // later unchanged, 2 for an invocation that names no organization.
        let status = match (error.retryable, error.code) {
            (true, _) => 69,
            (false, "ORGANIZATION_REQUIRED") => 2,
            _ => 1,
        };
        std::process::exit(status);
    }
}

fn log_filter() -> Result<EnvFilter, error::AppError> {
    match std::env::var_os("RUST_LOG") {
        None => Ok(EnvFilter::new("info")),
        Some(_) => EnvFilter::try_from_default_env().map_err(|cause| {
            error::AppError::invalid(
                "INVALID_LOG_FILTER",
                format!("RUST_LOG is not a log filter: {cause}"),
            )
        }),
    }
}
