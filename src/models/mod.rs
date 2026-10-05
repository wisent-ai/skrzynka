use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

/// How long a mailbox's host names may be (RFC 1035). A mailbox is read when its
/// provider reports new mail (IMAP IDLE); no polling interval exists.
pub const MAX_HOST_LENGTH: usize = 253;

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct ParseModelError(pub String);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mailbox {
    pub id: Uuid,
    #[serde(skip_serializing)]
    pub organization_id: String,
    pub skarbiec_item_id: String,
    pub smtp_skarbiec_item_id: Option<String>,
    pub display_name: String,
    pub email: String,
    pub imap_host: String,
    pub imap_port: u16,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_security: SmtpSecurity,
    pub enabled: bool,
    pub last_uid: u32,
    pub last_sync_at: Option<String>,
    pub last_error_code: Option<String>,
    pub last_error_message: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl Mailbox {
    pub fn outbound_skarbiec_item_id(&self) -> &str {
        self.smtp_skarbiec_item_id
            .as_deref()
            .unwrap_or(&self.skarbiec_item_id)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SmtpSecurity {
    Starttls,
    Tls,
}

impl SmtpSecurity {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Starttls => "starttls",
            Self::Tls => "tls",
        }
    }
}

impl std::str::FromStr for SmtpSecurity {
    type Err = ParseModelError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.trim().to_ascii_lowercase().as_str() {
            "starttls" => Ok(Self::Starttls),
            "tls" => Ok(Self::Tls),
            _ => Err(ParseModelError(
                "smtp_security must be starttls or tls".to_string(),
            )),
        }
    }
}

/// Declare one Skarbiec item a mailbox: tag it in Skarbiec and import every
/// message its INBOX holds. The item supplies every profile value.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeclareMailboxRequest {
    pub skarbiec_item_id: String,
}

/// What one pass over Skarbiec's declared mailboxes changed in Skrzynka's
/// state. `refused` names declared items whose profile could not be read,
/// with the exact code and sentence.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MailboxReconciliation {
    pub declared: usize,
    pub created: Vec<Uuid>,
    pub updated: Vec<Uuid>,
    pub undeclared: Vec<Uuid>,
    pub refused: Vec<MailboxDeclarationRefusal>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MailboxDeclarationRefusal {
    pub skarbiec_item_id: String,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MailboxImportState {
    Imported,
    Updated,
    Unchanged,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ImportItemCounts {
    pub imported: usize,
    pub unchanged: usize,
    pub conflicting: usize,
    pub rejected: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct MailboxImportSource {
    pub kind: &'static str,
    pub skarbiec_item_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MailboxImportResult {
    pub applied: bool,
    pub source: MailboxImportSource,
    pub mailbox_state: MailboxImportState,
    pub mailbox: Mailbox,
    pub messages: ImportItemCounts,
    pub rejected_by_reason: BTreeMap<String, usize>,
}

mod messages;

pub use messages::{
    CreateOutboundRequest, CreateReplyRequest, DeliveryStatus, HealthResponse, MailboxSyncResult,
    Message, NewMessage, OutboundMessage, ReplyAttempt, SkarbiecItemMetadata, StatusResponse,
    SyncAllSummary, SyncSummary,
};
