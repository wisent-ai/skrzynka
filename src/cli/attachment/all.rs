//! Saving many attachments into a new directory, each read from the
//! provider: every file one message carries (`message attachment save <ID>
//! --all --output <DIRECTORY>`), or every file `message attachment find`
//! keeps (`… find … --save <DIRECTORY>`), named so none lands on another or
//! outside the directory.

use super::super::{output::NewFile, print_json};
use super::SavedAttachment;
use crate::{
    error::AppError,
    service::{AppState, StoredFile},
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// One file to save: the message carrying it, its index among the
/// message's attachments, and the file it is written to.
struct Wanted {
    message_id: Uuid,
    index: usize,
    path: PathBuf,
}

/// One attachment that could not be read or written, and why.
#[derive(Serialize)]
struct NotSaved {
    message_id: Uuid,
    index: usize,
    code: &'static str,
    message: String,
}

/// What a save of many files wrote.
#[derive(Serialize)]
struct SavedFiles {
    directory: PathBuf,
    saved: Vec<SavedAttachment>,
    refused: Vec<NotSaved>,
}

impl SavedFiles {
    /// `ATTACHMENT_SAVE_INCOMPLETE` when any file was refused, to end the
    /// command with after the listing has been printed.
    fn outcome(&self) -> Result<(), AppError> {
        if self.refused.is_empty() {
            return Ok(());
        }
        Err(AppError::invalid(
            "ATTACHMENT_SAVE_INCOMPLETE",
            format!(
                "{} of the files could not be saved; each is listed under refused",
                self.refused.len()
            ),
        ))
    }
}

/// What `attachment save --all` wrote: the files of one message.
#[derive(Serialize)]
struct SavedAll<'a> {
    message_id: Uuid,
    #[serde(flatten)]
    files: &'a SavedFiles,
}

/// Create `output` and write every attachment of `message` into it as
/// `<index>-<name>`. One that cannot be read does not stop the others; any
/// refused ends with `ATTACHMENT_SAVE_INCOMPLETE` after printing.
pub(super) async fn save_all(
    state: &AppState,
    organization: &str,
    message: Uuid,
    output: PathBuf,
) -> Result<(), AppError> {
    let attachments = state.list_attachments(organization, message).await?;
    create(&output)?;
    let wanted = attachments
        .into_iter()
        .map(|attachment| Wanted {
            message_id: message,
            index: attachment.index,
            path: output.join(file_name(attachment.index, attachment.filename.as_deref())),
        })
        .collect();
    let files = save(state, organization, output, wanted).await;
    print_json(&SavedAll {
        message_id: message,
        files: &files,
    })?;
    files.outcome()
}

/// Create `output` and write every file `attachment find` kept into it as
/// `<message id>-<index>-<name>`, so files of several messages never meet.
/// None kept is `NOT_FOUND` before the directory is created; one that
/// cannot be read does not stop the others, and any refused ends with
/// `ATTACHMENT_SAVE_INCOMPLETE` after printing.
pub(super) async fn save_found(
    state: &AppState,
    organization: &str,
    found: Vec<StoredFile>,
    output: PathBuf,
) -> Result<(), AppError> {
    if found.is_empty() {
        return Err(AppError::not_found("attachment"));
    }
    create(&output)?;
    let wanted = found
        .into_iter()
        .map(|file| Wanted {
            message_id: file.message_id,
            index: file.index,
            path: output.join(format!(
                "{}-{}",
                file.message_id,
                file_name(file.index, file.filename.as_deref())
            )),
        })
        .collect();
    let files = save(state, organization, output, wanted).await;
    print_json(&files)?;
    files.outcome()
}

/// The new directory the files go into; an existing path is never written into.
fn create(output: &Path) -> Result<(), AppError> {
    std::fs::create_dir(output).map_err(|error| match error.kind() {
        std::io::ErrorKind::AlreadyExists => AppError::invalid(
            "ATTACHMENT_OUTPUT_EXISTS",
            format!("{} already exists; saving never writes into an existing path: name one that does not exist", output.display()),
        ),
        _ => AppError::invalid("ATTACHMENT_OUTPUT_INVALID", format!("{} could not be created: {error}", output.display())),
    })
}

/// Read each wanted file from the provider and write it to its path.
async fn save(
    state: &AppState,
    organization: &str,
    directory: PathBuf,
    wanted: Vec<Wanted>,
) -> SavedFiles {
    let mut done = SavedFiles {
        directory,
        saved: Vec::new(),
        refused: Vec::new(),
    };
    for Wanted {
        message_id,
        index,
        path,
    } in wanted
    {
        let written = match state.read_attachment(organization, message_id, index).await {
            Ok(content) => NewFile {
                path: &path,
                exists: "ATTACHMENT_OUTPUT_EXISTS",
                invalid: "ATTACHMENT_OUTPUT_INVALID",
            }
            .write(&content.bytes)
            .map(|()| content),
            Err(error) => Err(error),
        };
        match written {
            Ok(content) => done.saved.push(SavedAttachment {
                message_id,
                index,
                filename: content.attachment.filename,
                content_type: content.attachment.content_type,
                size_bytes: content.bytes.len(),
                sha256: format!("{:x}", Sha256::digest(&content.bytes)),
                path,
            }),
            Err(error) => done.refused.push(NotSaved {
                message_id,
                index,
                code: error.code,
                message: error.message,
            }),
        }
    }
    done
}

/// `<index>-<name>`, the stated name's last path component, or
/// `<index>-attachment` for a file that states none.
fn file_name(index: usize, stated: Option<&str>) -> String {
    let name = stated
        .and_then(|name| Path::new(name).file_name())
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty());
    match name {
        Some(name) => format!("{index}-{name}"),
        None => format!("{index}-attachment"),
    }
}
