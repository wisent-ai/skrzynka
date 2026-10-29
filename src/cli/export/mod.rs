//! `skrzynka message export`: every stored message of one mailbox, those
//! received within a window, or those of it a saved view lists, saved as its
//! original, read from the provider, into a new directory, with a manifest of
//! what was written and what could not be read.

use super::{output::NewFile, print_json};
use crate::{
    error::AppError,
    service::{filter_instant, AppState, MessageFilters},
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use uuid::Uuid;

mod check;
mod mbox;
mod restore;
pub(super) use check::{verify_export, VerifyArgs};
pub(super) use restore::{restore, RestoreArgs};

/// What `message export` reads and where it writes.
#[derive(clap::Args)]
pub(super) struct ExportArgs {
    /// Mailbox ID, as `skrzynka mailbox list` prints it, or the mailbox's own address.
    #[arg(long)]
    mailbox: String,
    /// Directory to create; an existing path is never written into.
    #[arg(long)]
    output: PathBuf,
    /// Only the mailbox's messages this saved view lists, as `message list --view` does.
    #[arg(long, conflicts_with = "thread")]
    view: Option<String>,
    /// Only the conversation this message of the mailbox belongs to, as `message thread` lists it.
    #[arg(long)]
    thread: Option<Uuid>,
    /// Only the messages received at or after this RFC 3339 instant or UTC day: what came since the last export.
    #[arg(
        long,
        conflicts_with = "view",
        conflicts_with = "thread",
        allow_hyphen_values = true
    )]
    since: Option<String>,
    /// Only the messages received before this RFC 3339 instant or UTC day: an export of one period.
    #[arg(
        long,
        conflicts_with = "view",
        conflicts_with = "thread",
        allow_hyphen_values = true
    )]
    until: Option<String>,
    /// Only the messages carrying this tag, as `message list --tag` keeps them.
    #[arg(long, conflicts_with = "view", conflicts_with = "thread")]
    tag: Option<String>,
    /// Also write every exported original into one `mailbox.mbox` (mboxrd) a mail program imports.
    #[arg(long)]
    mbox: bool,
}

/// One original written: `<message id>.eml` in the directory.
#[derive(Serialize)]
struct Exported {
    message_id: Uuid,
    file: String,
    size_bytes: usize,
    sha256: String,
}

/// The mbox written beside the originals: its name, size and digest, and
/// how many messages it holds, in the order the manifest lists them.
#[derive(Serialize)]
struct MboxFile {
    file: String,
    size_bytes: usize,
    sha256: String,
    messages: usize,
}

/// One message whose original could not be read or written, and why.
#[derive(Serialize)]
struct NotExported {
    message_id: Uuid,
    code: &'static str,
    message: String,
}

/// What `message export` prints and keeps as `manifest.json`.
#[derive(Serialize)]
struct Manifest {
    mailbox_id: Uuid,
    /// The saved view the messages were chosen by; absent for every message.
    view: Option<String>,
    /// The message whose conversation was exported; absent otherwise.
    thread: Option<Uuid>,
    /// The instant the messages were received from; absent for every message.
    since: Option<String>,
    /// The instant the messages were received before; absent for no end.
    until: Option<String>,
    /// The tag the messages carry; absent for every message.
    tag: Option<String>,
    directory: PathBuf,
    exported: Vec<Exported>,
    refused: Vec<NotExported>,
    /// The mbox of every exported original; absent without `--mbox`.
    mbox: Option<MboxFile>,
}

pub(super) async fn export(
    state: &AppState,
    organization: &str,
    ExportArgs {
        mailbox,
        output,
        view,
        thread,
        since,
        until,
        tag,
        mbox,
    }: ExportArgs,
) -> Result<(), AppError> {
    let mailbox = state.resolve_mailbox(organization, &mailbox)?.id;
    // The window is read once, so a span such as -7d is one instant for the
    // whole export and the manifest names it.
    let since = filter_instant(since.as_deref(), "since")?.map(|at| at.to_rfc3339());
    let until = filter_instant(until.as_deref(), "until")?.map(|at| at.to_rfc3339());
    let messages = match (view.as_deref(), thread) {
        // The view's own filters choose; only this mailbox's messages are kept.
        (Some(name), _) => state
            .list_view(organization, name, None, None)?
            .into_iter()
            .filter(|message| message.mailbox_id == mailbox)
            .collect(),
        // A conversation is one mailbox's; a message of another is refused.
        (None, Some(id)) => {
            let held = state.get_message(organization, id)?.mailbox_id;
            if held != mailbox {
                return Err(AppError::invalid(
                    "EXPORT_THREAD_ELSEWHERE",
                    format!("message {id} belongs to mailbox {held}, not {mailbox}; export its conversation with --mailbox {held}"),
                ));
            }
            state.message_thread(organization, id)?
        }
        // Every stored message, or those received in the window or carrying
        // the tag: blocked senders', snoozed and muted mail included.
        (None, None) => {
            let filters = MessageFilters {
                include_blocked: true,
                include_snoozed: true,
                include_muted: true,
                since: since.as_deref(),
                until: until.as_deref(),
                tag: tag.as_deref(),
                ..MessageFilters::default()
            };
            state.list_messages(organization, Some(mailbox), filters, None, None)?
        }
    };
    std::fs::create_dir(&output).map_err(|error| match error.kind() {
        std::io::ErrorKind::AlreadyExists => AppError::invalid(
            "EXPORT_OUTPUT_EXISTS",
            format!(
                "{} already exists; an export never writes into an existing path: name one that does not exist",
                output.display()
            ),
        ),
        _ => AppError::invalid(
            "EXPORT_OUTPUT_INVALID",
            format!("{} could not be created: {error}", output.display()),
        ),
    })?;
    let mut manifest = Manifest {
        mailbox_id: mailbox,
        view,
        thread,
        since,
        until,
        tag,
        directory: output.clone(),
        exported: Vec::new(),
        refused: Vec::new(),
        mbox: None,
    };
    // Each exported original, as an mbox entry, when --mbox asks for one.
    let mut gathered: Vec<u8> = Vec::new();
    for message in messages {
        let name = format!("{}.eml", message.id);
        let path = output.join(&name);
        let written = match state.message_source(organization, message.id).await {
            Ok(source) => NewFile {
                path: &path,
                exists: "EXPORT_OUTPUT_EXISTS",
                invalid: "EXPORT_OUTPUT_INVALID",
            }
            .write(&source)
            .map(|()| source),
            Err(error) => Err(error),
        };
        match written {
            Ok(source) => {
                if mbox {
                    gathered.extend(mbox::entry(&source, &message.received_at, message.id)?);
                }
                manifest.exported.push(Exported {
                    message_id: message.id,
                    file: name,
                    size_bytes: source.len(),
                    sha256: format!("{:x}", Sha256::digest(&source)),
                })
            }
            Err(error) => manifest.refused.push(NotExported {
                message_id: message.id,
                code: error.code,
                message: error.message,
            }),
        }
    }
    if mbox {
        let file = "mailbox.mbox".to_string();
        NewFile {
            path: &output.join(&file),
            exists: "EXPORT_OUTPUT_EXISTS",
            invalid: "EXPORT_OUTPUT_INVALID",
        }
        .write(&gathered)?;
        manifest.mbox = Some(MboxFile {
            file,
            size_bytes: gathered.len(),
            sha256: format!("{:x}", Sha256::digest(&gathered)),
            messages: manifest.exported.len(),
        });
    }
    let listing = serde_json::to_vec_pretty(&manifest).map_err(|error| {
        AppError::internal(format!(
            "the export manifest could not be written as JSON: {error}"
        ))
    })?;
    NewFile {
        path: &output.join("manifest.json"),
        exists: "EXPORT_OUTPUT_EXISTS",
        invalid: "EXPORT_OUTPUT_INVALID",
    }
    .write(&listing)?;
    print_json(&manifest)?;
    if manifest.refused.is_empty() {
        return Ok(());
    }
    Err(AppError::invalid(
        "EXPORT_INCOMPLETE",
        format!(
            "{} of the mailbox's messages could not be exported; each is listed under refused in {}",
            manifest.refused.len(),
            output.join("manifest.json").display()
        ),
    ))
}
