//! `skrzynka message restore`: put the originals a `message export` wrote
//! back into one of the mailbox's provider folders, each checked against the
//! manifest's SHA-256 first, none appended twice.

use super::{
    super::print_json,
    check::{checked_file, read_manifest, FileFault},
};
use crate::{error::AppError, service::AppState};
use serde::Serialize;
use std::path::PathBuf;
use uuid::Uuid;

/// What `message restore` reads and where it puts it.
#[derive(clap::Args)]
pub(in crate::cli) struct RestoreArgs {
    /// Mailbox ID, as `skrzynka mailbox list` prints it, or the mailbox's own address.
    #[arg(long)]
    mailbox: String,
    /// A directory `skrzynka message export` wrote, holding its manifest.json.
    #[arg(long)]
    input: PathBuf,
    /// The provider folder to put the messages in, as `mailbox folders` lists it.
    #[arg(long)]
    folder: String,
}

/// One file put back, or found already there.
#[derive(Serialize)]
struct Restored {
    file: String,
    message_id: Option<String>,
    appended: bool,
}

/// One file not put back, and why.
#[derive(Serialize)]
struct NotRestored {
    file: String,
    code: &'static str,
    message: String,
}

#[derive(Serialize)]
struct RestoreReport {
    mailbox_id: Uuid,
    folder: String,
    restored: Vec<Restored>,
    refused: Vec<NotRestored>,
}

pub(in crate::cli) async fn restore(
    state: &AppState,
    organization: &str,
    RestoreArgs {
        mailbox,
        input,
        folder,
    }: RestoreArgs,
) -> Result<(), AppError> {
    let mailbox = state.resolve_mailbox(organization, &mailbox)?.id;
    let manifest = read_manifest(&input, "RESTORE_INPUT_INVALID")?;
    let mut report = RestoreReport {
        mailbox_id: mailbox,
        folder: folder.trim().to_string(),
        restored: Vec::new(),
        refused: Vec::new(),
    };
    for entry in manifest.exported {
        let refuse = |code: &'static str, message: String| NotRestored {
            file: entry.file.clone(),
            code,
            message,
        };
        let source = match checked_file(&input, &entry) {
            Ok(source) => source,
            Err(FileFault::Unreadable(message)) => {
                report.refused.push(refuse("RESTORE_FILE_INVALID", message));
                continue;
            }
            Err(FileFault::Changed(message)) => {
                report.refused.push(refuse("RESTORE_FILE_CHANGED", message));
                continue;
            }
        };
        match state
            .append_message(organization, mailbox, &folder, source)
            .await
        {
            Ok(outcome) => report.restored.push(Restored {
                file: entry.file.clone(),
                message_id: outcome.message_id,
                appended: outcome.appended,
            }),
            Err(error) => report.refused.push(refuse(error.code, error.message)),
        }
    }
    print_json(&report)?;
    if report.refused.is_empty() {
        return Ok(());
    }
    Err(AppError::invalid(
        "RESTORE_INCOMPLETE",
        format!(
            "{} of the export's files were not restored; each is listed under refused",
            report.refused.len()
        ),
    ))
}
