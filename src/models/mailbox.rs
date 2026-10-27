//! A mailbox's recovery guidance, and what a sync and the service report.

use super::Mailbox;
use serde::{Deserialize, Serialize};
use std::path::Path;
use uuid::Uuid;

impl Mailbox {
    /// Reconstruct guidance from the stored failure, without contacting a provider
    /// or claiming that a readable credential has authenticated. A credential
    /// failure names the Skarbiec executable the resolver actually runs
    /// (`--skarbiec-bin`); with an explicit credentials file there is no
    /// Skarbiec to ask, and `credential_source` names that file instead.
    pub(crate) fn recovery_command(&self, skarbiec: Option<&Path>) -> Option<String> {
        let code = self.last_error_code.as_deref()?;
        match code {
            "SKARBIEC_ITEM_INVALID" | "SKARBIEC_UNAVAILABLE" | "SKARBIEC_RESPONSE_INVALID" => {
                let binary = skarbiec?.display().to_string();
                Some(format!("{} status", crate::gmail::shell_quoted(&binary)))
            }
            // Google refused the bound login's password for IMAP: Weles signs
            // that same login in and mints the app password, instead of an
            // operator pasting one on stdin.
            "GMAIL_IMAP_PASSWORD_REJECTED" => Some(crate::gmail::app_password_action_for_login(
                &self.organization_id,
                &self.skarbiec_item_id,
                &self.email,
            )),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncSummary {
    pub mailbox_id: Uuid,
    pub received: usize,
    pub skipped: usize,
    pub last_uid: u32,
    pub completed_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncAllSummary {
    pub completed_at: String,
    /// What Skarbiec's declarations changed before the mailboxes were read.
    pub reconciliation: super::MailboxReconciliation,
    pub mailboxes: Vec<MailboxSyncResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MailboxSyncResult {
    pub mailbox_id: Uuid,
    pub ok: bool,
    pub summary: Option<SyncSummary>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusResponse {
    pub product: &'static str,
    pub version: &'static str,
    /// The fleet database Skrzynka's state lives in, as Stado names it.
    pub database: &'static str,
    pub schema_version: u32,
    pub mailbox_count: usize,
    pub enabled_mailbox_count: usize,
    pub message_count: usize,
    pub skarbiec_available: bool,
    /// Why `skarbiec version` did not answer (the failed command, its exit
    /// status and what it wrote, or why it never ran); null while it answers.
    pub skarbiec_unavailable: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub product: &'static str,
    pub version: &'static str,
    pub schema_version: u32,
    /// Why mailbox announcements stopped reaching the service, when they
    /// have; null while every declared mailbox is taken up as it is declared.
    pub watching_stopped: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkarbiecItemMetadata {
    pub id: String,
    pub kind: Option<String>,
    pub state: Option<String>,
    pub tags: Vec<String>,
    pub versions: Option<u64>,
    pub updated_at: Option<String>,
}

impl SkarbiecItemMetadata {
    pub(crate) fn matches_coordinate(&self, coordinate: &str) -> bool {
        if self.id == coordinate {
            return true;
        }
        coordinate.strip_prefix("role:").is_some_and(|role| {
            self.tags
                .iter()
                .any(|tag| tag.strip_prefix("stado:role:") == Some(role))
        })
    }
}
