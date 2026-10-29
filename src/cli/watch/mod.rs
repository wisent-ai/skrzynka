//! `skrzynka message watch`: print every message stored from now on, or
//! after a stored-order number, as each import stores it, until the process
//! is stopped: one JSON object per line, or with `--text` the message's
//! `path: value` lines (docs/messages/watch.md).

use crate::{error::AppError, service::AppState};
use clap::Args;
use std::io::Write;

/// What `message watch` keeps to and where it starts.
#[derive(Args)]
pub(super) struct WatchArgs {
    /// Only this mailbox's messages, by ID or address.
    #[arg(long)]
    mailbox: Option<String>,
    /// Start after this stored-order number, as a line printed earlier carries
    /// it in `stored_order`: every message stored since is printed first.
    #[arg(long)]
    after: Option<i64>,
}

pub(super) async fn watch(
    state: &AppState,
    organization: &str,
    args: WatchArgs,
) -> Result<(), AppError> {
    let mut watch = state.watch_messages(organization, args.mailbox.as_deref(), args.after)?;
    let text = super::TEXT.get().copied().ok_or_else(|| {
        AppError::internal("message watch started before the command chose JSON or --text")
    })?;
    loop {
        for each in watch.next().await? {
            if text {
                super::print_json(&each)?;
                continue;
            }
            let line = serde_json::to_string(&each).map_err(|error| {
                AppError::internal(format!(
                    "a watched message could not be written as JSON: {error}"
                ))
            })?;
            let mut out = std::io::stdout().lock();
            writeln!(out, "{line}")
                .and_then(|()| out.flush())
                .map_err(|error| {
                    AppError::internal(format!(
                        "standard output refused a watched message: {error}"
                    ))
                })?;
        }
    }
}
