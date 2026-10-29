//! The message subcommands that read: one conversation, contacts, tags,
//! notes, snoozes, and a message's original saved to a new file.

use super::{output::NewFile, print_json};
use crate::{
    error::AppError,
    service::{subscription_table, tag_table, AppState, SubscriptionFilter, TagFilter},
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use uuid::Uuid;

mod blocked;
mod bounced;
mod contacts;
pub(super) use blocked::{blocked, BlockedArgs};
pub(super) use bounced::{bounced, BouncedArgs};
pub(super) use contacts::{contacts, ContactsArgs};
pub(super) mod mutes;

/// The mailbox a `--mailbox` flag names, by ID or address, as `message send
/// --mailbox` takes it; none when the flag is absent. An unknown one is
/// `NOT_FOUND`, an address more than one mailbox has
/// `MAILBOX_SELECTOR_AMBIGUOUS`.
pub(super) fn chosen_mailbox(
    state: &AppState,
    organization: &str,
    selector: Option<String>,
) -> Result<Option<Uuid>, AppError> {
    match selector {
        Some(written) => Ok(Some(state.resolve_mailbox(organization, &written)?.id)),
        None => Ok(None),
    }
}

/// One stored message, named by its ID.
#[derive(clap::Args)]
pub(super) struct MessageRef {
    /// Message ID, as `skrzynka message list` prints it.
    pub(super) id: Uuid,
}

/// What `message subscriptions` lists: one mailbox's lists or every
/// mailbox's, only those still subscribed to, or those matching a text.
#[derive(clap::Args)]
pub(super) struct SubscriptionsArgs {
    /// Only this mailbox's lists, by ID or address.
    #[arg(long)]
    pub(super) mailbox: Option<String>,
    /// Only the lists no request to leave has been sent to (none ended `sent`).
    #[arg(long)]
    pub(super) subscribed: bool,
    /// Only the lists whose List-Id or sender contains this, case aside.
    #[arg(long)]
    pub(super) contains: Option<String>,
    /// Only the lists whose newest message arrived at or after an RFC 3339 instant, YYYY-MM-DD day or span back such as -30d.
    #[arg(long, allow_hyphen_values = true)]
    pub(super) since: Option<String>,
    /// Only the lists whose newest message arrived before this: lists that have gone quiet.
    #[arg(long, allow_hyphen_values = true)]
    pub(super) until: Option<String>,
    /// Only the lists Skrzynka can leave itself: those offering a one-click or mailto way out.
    #[arg(long)]
    pub(super) leavable: bool,
    /// Write the lists kept as a CSV table (RFC 4180), one row each, to this new file.
    #[arg(long)]
    pub(super) csv: Option<PathBuf>,
    /// Ask every list kept to stop sending instead, each through its newest message; needs --contains, --since or --until.
    #[arg(long, conflicts_with = "csv", requires = "idempotency_key")]
    pub(super) leave: bool,
    /// With --leave: the key each list's request is claimed by, as <KEY>-<newest message id>; required, never drawn.
    #[arg(long, requires = "leave")]
    pub(super) idempotency_key: Option<String>,
}

/// Print the mailing lists the filters keep, write them as a CSV table to a
/// new file and print what was written, or ask each to stop sending.
pub(super) async fn subscriptions(
    state: &AppState,
    organization: &str,
    args: SubscriptionsArgs,
) -> Result<(), AppError> {
    let SubscriptionsArgs {
        mailbox,
        subscribed,
        contains,
        since,
        until,
        leavable,
        csv,
        leave,
        idempotency_key,
    } = args;
    let mailbox_id = chosen_mailbox(state, organization, mailbox)?;
    let filter = SubscriptionFilter {
        mailbox_id,
        subscribed,
        contains,
        since,
        until,
        leavable,
    };
    if leave {
        let key = super::message::required_key(idempotency_key, "message subscriptions --leave")?;
        return print_json(
            &state
                .unsubscribe_listed(organization, &filter, &key)
                .await?,
        );
    }
    let Some(path) = csv else {
        return print_json(&state.list_subscriptions(organization, &filter)?);
    };
    let file = NewFile {
        path: &path,
        exists: "SUBSCRIPTION_OUTPUT_EXISTS",
        invalid: "SUBSCRIPTION_OUTPUT_INVALID",
    };
    file.refuse_occupied()?;
    let lists = state.list_subscriptions(organization, &filter)?.lists;
    let table = subscription_table(&lists);
    file.write(table.as_bytes())?;
    print_json(
        &serde_json::json!({ "path": path, "lists": lists.len(), "size_bytes": table.len() }),
    )
}

/// What `message unsubscribe` leaves: the list a stored message came from,
/// or the list named by its List-Id; and the key that makes a repeated
/// request the same one.
#[derive(clap::Args)]
pub(super) struct UnsubscribeArgs {
    /// Message ID, as `skrzynka message list` prints it; or name the list with --list.
    #[arg(required_unless_present = "list", conflicts_with = "list")]
    pub(super) id: Option<Uuid>,
    /// The list's List-Id, as `message subscriptions` prints it; its newest stored message carries the way out.
    #[arg(long)]
    pub(super) list: Option<String>,
    /// With --list: the mailbox, by ID or address, whose copy of the list to leave when several receive it.
    #[arg(long, requires = "list")]
    pub(super) mailbox: Option<String>,
    /// Key that makes a repeated request the same request; required, never drawn.
    #[arg(long)]
    pub(super) idempotency_key: Option<String>,
}

/// One sender to block or unblock: an address, or `@domain` for all of one.
#[derive(clap::Args)]
pub(super) struct SenderArg {
    /// The address, such as news@example.com, or @example.com for its domain.
    pub(super) address: String,
}

/// What `message snooze` sets: on one message, every message a saved view
/// lists, or every message of one conversation.
#[derive(clap::Args)]
pub(super) struct SnoozeArgs {
    /// Message ID, as `skrzynka message list` prints it.
    #[arg(
        required_unless_present = "view",
        required_unless_present = "thread",
        conflicts_with = "view",
        conflicts_with = "thread"
    )]
    pub(super) id: Option<Uuid>,
    /// Snooze every message this saved view lists instead of one.
    #[arg(long, conflicts_with = "thread")]
    pub(super) view: Option<String>,
    /// Snooze every message of the conversation this message belongs to, as `message thread` lists it.
    #[arg(long)]
    pub(super) thread: Option<Uuid>,
    /// RFC 3339 instant ahead of now, or a span from now such as +2h, +3d or +1w (m, h, d, w).
    #[arg(long)]
    pub(super) until: String,
}

/// What `message wake` shows again: one message, or every snoozed message of
/// a view or of a conversation.
#[derive(clap::Args)]
pub(super) struct WakeArgs {
    /// Message ID, as `skrzynka message list` prints it.
    #[arg(
        required_unless_present = "view",
        required_unless_present = "thread",
        conflicts_with = "view",
        conflicts_with = "thread"
    )]
    pub(super) id: Option<Uuid>,
    /// Wake every snoozed message this saved view keeps instead of one.
    #[arg(long, conflicts_with = "thread")]
    pub(super) view: Option<String>,
    /// Wake every snoozed message of the conversation this message belongs to.
    #[arg(long)]
    pub(super) thread: Option<Uuid>,
}

/// What `message note` keeps or removes.
#[derive(clap::Args)]
pub(super) struct NoteArgs {
    /// Message ID, as `skrzynka message list` prints it.
    pub(super) id: Uuid,
    /// UTF-8 file holding the note; it replaces the message's note.
    #[arg(long, required_unless_present = "clear", conflicts_with = "clear")]
    pub(super) file: Option<PathBuf>,
    /// Remove the message's note.
    #[arg(long)]
    pub(super) clear: bool,
    /// Add the file's text to the note on a line of its own instead of replacing it.
    #[arg(long, requires = "file")]
    pub(super) append: bool,
}

/// What `message source` saves: one stored message's original, to a new file.
#[derive(clap::Args)]
pub(super) struct SourceArgs {
    /// Message ID, as `skrzynka message list` prints it.
    id: Uuid,
    /// New file the original is written to; an existing file is never replaced.
    #[arg(long)]
    output: PathBuf,
}

/// What `message tag` changes: one stored message, every message a saved
/// view lists, or every message of one conversation.
#[derive(clap::Args)]
pub(super) struct TagArgs {
    /// Message ID, as `skrzynka message list` prints it.
    #[arg(
        required_unless_present = "view",
        required_unless_present = "thread",
        conflicts_with = "view",
        conflicts_with = "thread"
    )]
    pub(super) id: Option<Uuid>,
    /// Change every message this saved view lists instead of one.
    #[arg(long, conflicts_with = "thread")]
    pub(super) view: Option<String>,
    /// Change every message of the conversation this message belongs to, as `message thread` lists it.
    #[arg(long)]
    pub(super) thread: Option<Uuid>,
    /// Tag to add; repeatable.
    #[arg(long = "add")]
    pub(super) add: Vec<String>,
    /// Tag to remove; repeatable.
    #[arg(long = "remove")]
    pub(super) remove: Vec<String>,
}

/// What `message tags` does: list every tag, or rename or remove one everywhere.
#[derive(clap::Args)]
pub(super) struct TagsArgs {
    /// Rename this tag on every message, tag rule and saved view; needs --to.
    #[arg(long, requires = "to", conflicts_with = "remove")]
    pub(super) rename: Option<String>,
    /// The name --rename gives the tag; a message carrying both keeps one.
    #[arg(long, requires = "rename")]
    pub(super) to: Option<String>,
    /// Take this tag off every message; rules and views naming it are kept.
    #[arg(long)]
    pub(super) remove: Option<String>,
    /// List only the tags this mailbox's messages carry, with their counts there; by ID or address.
    #[arg(long, conflicts_with = "rename", conflicts_with = "remove")]
    pub(super) mailbox: Option<String>,
    /// List only the tags holding this text, case aside.
    #[arg(long, conflicts_with = "rename", conflicts_with = "remove")]
    pub(super) contains: Option<String>,
    /// Count only messages received at or after an RFC 3339 instant, YYYY-MM-DD day or span back such as -30d.
    #[arg(
        long,
        conflicts_with = "rename",
        conflicts_with = "remove",
        allow_hyphen_values = true
    )]
    pub(super) since: Option<String>,
    /// Count only messages received before this.
    #[arg(
        long,
        conflicts_with = "rename",
        conflicts_with = "remove",
        allow_hyphen_values = true
    )]
    pub(super) until: Option<String>,
    /// Write the tags listed as a CSV table (tag, messages) to this new file.
    #[arg(long, conflicts_with = "rename", conflicts_with = "remove")]
    pub(super) csv: Option<PathBuf>,
}

/// Print the tags `message tags` lists, or write them as a CSV table to a
/// new file and print what was written.
pub(super) fn list_tags(
    state: &AppState,
    organization: &str,
    args: TagsArgs,
) -> Result<(), AppError> {
    let mailbox_id = chosen_mailbox(state, organization, args.mailbox)?;
    let filter = TagFilter {
        mailbox_id,
        contains: args.contains,
        since: args.since,
        until: args.until,
    };
    let Some(path) = args.csv else {
        return print_json(&state.list_tags(organization, &filter)?);
    };
    let file = NewFile {
        path: &path,
        exists: "TAG_OUTPUT_EXISTS",
        invalid: "TAG_OUTPUT_INVALID",
    };
    file.refuse_occupied()?;
    let tags = state.list_tags(organization, &filter)?;
    let table = tag_table(&tags);
    file.write(table.as_bytes())?;
    print_json(&serde_json::json!({ "path": path, "tags": tags.len(), "size_bytes": table.len() }))
}

/// What `message source` wrote: the message, the file, and the digest of
/// the bytes in it.
#[derive(Serialize)]
struct SavedSource {
    message_id: Uuid,
    size_bytes: usize,
    sha256: String,
    path: PathBuf,
}

/// Read a message's original from the provider and write it to a new file.
pub(super) async fn save_source(
    state: &AppState,
    organization: &str,
    SourceArgs { id, output }: SourceArgs,
) -> Result<(), AppError> {
    let file = NewFile {
        path: &output,
        exists: "MESSAGE_SOURCE_OUTPUT_EXISTS",
        invalid: "MESSAGE_SOURCE_OUTPUT_INVALID",
    };
    // An occupied path is refused before the provider is read.
    file.refuse_occupied()?;
    let source = state.message_source(organization, id).await?;
    file.write(&source)?;
    print_json(&SavedSource {
        message_id: id,
        size_bytes: source.len(),
        sha256: format!("{:x}", Sha256::digest(&source)),
        path: output,
    })
}
