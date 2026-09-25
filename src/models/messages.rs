//! Messages, the replies and outbound mail sent from a mailbox, and the summaries the API reports.

use super::ParseModelError;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: Uuid,
    pub mailbox_id: Uuid,
    pub external_uid: u32,
    pub message_id: Option<String>,
    pub in_reply_to: Option<String>,
    pub references: Option<String>,
    pub sender: String,
    pub reply_to: Option<String>,
    pub recipients: String,
    pub subject: String,
    pub sent_at: Option<String>,
    pub received_at: String,
    pub body_text: String,
    pub snippet: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NewMessage {
    pub external_uid: u32,
    pub message_id: Option<String>,
    pub in_reply_to: Option<String>,
    pub references: Option<String>,
    pub sender: String,
    pub reply_to: Option<String>,
    pub recipients: String,
    pub subject: String,
    pub sent_at: Option<String>,
    pub body_text: String,
    pub snippet: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplyAttempt {
    pub id: Uuid,
    pub message_id: Uuid,
    pub idempotency_key: String,
    pub body: String,
    pub status: DeliveryStatus,
    pub provider_message_id: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub sent_at: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryStatus {
    Pending,
    Sending,
    Sent,
    Failed,
    Uncertain,
}

impl DeliveryStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Sending => "sending",
            Self::Sent => "sent",
            Self::Failed => "failed",
            Self::Uncertain => "uncertain",
        }
    }
}

impl std::str::FromStr for DeliveryStatus {
    type Err = ParseModelError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "pending" => Ok(Self::Pending),
            "sending" => Ok(Self::Sending),
            "sent" => Ok(Self::Sent),
            "failed" => Ok(Self::Failed),
            "uncertain" => Ok(Self::Uncertain),
            _ => Err(ParseModelError(format!("unknown delivery status: {value}"))),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateReplyRequest {
    pub idempotency_key: String,
    pub body: String,
}

/// Mail this mailbox originates rather than answers. It carries its own
/// recipients and subject because no inbound message supplies them, and it
/// walks the same delivery states as a reply attempt: one idempotency key,
/// one provider mutation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboundMessage {
    pub id: Uuid,
    pub mailbox_id: Uuid,
    pub idempotency_key: String,
    pub recipients: String,
    pub cc: Option<String>,
    pub subject: String,
    pub body: String,
    pub status: DeliveryStatus,
    pub provider_message_id: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub sent_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateOutboundRequest {
    pub idempotency_key: String,
    pub to: Vec<String>,
    #[serde(default)]
    pub cc: Vec<String>,
    pub subject: String,
    pub body: String,
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
    /// What Skarbiec's declarations changed before the mailboxes were polled.
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
    pub database_path: String,
    pub schema_version: u32,
    pub mailbox_count: usize,
    pub enabled_mailbox_count: usize,
    pub message_count: usize,
    pub poll_interval_seconds: u64,
    pub skarbiec_available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub product: &'static str,
    pub version: &'static str,
    pub schema_version: u32,
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
