//! The states `mailbox list --state` keeps, as Skrzynka Desktop's Mailboxes
//! facets name them, decided here so both read the same mailboxes.

use crate::models::Mailbox;
use serde::Deserialize;

/// One state a mailbox is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum MailboxState {
    /// Declared, and its last sync recorded an error.
    Failing,
    /// No longer declared in Skarbiec: not read until declared again.
    Undeclared,
    /// Never synchronized yet.
    Unsynchronized,
}

impl MailboxState {
    /// Whether `mailbox` is in this state now.
    pub(in crate::service) fn holds(self, mailbox: &Mailbox) -> bool {
        match self {
            Self::Failing => mailbox.enabled && mailbox.last_error_code.is_some(),
            Self::Undeclared => !mailbox.enabled,
            Self::Unsynchronized => mailbox.last_sync_at.is_none(),
        }
    }
}
