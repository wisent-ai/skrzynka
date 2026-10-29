//! `skrzynka mailbox responder`: read a mailbox's automatic reply, set it
//! from a file, remove it, read the responder's decisions, or answer what
//! is waiting now (docs/replies/auto-reply.md).

use super::print_json;
use crate::{
    error::AppError,
    service::{AppState, AutoReplyRequest},
};
use clap::Args;

/// What `mailbox responder` reads or changes.
#[derive(Args)]
pub(super) struct AutoReplyArgs {
    /// Mailbox ID, as `skrzynka mailbox list` prints it.
    id: uuid::Uuid,
    /// UTF-8 file holding the reply's text; sets the automatic reply.
    #[arg(
        long,
        requires = "every",
        conflicts_with = "clear",
        conflicts_with = "decisions",
        conflicts_with = "answer"
    )]
    file: Option<std::path::PathBuf>,
    /// The span one sender waits before being answered again, such as 7d (m, h, d or w).
    #[arg(long, requires = "file")]
    every: Option<String>,
    /// Answer mail received from this RFC 3339 instant or span from now (+2h); now when omitted.
    #[arg(long, requires = "file", allow_hyphen_values = true)]
    from: Option<String>,
    /// Stop answering mail received from this RFC 3339 instant or span from now (+5d).
    #[arg(long, requires = "file", allow_hyphen_values = true)]
    until: Option<String>,
    /// Remove the automatic reply; decisions already made are kept.
    #[arg(long, conflicts_with = "decisions", conflicts_with = "answer")]
    clear: bool,
    /// Every decision the responder made for the mailbox, newest first.
    #[arg(long, conflicts_with = "answer")]
    decisions: bool,
    /// Answer every message of the organization's mailboxes the responder has not decided on, now.
    #[arg(long)]
    answer: bool,
}

pub(super) async fn run_auto_reply(
    state: &AppState,
    organization: &str,
    args: AutoReplyArgs,
) -> Result<(), AppError> {
    if args.answer {
        return print_json(&state.answer_automatically(Some(organization)).await?);
    }
    if args.decisions {
        return print_json(&state.auto_reply_decisions(organization, args.id)?);
    }
    if args.clear {
        return print_json(&state.set_mailbox_auto_reply(organization, args.id, None)?);
    }
    match (args.file, args.every) {
        (Some(file), Some(every)) => {
            let body = super::message::read_body_file(&file, "AUTO_REPLY_INVALID")?;
            let request = AutoReplyRequest {
                body,
                every,
                from: args.from,
                until: args.until,
            };
            print_json(&state.set_mailbox_auto_reply(organization, args.id, Some(request))?)
        }
        _ => print_json(&state.mailbox_auto_reply(organization, args.id)?),
    }
}
