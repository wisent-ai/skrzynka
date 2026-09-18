use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

/// The bounds a mailbox profile is held to: how often it may be polled, and how long its
/// display name, its host names (RFC 1035) and its Skarbiec item id may be.
pub const MIN_POLL_INTERVAL_SECONDS: u64 = 15;
pub const MAX_POLL_INTERVAL_SECONDS: u64 = 86_400;
pub const MAX_DISPLAY_NAME_CHARS: usize = 200;
pub const MAX_HOST_LENGTH: usize = 253;
pub const MAX_ITEM_ID_LENGTH: usize = 256;
/// The bounds a reply or outbound request is held to: its idempotency key, its subject and
/// its body, which is also the largest body file the CLI reads.
pub const MAX_IDEMPOTENCY_KEY_LENGTH: usize = 200;
pub const MAX_SUBJECT_CHARS: usize = 500;
pub const MAX_BODY_BYTES: usize = 256 * 1024;

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
    pub poll_interval_seconds: u64,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateMailboxRequest {
    pub skarbiec_item_id: String,
    pub display_name: Option<String>,
    pub email: Option<String>,
    pub imap_host: Option<String>,
    pub imap_port: Option<u16>,
    pub smtp_host: Option<String>,
    pub smtp_port: Option<u16>,
    pub smtp_security: Option<SmtpSecurity>,
    pub poll_interval_seconds: Option<u64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MailboxImportState {
    Imported,
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
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UpdateMailboxRequest {
    pub display_name: Option<String>,
    pub email: Option<String>,
    pub imap_host: Option<String>,
    pub imap_port: Option<u16>,
    pub smtp_host: Option<String>,
    pub smtp_port: Option<u16>,
    pub smtp_security: Option<SmtpSecurity>,
    pub poll_interval_seconds: Option<u64>,
    pub enabled: Option<bool>,
}

mod messages;

pub use messages::{
    CreateOutboundRequest, CreateReplyRequest, DeliveryStatus, HealthResponse, MailboxSyncResult,
    Message, NewMessage, OutboundMessage, ReplyAttempt, SkarbiecItemMetadata, StatusResponse,
    SyncAllSummary, SyncSummary,
};
