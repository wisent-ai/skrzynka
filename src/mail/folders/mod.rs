//! The folders a mailbox's provider holds, as IMAP `LIST` names them: the
//! names `message move --folder` takes, with what each is for when the
//! provider says (RFC 6154 special use).

use super::{dependency_error, incoming::open_inbox};
use crate::{error::AppError, models::Mailbox, skarbiec::ResolvedCredentials};
use imap_proto::NameAttribute;
use serde::{Deserialize, Serialize};

mod append;
pub use append::{append_message, AppendOutcome};

/// One folder as the provider lists it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderFolder {
    /// The name as the provider lists it, the one `message move` takes.
    pub name: String,
    /// The character the provider separates levels of names with; absent
    /// when its names are flat.
    pub delimiter: Option<String>,
    /// Whether the folder can hold messages; a `\Noselect` name only holds
    /// other folders, and a message cannot be moved to it.
    pub selectable: bool,
    /// What the provider says the folder is for (RFC 6154): `all`,
    /// `archive`, `drafts`, `flagged`, `junk`, `sent` or `trash`; absent
    /// when it says nothing.
    pub special_use: Option<String>,
    /// Every other attribute the provider gave, as it wrote it.
    pub attributes: Vec<String>,
}

/// Every folder the provider lists for the mailbox, in its order.
pub fn list_folders(
    mailbox: &Mailbox,
    credentials: &ResolvedCredentials,
) -> Result<Vec<ProviderFolder>, AppError> {
    let mut session = open_inbox(mailbox, credentials)?;
    let listed = session.list(Some(""), Some("*")).map_err(|error| {
        dependency_error(
            "IMAP_LIST_FAILED",
            format!(
                "IMAP LIST \"\" * for {} at {}:{} failed: {error:?}",
                mailbox.email, mailbox.imap_host, mailbox.imap_port
            ),
            matches!(error, imap::Error::Io(_) | imap::Error::ConnectionLost),
        )
    });
    let folders = listed.map(|names| names.iter().map(folder).collect());
    // The listing is read; a LOGOUT the server does not answer cleanly
    // changes none of it and is said, not dropped.
    if let Err(error) = session.logout() {
        tracing::warn!(%error, host = %mailbox.imap_host, "IMAP LOGOUT after LIST failed");
    }
    folders
}

fn folder(name: &imap::types::Name<'_>) -> ProviderFolder {
    let mut folder = ProviderFolder {
        name: name.name().to_string(),
        delimiter: name.delimiter().map(str::to_string),
        selectable: true,
        special_use: None,
        attributes: Vec::new(),
    };
    for attribute in name.attributes() {
        let special = |purpose: &str| Some(purpose.to_string());
        match attribute {
            NameAttribute::NoSelect => folder.selectable = false,
            NameAttribute::All => folder.special_use = special("all"),
            NameAttribute::Archive => folder.special_use = special("archive"),
            NameAttribute::Drafts => folder.special_use = special("drafts"),
            NameAttribute::Flagged => folder.special_use = special("flagged"),
            NameAttribute::Junk => folder.special_use = special("junk"),
            NameAttribute::Sent => folder.special_use = special("sent"),
            NameAttribute::Trash => folder.special_use = special("trash"),
            NameAttribute::NoInferiors => folder.attributes.push("\\Noinferiors".to_string()),
            NameAttribute::Marked => folder.attributes.push("\\Marked".to_string()),
            NameAttribute::Unmarked => folder.attributes.push("\\Unmarked".to_string()),
            NameAttribute::Extension(written) => folder.attributes.push(written.to_string()),
            // The type may grow attributes this build does not name; each is
            // still kept, as the library spells it.
            other => folder.attributes.push(format!("{other:?}")),
        }
    }
    folder
}
