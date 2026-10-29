//! Putting a whole message back into a provider folder with IMAP `APPEND`
//! (RFC 3501 section 6.3.11): how a mailbox exported with `message export`
//! is restored. A message whose Message-ID the folder already holds is not
//! appended again, so restoring the same export twice leaves one copy.

use super::super::{dependency_error, incoming::open_inbox};
use crate::{error::AppError, models::Mailbox, skarbiec::ResolvedCredentials};
use serde::Serialize;

/// What one append did: the folder, the message's Message-ID as its header
/// states it (absent when it states none, so no copy could be looked for),
/// and whether it was appended or the folder already held it.
#[derive(Debug, Clone, Serialize)]
pub struct AppendOutcome {
    pub folder: String,
    pub message_id: Option<String>,
    pub appended: bool,
}

/// Append `source` (a whole RFC 5322 message) to `folder`, unless the folder
/// already holds a message with the same Message-ID.
pub fn append_message(
    mailbox: &Mailbox,
    credentials: &ResolvedCredentials,
    folder: &str,
    source: &[u8],
) -> Result<AppendOutcome, AppError> {
    let invalid = |message: String| AppError::invalid("MESSAGE_APPEND_INVALID", message);
    let (headers, _) = mailparse::parse_headers(source).map_err(|error| {
        invalid(format!(
            "the message does not read as RFC 5322 headers: {error}"
        ))
    })?;
    if headers.is_empty() {
        return Err(invalid(
            "the message states no header; it is not a whole message".to_string(),
        ));
    }
    let message_id = headers
        .iter()
        .find(|header| header.get_key_ref().eq_ignore_ascii_case("Message-ID"))
        .map(|header| header.get_value().trim().to_string())
        .filter(|value| !value.is_empty());
    let failed = |command: String, error: imap::Error| {
        dependency_error(
            "IMAP_APPEND_FAILED",
            format!(
                "IMAP {command} for {} at {}:{} failed: {error:?}",
                mailbox.email, mailbox.imap_host, mailbox.imap_port
            ),
            matches!(error, imap::Error::Io(_) | imap::Error::ConnectionLost),
        )
    };
    let mut session = open_inbox(mailbox, credentials)?;
    session
        .select(folder)
        .map_err(|error| failed(format!("SELECT {folder:?}"), error))?;
    if let Some(id) = message_id.as_deref() {
        let quoted = id.replace('\\', "\\\\").replace('"', "\\\"");
        let query = format!("HEADER Message-ID \"{quoted}\"");
        let held = session
            .uid_search(&query)
            .map_err(|error| failed(format!("UID SEARCH {query}"), error))?;
        if !held.is_empty() {
            logout(&mut session, mailbox);
            return Ok(AppendOutcome {
                folder: folder.to_string(),
                message_id,
                appended: false,
            });
        }
    }
    session
        .append(folder, source)
        .finish()
        .map_err(|error| failed(format!("APPEND {folder:?}"), error))?;
    logout(&mut session, mailbox);
    Ok(AppendOutcome {
        folder: folder.to_string(),
        message_id,
        appended: true,
    })
}

/// The work is done; a LOGOUT the server does not answer cleanly changes
/// none of it and is said, not dropped.
fn logout(session: &mut imap::Session<imap::Connection>, mailbox: &Mailbox) {
    if let Err(error) = session.logout() {
        tracing::warn!(%error, host = %mailbox.imap_host, "IMAP LOGOUT after APPEND failed");
    }
}
