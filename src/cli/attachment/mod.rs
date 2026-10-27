//! `skrzynka message attachment`: what files a message carries, and one of
//! them, or every one, read from the provider and written to new files; and
//! the files of all stored mail found, listed, tabled or saved together.

use super::{arguments::AttachmentCommand, output::NewFile, print_json};
use crate::{
    error::AppError,
    service::{file_table, AppState, FileFilter},
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use uuid::Uuid;

mod all;

/// What `message attachment list` reads.
#[derive(clap::Args)]
pub(super) struct ListArgs {
    /// Message ID, as `skrzynka message list` prints it.
    message: Uuid,
}

/// What `message attachment save` reads and where it writes.
#[derive(clap::Args)]
pub(super) struct SaveArgs {
    /// Message ID, as `skrzynka message list` prints it.
    message: Uuid,
    /// The attachment's index, as `message attachment list` prints it.
    #[arg(long, required_unless_present = "all", conflicts_with = "all")]
    index: Option<usize>,
    /// Save every attachment into the new directory --output names.
    #[arg(long)]
    all: bool,
    /// File to create (with --all, the directory); an existing path is never written into.
    #[arg(long)]
    output: PathBuf,
}

/// What `message attachment find` keeps.
#[derive(clap::Args)]
pub(super) struct FindArgs {
    /// Only this mailbox's mail, by ID or address.
    #[arg(long)]
    mailbox: Option<String>,
    /// Only files whose recorded name contains this, case aside.
    #[arg(long)]
    name: Option<String>,
    /// Only files whose content type contains this, case aside (pdf, image/).
    #[arg(long = "type")]
    content_type: Option<String>,
    /// Only files of at least this many recorded bytes.
    #[arg(long)]
    over: Option<u64>,
    /// Only files of mail whose From header contains this, case aside.
    #[arg(long)]
    sender: Option<String>,
    /// Only files of mail received at or after an RFC 3339 instant, YYYY-MM-DD day or span back such as -30d.
    #[arg(long, allow_hyphen_values = true)]
    since: Option<String>,
    /// Only files of mail received before this.
    #[arg(long, allow_hyphen_values = true)]
    until: Option<String>,
    /// Only files of mail carrying this tag.
    #[arg(long)]
    tag: Option<String>,
    /// List the largest files first, those of no recorded size last.
    #[arg(long)]
    largest_first: bool,
    /// Write the files found as a CSV table (RFC 4180), one row each, to this new file.
    #[arg(long)]
    csv: Option<PathBuf>,
    /// Read every file found from the provider and save it into this new directory as <message id>-<index>-<name>.
    #[arg(long, conflicts_with = "csv")]
    save: Option<PathBuf>,
}

/// What `attachment save` wrote: the attachment, the file, and the digest of
/// the bytes in it.
#[derive(Serialize)]
pub(super) struct SavedAttachment {
    message_id: Uuid,
    index: usize,
    filename: Option<String>,
    content_type: String,
    size_bytes: usize,
    sha256: String,
    path: PathBuf,
}

pub(super) async fn run_attachment(
    state: AppState,
    organization: &str,
    command: AttachmentCommand,
) -> Result<(), AppError> {
    match command {
        AttachmentCommand::List(ListArgs { message }) => {
            print_json(&state.list_attachments(organization, message).await?)
        }
        AttachmentCommand::Find(FindArgs { mailbox, name, content_type, over, sender, since, until, tag, largest_first, csv, save }) => {
            let mailbox_id = super::reading::chosen_mailbox(&state, organization, mailbox)?;
            let filter = FileFilter { mailbox_id, name, content_type, over, sender, since, until, tag, largest_first };
            if let Some(directory) = save {
                return all::save_found(&state, organization, state.find_files(organization, &filter)?, directory).await;
            }
            let Some(path) = csv else {
                return print_json(&state.find_files(organization, &filter)?);
            };
            let file = NewFile { path: &path, exists: "ATTACHMENT_OUTPUT_EXISTS", invalid: "ATTACHMENT_OUTPUT_INVALID" };
            file.refuse_occupied()?;
            let files = state.find_files(organization, &filter)?;
            let table = file_table(&files);
            file.write(table.as_bytes())?;
            print_json(&serde_json::json!({ "path": path, "files": files.len(), "size_bytes": table.len() }))
        }
        AttachmentCommand::Save(SaveArgs { message, all: true, output, .. }) => {
            all::save_all(&state, organization, message, output).await
        }
        AttachmentCommand::Save(SaveArgs { index: None, .. }) => Err(AppError::invalid(
            "ATTACHMENT_INDEX_INVALID",
            "name the attachment with --index, as message attachment list prints it, or save every one with --all",
        )),
        AttachmentCommand::Save(SaveArgs {
            message,
            index: Some(index),
            output,
            ..
        }) => {
            let file = NewFile {
                path: &output,
                exists: "ATTACHMENT_OUTPUT_EXISTS",
                invalid: "ATTACHMENT_OUTPUT_INVALID",
            };
            // An occupied path is refused before the provider is read.
            file.refuse_occupied()?;
            let content = state.read_attachment(organization, message, index).await?;
            file.write(&content.bytes)?;
            print_json(&SavedAttachment {
                message_id: message,
                index,
                filename: content.attachment.filename,
                content_type: content.attachment.content_type,
                size_bytes: content.bytes.len(),
                sha256: format!("{:x}", Sha256::digest(&content.bytes)),
                path: output,
            })
        }
    }
}
