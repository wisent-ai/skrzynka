//! `skrzynka message replies`: the reply attempts made to one message, or
//! every reply sent, narrowed by its flags, written as a CSV table, or its
//! failures sent again.

use super::super::{output::NewFile, print_json};
use crate::{
    error::AppError,
    service::{reply_table, AppState, ReplyFilter},
};
use std::path::PathBuf;
use uuid::Uuid;

/// What `message replies` lists: one message's reply attempts, or every
/// reply sent from every mailbox or one, in one delivery status or window.
#[derive(clap::Args)]
pub(in crate::cli) struct RepliesArgs {
    /// Message ID, as `skrzynka message list` prints it; without it, every message's replies.
    #[arg(
        conflicts_with = "mailbox",
        conflicts_with = "status",
        conflicts_with = "since",
        conflicts_with = "until",
        conflicts_with = "contains",
        conflicts_with = "attachment",
        conflicts_with = "oldest_first",
        conflicts_with = "resend_failed"
    )]
    id: Option<Uuid>,
    /// Only the replies to this mailbox's messages, by ID or address.
    #[arg(long)]
    mailbox: Option<String>,
    /// Only the replies in this delivery status (pending, sending, sent, failed, uncertain).
    #[arg(long)]
    status: Option<String>,
    /// Only the replies claimed at or after this RFC 3339 instant or UTC day.
    #[arg(long, allow_hyphen_values = true)]
    since: Option<String>,
    /// Only the replies claimed before this RFC 3339 instant or UTC day.
    #[arg(long, allow_hyphen_values = true)]
    until: Option<String>,
    /// Only the replies whose text contains this, case aside.
    #[arg(long)]
    contains: Option<String>,
    /// Only the replies that carried a file whose name contains this, case aside.
    #[arg(long)]
    attachment: Option<String>,
    /// List the oldest claimed first.
    #[arg(long)]
    oldest_first: bool,
    /// Write the replies listed as a CSV table (RFC 4180), one row each without its text, to this new file.
    #[arg(long)]
    csv: Option<PathBuf>,
    /// Send every failed reply listed again, as sent, each claimed by <KEY>-<reply id>; needs --idempotency-key.
    #[arg(long, conflicts_with = "csv", requires = "idempotency_key")]
    resend_failed: bool,
    /// With --resend-failed: the key each new reply is claimed by; required, never drawn.
    #[arg(long, requires = "resend_failed")]
    idempotency_key: Option<String>,
}

/// Print one message's reply attempts or every reply the flags keep, write
/// those as a CSV table to a new file and print what was written, or send
/// the failed ones again.
pub(in crate::cli) async fn replies(
    state: &AppState,
    organization: &str,
    args: RepliesArgs,
) -> Result<(), AppError> {
    let RepliesArgs {
        id,
        mailbox,
        status,
        since,
        until,
        contains,
        attachment,
        oldest_first,
        csv,
        resend_failed,
        idempotency_key,
    } = args;
    if resend_failed {
        let key = super::super::message::required_key(
            idempotency_key,
            "message replies --resend-failed",
        )?;
        let mailbox_id = super::super::reading::chosen_mailbox(state, organization, mailbox)?;
        let filter = ReplyFilter {
            mailbox_id,
            status,
            since,
            until,
            contains,
            attachment,
            oldest_first,
        };
        return print_json(
            &state
                .resend_failed_replies(organization, &filter, &key)
                .await?,
        );
    }
    let file = match &csv {
        Some(path) => Some(NewFile {
            path: path.as_path(),
            exists: "REPLY_OUTPUT_EXISTS",
            invalid: "REPLY_OUTPUT_INVALID",
        }),
        None => None,
    };
    if let Some(file) = &file {
        file.refuse_occupied()?;
    }
    let replies = match id {
        Some(id) => state.list_replies(organization, id)?,
        None => {
            let mailbox_id = super::super::reading::chosen_mailbox(state, organization, mailbox)?;
            state.all_replies(
                organization,
                &ReplyFilter {
                    mailbox_id,
                    status,
                    since,
                    until,
                    contains,
                    attachment,
                    oldest_first,
                },
            )?
        }
    };
    let Some(file) = file else {
        return print_json(&replies);
    };
    let table = reply_table(&replies);
    file.write(table.as_bytes())?;
    print_json(
        &serde_json::json!({ "path": file.path, "replies": replies.len(), "size_bytes": table.len() }),
    )
}
