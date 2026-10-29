//! Blocked senders: addresses, or `@domain` for every address of a domain,
//! whose mail listings leave out unless asked to include it. Nothing is
//! deleted and the provider is not told; unblocking shows the mail again.

use super::{messages::entry_address, AppState};
use crate::{error::AppError, models::Message};
use serde::Serialize;

mod import;
mod table;
pub use table::blocked_table;

/// What `message block` and `message unblock` answer: every blocked entry.
#[derive(Debug, Serialize)]
pub struct BlockedSenders {
    pub blocked: Vec<String>,
}

/// One blocked entry and how many stored messages it leaves out of listings
/// now, as `message blocked --count` prints it.
#[derive(Debug, Serialize)]
pub struct BlockedCount {
    pub entry: String,
    pub messages: usize,
}

impl AppState {
    /// Every blocked entry, or with `contains` those holding the text, case
    /// aside (a blank one is `MESSAGE_FILTER_INVALID`).
    pub fn blocked_senders(
        &self,
        organization_id: &str,
        contains: Option<&str>,
    ) -> Result<BlockedSenders, AppError> {
        Ok(BlockedSenders {
            blocked: self.blocked_entries(organization_id, contains)?,
        })
    }

    /// Every blocked entry, or those `contains` keeps, with how many stored
    /// messages of the organization it leaves out of listings now; a message
    /// an address and its domain both block counts under each.
    pub fn blocked_counts(
        &self,
        organization_id: &str,
        contains: Option<&str>,
    ) -> Result<Vec<BlockedCount>, AppError> {
        let entries = self.blocked_entries(organization_id, contains)?;
        let messages = self
            .database
            .list_messages(organization_id, None, None, None)?;
        Ok(entries
            .into_iter()
            .map(|entry| {
                let alone = [entry];
                let messages = messages
                    .iter()
                    .filter(|message| is_blocked(message, &alone))
                    .count();
                let [entry] = alone;
                BlockedCount { entry, messages }
            })
            .collect())
    }

    /// The blocked entries holding `contains`, case aside, or every one.
    fn blocked_entries(
        &self,
        organization_id: &str,
        contains: Option<&str>,
    ) -> Result<Vec<String>, AppError> {
        let contains = super::messages::filter_value(contains, "contains")?;
        Ok(self
            .database
            .blocked_senders(organization_id)?
            .into_iter()
            .filter(|entry| contains.as_deref().is_none_or(|part| entry.contains(part)))
            .collect())
    }

    /// Block an address or `@domain`, stored trimmed and in lower case.
    pub fn block_sender(
        &self,
        organization_id: &str,
        address: &str,
    ) -> Result<BlockedSenders, AppError> {
        let entry = blocked_entry(address)?;
        self.database.block_sender(organization_id, &entry)?;
        self.blocked_senders(organization_id, None)
    }

    /// Unblock an entry; one that is not blocked is refused with the list.
    pub fn unblock_sender(
        &self,
        organization_id: &str,
        address: &str,
    ) -> Result<BlockedSenders, AppError> {
        let entry = blocked_entry(address)?;
        let blocked = self.database.blocked_senders(organization_id)?;
        if !blocked.contains(&entry) {
            return Err(AppError::new(
                axum::http::StatusCode::NOT_FOUND,
                "BLOCKED_SENDER_NOT_FOUND",
                format!(
                    "{entry} is not blocked; blocked are: {}",
                    blocked.join(", ")
                ),
                false,
            ));
        }
        self.database.unblock_sender(organization_id, &entry)?;
        self.blocked_senders(organization_id, None)
    }
}

/// An entry as stored: a whole address, or `@domain`, trimmed and lower case.
pub(in crate::service) fn blocked_entry(written: &str) -> Result<String, AppError> {
    let entry = written.trim().to_lowercase();
    let valid = match entry.split_once('@') {
        Some((local, domain)) => {
            !domain.is_empty()
                && !domain.contains('@')
                && !entry
                    .chars()
                    .any(|character| character.is_whitespace() || character == ',')
                && (local.is_empty() || !local.contains('<'))
        }
        None => false,
    };
    if !valid {
        return Err(AppError::invalid(
            "BLOCKED_SENDER_INVALID",
            format!("{written:?} is neither an address nor @domain; write one, such as news@example.com or @example.com"),
        ));
    }
    Ok(entry)
}

/// Whether `message` comes from a blocked sender: its From address is an
/// entry, or ends with an `@domain` entry.
pub(super) fn is_blocked(message: &Message, blocked: &[String]) -> bool {
    let address = entry_address(&message.sender).to_lowercase();
    blocked.iter().any(|entry| {
        *entry == address || (entry.starts_with('@') && address.ends_with(entry.as_str()))
    })
}
