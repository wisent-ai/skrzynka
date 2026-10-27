//! `skrzynka message list`: the stored mail a listing keeps, by the filters
//! given or a saved view.

use super::{output::NewFile, print_json};
use crate::{
    error::AppError,
    service::{message_table, AppState, SavedFilters},
};
use std::path::PathBuf;

/// What `message list` narrows the stored mail by, and the page it prints.
#[derive(clap::Args)]
pub(super) struct ListArgs {
    /// Only this mailbox's messages, by ID or address.
    #[arg(long)]
    pub(super) mailbox: Option<String>,
    /// Only messages whose To header names this address whole.
    #[arg(long)]
    pub(super) recipient: Option<String>,
    /// Only messages whose Cc header names this address whole.
    #[arg(long)]
    pub(super) cc: Option<String>,
    /// Only messages whose From header contains this text.
    #[arg(long)]
    pub(super) sender: Option<String>,
    /// Only messages whose subject, text or your note contains this, case aside. (The
    /// global `--text` chooses the output format, so this one is `--contains`.)
    #[arg(long)]
    pub(super) contains: Option<String>,
    /// Only messages received at or after this RFC 3339 instant or UTC day (YYYY-MM-DD).
    #[arg(long, allow_hyphen_values = true)]
    pub(super) since: Option<String>,
    /// Only messages received before this RFC 3339 instant or UTC day (YYYY-MM-DD).
    #[arg(long, allow_hyphen_values = true)]
    pub(super) until: Option<String>,
    /// Only messages carrying this tag.
    #[arg(long)]
    pub(super) tag: Option<String>,
    /// Only messages from the mailing list whose List-Id contains this.
    #[arg(long)]
    pub(super) list: Option<String>,
    /// Only messages carrying an attachment whose file name contains this.
    #[arg(long)]
    pub(super) attachment: Option<String>,
    /// Only messages the import recorded at least one attachment of.
    #[arg(long)]
    pub(super) with_attachments: bool,
    /// Only messages whose recorded files come to at least this many bytes: what takes the room.
    #[arg(long, value_name = "BYTES")]
    pub(super) files_over: Option<u64>,
    /// Only bounce reports (delivery status notifications).
    #[arg(long)]
    pub(super) bounces: bool,
    /// Include the mail of blocked senders, which listings leave out otherwise.
    #[arg(long)]
    pub(super) include_blocked: bool,
    /// Include snoozed messages, which listings leave out until they wake.
    #[arg(long)]
    pub(super) include_snoozed: bool,
    /// Include the mail of muted conversations, which listings leave out otherwise.
    #[arg(long)]
    pub(super) include_muted: bool,
    /// Only messages no reply to has been sent for (none ended `sent`).
    #[arg(long)]
    pub(super) unanswered: bool,
    /// Only messages a reply to has been sent for (one ended `sent`).
    #[arg(long, conflicts_with = "unanswered")]
    pub(super) answered: bool,
    /// Only messages whose receiving provider recorded an authentication verdict other than pass.
    #[arg(long)]
    pub(super) failed_authentication: bool,
    /// List the messages oldest first instead of newest first; pages are cut in that order.
    #[arg(long)]
    pub(super) oldest_first: bool,
    /// Only messages snoozed until an instant still ahead, with when each wakes.
    #[arg(long)]
    pub(super) snoozed: bool,
    /// Only messages carrying your note.
    #[arg(long)]
    pub(super) noted: bool,
    /// Only the mail of muted conversations.
    #[arg(long)]
    pub(super) muted: bool,
    /// Only messages carrying no tag: the mail not sorted yet.
    #[arg(long, conflicts_with = "tag")]
    pub(super) untagged: bool,
    /// List what a saved view keeps instead of naming filters here.
    #[arg(long)]
    pub(super) view: Option<String>,
    /// Keep these filters (and --mailbox) as a view under this name, then list.
    #[arg(long, conflicts_with = "view")]
    pub(super) save_view: Option<String>,
    /// Messages per page; without it, every message from --offset on.
    #[arg(long)]
    pub(super) limit: Option<u32>,
    /// Messages to skip before the page; without it, none are skipped.
    #[arg(long)]
    pub(super) offset: Option<u32>,
    /// Write the messages listed as a CSV table (RFC 4180), one row each without the text but its snippet, to this new file.
    #[arg(long)]
    pub(super) csv: Option<PathBuf>,
}

/// Print the stored messages `list` keeps: those of a saved view, or of the
/// filters given, kept as a view first when `--save-view` names one; with
/// `--csv`, write them as a CSV table to a new file and print what was written.
pub(super) fn list(state: &AppState, organization: &str, list: ListArgs) -> Result<(), AppError> {
    let saved = SavedFilters {
        mailbox_id: super::reading::chosen_mailbox(state, organization, list.mailbox)?,
        recipient: list.recipient,
        cc: list.cc,
        sender: list.sender,
        text: list.contains,
        since: list.since,
        until: list.until,
        tag: list.tag,
        list: list.list,
        attachment: list.attachment,
        with_attachments: list.with_attachments,
        bounces: list.bounces,
        include_blocked: list.include_blocked,
        include_snoozed: list.include_snoozed,
        include_muted: list.include_muted,
        unanswered: list.unanswered,
        failed_authentication: list.failed_authentication,
        oldest_first: list.oldest_first,
        snoozed: list.snoozed,
        noted: list.noted,
        muted: list.muted,
        untagged: list.untagged,
        answered: list.answered,
        files_over: list.files_over,
    };
    let (view, save_view) = (list.view.as_deref(), list.save_view.as_deref());
    let Some(path) = list.csv else {
        return print_json(&state.list_listing(organization, &saved, view, save_view, list.limit, list.offset)?);
    };
    let file = NewFile { path: &path, exists: "MESSAGE_OUTPUT_EXISTS", invalid: "MESSAGE_OUTPUT_INVALID" };
    file.refuse_occupied()?;
    let messages = state.list_listing(organization, &saved, view, save_view, list.limit, list.offset)?;
    let table = message_table(&messages);
    file.write(table.as_bytes())?;
    print_json(&serde_json::json!({ "path": path, "messages": messages.len(), "size_bytes": table.len() }))
}
