//! `skrzynka message outbound`: reading back what went out, one message by
//! ID, a listing narrowed by its flags, that listing written as a CSV table,
//! an answer expected to every message of it, or its failures sent again.

use super::super::{output::NewFile, print_json};
use crate::{
    error::AppError,
    service::{outbound_table, AppState, OutboundFilters},
};
use std::path::PathBuf;
use uuid::Uuid;

/// What `message outbound` reads: one message by ID, or a listing.
#[derive(clap::Args)]
pub(in crate::cli) struct OutboundArgs {
    /// Only this outbound message, as `skrzynka message send` or this
    /// listing prints its ID; it cannot be combined with the listing's flags.
    #[arg(long, conflicts_with = "listing")]
    id: Option<Uuid>,
    #[command(flatten)]
    listing: OutboundListing,
}

/// The listing's flags, which `--id` excludes.
#[derive(clap::Args)]
#[group(id = "listing", multiple = true)]
struct OutboundListing {
    /// Only messages sent from this mailbox.
    #[arg(long)]
    mailbox: Option<String>,
    /// Only messages whose To, Cc or Bcc contains this, case aside.
    #[arg(long)]
    to: Option<String>,
    /// Only messages whose subject or body contains this, case aside.
    #[arg(long)]
    contains: Option<String>,
    /// Only messages in this delivery status (pending, sending, sent, failed, uncertain).
    #[arg(long)]
    status: Option<String>,
    /// Only messages that carried a file whose name contains this, case aside.
    #[arg(long)]
    attachment: Option<String>,
    /// Only messages claimed at or after this RFC 3339 instant or UTC day.
    #[arg(long, allow_hyphen_values = true)]
    since: Option<String>,
    /// Only messages claimed before this RFC 3339 instant or UTC day.
    #[arg(long, allow_hyphen_values = true)]
    until: Option<String>,
    /// Only messages a stored bounce report returned, matched by Message-ID.
    #[arg(long)]
    bounced: bool,
    /// Only messages a stored message of the sending mailbox answers, by In-Reply-To or References.
    #[arg(long, conflicts_with = "unanswered")]
    answered: bool,
    /// Only messages nothing stored answers yet.
    #[arg(long)]
    unanswered: bool,
    /// List the oldest claimed first; --limit and --offset page in that order.
    #[arg(long)]
    oldest_first: bool,
    /// Messages per page; without it, every message from --offset on.
    #[arg(long)]
    limit: Option<u32>,
    /// Messages to skip before the page; without it, none are skipped.
    #[arg(long)]
    offset: Option<u32>,
    /// Write the messages listed as a CSV table (RFC 4180), one row each without the body, to this new file.
    #[arg(long)]
    csv: Option<PathBuf>,
    /// Expect an answer by this RFC 3339 instant or span from now (+3d) to every sent message listed, as `followup set` does to one.
    #[arg(long, conflicts_with = "csv")]
    follow_up: Option<String>,
    /// Send every failed message listed again, as written, each claimed by <KEY>-<outbound id>; needs --idempotency-key.
    #[arg(long, conflicts_with = "csv", conflicts_with = "follow_up", requires = "idempotency_key")]
    resend_failed: bool,
    /// With --resend-failed: the key each new send is claimed by; required, never drawn.
    #[arg(long, requires = "resend_failed")]
    idempotency_key: Option<String>,
}

/// Print one outbound message, the listing, write the listing as a CSV table
/// to a new file and print what was written, or act on every message of it.
pub(in crate::cli) async fn outbound(state: &AppState, organization: &str, args: OutboundArgs) -> Result<(), AppError> {
    if let Some(id) = args.id {
        return print_json(&state.get_outbound(organization, id)?);
    }
    let listing = args.listing;
    let mailbox_id = super::super::reading::chosen_mailbox(state, organization, listing.mailbox)?;
    let filters = OutboundFilters {
        to: listing.to,
        contains: listing.contains,
        status: listing.status,
        attachment: listing.attachment,
        since: listing.since,
        until: listing.until,
        bounced: listing.bounced,
        answered: (listing.answered || listing.unanswered).then_some(listing.answered),
        oldest_first: listing.oldest_first,
    };
    if let Some(by) = listing.follow_up {
        return print_json(&state.expect_answers(organization, mailbox_id, &filters, listing.limit, listing.offset, &by)?);
    }
    if listing.resend_failed {
        let key = super::super::message::required_key(listing.idempotency_key, "message outbound --resend-failed")?;
        return print_json(&state.resend_failed(organization, mailbox_id, &filters, &key).await?);
    }
    let Some(path) = listing.csv else {
        return print_json(&state.search_outbound(organization, mailbox_id, &filters, listing.limit, listing.offset)?);
    };
    let file = NewFile { path: &path, exists: "OUTBOUND_OUTPUT_EXISTS", invalid: "OUTBOUND_OUTPUT_INVALID" };
    file.refuse_occupied()?;
    let messages = state.search_outbound(organization, mailbox_id, &filters, listing.limit, listing.offset)?;
    let table = outbound_table(&messages);
    file.write(table.as_bytes())?;
    print_json(&serde_json::json!({ "path": path, "messages": messages.len(), "size_bytes": table.len() }))
}
