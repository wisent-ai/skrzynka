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
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .init();
    if let Err(error) = cli::run(cli::Cli::parse()).await {
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
