//! `skrzynka message contacts`: everyone the stored mail came from or
//! originated mail went to, printed, written as vCards or a CSV table, or
//! put on the suppression list.

use super::super::{output::NewFile, print_json};
use crate::{
    error::AppError,
    service::{contact_table, vcards, AppState, ContactFilter},
};
use std::path::PathBuf;
use uuid::Uuid;

/// What `message contacts` reads, and where it writes them as vCards.
#[derive(clap::Args)]
pub(in crate::cli) struct ContactsArgs {
    /// Only this mailbox's correspondents, by ID or address.
    #[arg(long)]
    mailbox: Option<String>,
    /// Only those whose address or a name they were written with contains this, case aside.
    #[arg(long)]
    contains: Option<String>,
    /// The latest exchanged with first, instead of the most exchanged.
    #[arg(long)]
    recent: bool,
    /// Only those a stored bounce report says delivery to failed for good, as `message bounced` lists them.
    #[arg(long)]
    bounced: bool,
    /// Only those an entry of `message suppressed` covers: no originated mail may go to them.
    #[arg(long)]
    suppressed: bool,
    /// Only those a blocked entry covers, itself or by its domain: whose mail listings leave out.
    #[arg(long)]
    blocked: bool,
    /// Only those exchanged with at or after this RFC 3339 instant or UTC day.
    #[arg(long, allow_hyphen_values = true)]
    since: Option<String>,
    /// Only those last exchanged with before this RFC 3339 instant or UTC day: who went quiet.
    #[arg(long, allow_hyphen_values = true)]
    until: Option<String>,
    /// Only those who wrote and were never written to: who is waiting on you.
    #[arg(long, conflicts_with = "never_received")]
    never_written: bool,
    /// Only those written to who never wrote back: who has not answered.
    #[arg(long)]
    never_received: bool,
    /// Write them as vCards (RFC 6350) to this new file instead of printing them.
    #[arg(long, conflicts_with = "csv")]
    vcard: Option<PathBuf>,
    /// Write them as a CSV table (RFC 4180), one row per contact with its counts and marks, to this new file.
    #[arg(long)]
    csv: Option<PathBuf>,
    /// Suppress every contact listed the suppression list does not cover yet; the listing must be narrowed.
    #[arg(long, conflicts_with = "vcard", conflicts_with = "csv", conflicts_with = "suppressed")]
    suppress_listed: bool,
    /// With --suppress-listed: why, kept on every entry added.
    #[arg(long, requires = "suppress_listed")]
    reason: Option<String>,
    /// Block every contact listed no blocked entry covers yet, so listings leave their mail out; the listing must be narrowed.
    #[arg(long, conflicts_with = "vcard", conflicts_with = "csv", conflicts_with = "suppress_listed", conflicts_with = "blocked")]
    block_listed: bool,
}

/// Print the correspondents, write them as vCards or a CSV table to a new
/// file and print what was written, or put them on the suppression or
/// blocked list.
pub(in crate::cli) fn contacts(state: &AppState, organization: &str, args: ContactsArgs) -> Result<(), AppError> {
    let ContactsArgs {
        mailbox, contains, recent, bounced, suppressed, blocked, since, until, never_written, never_received, vcard, csv,
        suppress_listed, reason, block_listed,
    } = args;
    let filter = ContactFilter {
        mailbox_id: super::chosen_mailbox(state, organization, mailbox)?,
        contains,
        recent,
        bounced,
        suppressed,
        blocked,
        since,
        until,
        never_written,
        never_received,
    };
    if suppress_listed {
        return print_json(&state.suppress_contacts(organization, &filter, reason.as_deref())?);
    }
    if block_listed {
        return print_json(&state.block_contacts(organization, &filter)?);
    }
    let (path, render): (PathBuf, fn(&[crate::service::Contact]) -> String) = match (vcard, csv) {
        (Some(path), _) => (path, vcards),
        (None, Some(path)) => (path, contact_table),
        (None, None) => return print_json(&state.list_contacts(organization, &filter)?),
    };
    let file = NewFile {
        path: &path,
        exists: "CONTACTS_OUTPUT_EXISTS",
        invalid: "CONTACTS_OUTPUT_INVALID",
    };
    file.refuse_occupied()?;
    let correspondents = state.list_contacts(organization, &filter)?;
    let written = render(&correspondents);
    file.write(written.as_bytes())?;
    print_json(&serde_json::json!({
        "path": path,
        "contacts": correspondents.len(),
        "size_bytes": written.len(),
    }))
}
