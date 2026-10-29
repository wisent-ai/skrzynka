//! Messages, the replies and outbound mail sent from a mailbox, and the summaries the API reports.

use super::ParseModelError;
use crate::mail::{AuthenticationVerdict, BounceReport, MailingList, MessageAttachment};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// A message as Skrzynka stored it. `recipients` and `subject` are the
/// message's own `To` and `Subject` headers, absent when it states none:
/// nothing is written in for a header the sender never wrote. `sent_at` is
/// the instant its `Date` header names; a `Date` header that cannot be read
/// as one leaves `sent_at` absent and is kept, with why, in
/// `sent_at_unreadable`, so an undated message is told from a misdated one.
/// `attachments` is what the import recorded of the message's files and
/// `mailing_list` the list headers it states; both are absent for a message
/// imported before Skrzynka recorded them, which is not the same as a message
/// that has no files or comes from no list.
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
    pub recipients: Option<String>,
    /// The Cc header as written; absent when it states none or was imported before Cc was recorded.
    pub cc: Option<String>,
    pub subject: Option<String>,
    pub sent_at: Option<String>,
    pub sent_at_unreadable: Option<UnreadableDate>,
    pub received_at: String,
    pub body_text: String,
    pub snippet: String,
    pub attachments: Option<Vec<MessageAttachment>>,
    pub mailing_list: Option<MailingList>,
    /// The delivery status notification this message is, as its import
    /// recorded it; absent for any other message, and for one imported before
    /// reports were read.
    pub bounce: Option<BounceReport>,
    /// The receiving provider's `Authentication-Results` verdicts on the
    /// sender (SPF, DKIM, DMARC and others), as written; absent for a message
    /// imported before they were recorded.
    pub authentication: Option<Vec<AuthenticationVerdict>>,
    /// The operator's own labels on the message, in tag order; kept only in
    /// Skrzynka, never sent to the provider.
    pub tags: Vec<String>,
    /// The instant (RFC 3339, UTC) listings leave the message out until.
    pub snoozed_until: Option<String>,
    /// The operator's own note on the message; kept only in Skrzynka.
    pub note: Option<String>,
}

/// A `Date` header a message states but that does not read as an instant:
/// the header as the message wrote it, and why it could not be read.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct UnreadableDate {
    pub header: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NewMessage {
    pub external_uid: u32,
    pub message_id: Option<String>,
    pub in_reply_to: Option<String>,
    pub references: Option<String>,
    pub sender: String,
    pub reply_to: Option<String>,
    pub recipients: Option<String>,
    /// The Cc header as written, recorded with the message's other details.
    pub cc: Option<String>,
    pub subject: Option<String>,
    pub sent_at: Option<String>,
    pub sent_at_unreadable: Option<UnreadableDate>,
    pub body_text: String,
    pub snippet: String,
    pub attachments: Vec<MessageAttachment>,
    pub mailing_list: MailingList,
    /// The delivery status notification this message is, when it is one.
    pub bounce: Option<BounceReport>,
    /// The receiving provider's `Authentication-Results` verdicts, in header order.
    pub authentication: Vec<AuthenticationVerdict>,
}

impl NewMessage {
    /// What a `Date` header the message states but that does not read as an
    /// instant is kept as: the header as written and why it was not read.
    pub fn unreadable_date(header: String, error: String) -> UnreadableDate {
        UnreadableDate { header, error }
    }

    /// Whether `stored` is this provider message as Skrzynka stored it: the
    /// same UID, every header it kept and the same text. The derived snippet
    /// and the attachments are not compared; the caller compares those.
    pub fn is_stored_as(&self, stored: &Message) -> bool {
        self.external_uid == stored.external_uid
            && self.message_id == stored.message_id
            && self.in_reply_to == stored.in_reply_to
            && self.references == stored.references
            && self.sender == stored.sender
            && self.reply_to == stored.reply_to
            && self.recipients == stored.recipients
            && self.subject == stored.subject
            && self.sent_at == stored.sent_at
            && self.sent_at_unreadable == stored.sent_at_unreadable
            && self.body_text == stored.body_text
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplyAttempt {
    pub id: Uuid,
    pub message_id: Uuid,
    pub idempotency_key: String,
    pub body: String,
    /// The files the reply carries, by name, type, size and SHA-256; the
    /// bytes are not kept.
    pub attachments: Vec<crate::mail::OutboundAttachment>,
    pub status: DeliveryStatus,
    pub provider_message_id: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub sent_at: Option<String>,
    /// The mailbox's signature when the reply was claimed, sent after `body`.
    pub signature: Option<String>,
    /// The reply went to every To and Cc address of the message, not only its sender.
    pub reply_all: bool,
    /// The addresses the request copied openly, comma-separated; absent for none.
    #[serde(default)]
    pub cc: Option<String>,
    /// The blind copies the request named, comma-separated; never in a header.
    #[serde(default)]
    pub bcc: Option<String>,
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
    /// Files to attach, each a name, a MIME type and the bytes in base64.
    #[serde(default)]
    pub attachments: Vec<crate::mail::OutgoingAttachment>,
    /// Answer every To and Cc address of the message too, the mailbox's own aside.
    #[serde(default)]
    pub reply_all: bool,
    /// Quote the message's text below the reply, each line after `> `.
    #[serde(default)]
    pub quote: bool,
    /// More addresses to copy openly, bare; each recorded on the attempt.
    #[serde(default)]
    pub cc: Vec<String>,
    /// Blind copies: delivered to, named in no header of the reply.
    #[serde(default)]
    pub bcc: Vec<String>,
    /// Sent by the mailbox's automatic responder, never by a caller: the
    /// reply states `Auto-Submitted: auto-replied` (docs/replies/auto-reply.md).
    #[serde(skip)]
    pub automatic: bool,
}

/// Mail this mailbox originates: its own recipients and subject, walking the
/// delivery states of a reply attempt (one idempotency key, one provider mutation).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboundMessage {
    pub id: Uuid,
    pub mailbox_id: Uuid,
    pub idempotency_key: String,
    pub recipients: String,
    pub cc: Option<String>,
    /// Blind copies: envelope recipients the message's headers never name.
    pub bcc: Option<String>,
    pub subject: String,
    pub body: String,
    /// The files the message carries, by name, type, size and SHA-256; bytes are not kept.
    pub attachments: Vec<crate::mail::OutboundAttachment>,
    pub status: DeliveryStatus,
    pub provider_message_id: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub sent_at: Option<String>,
    /// The mailbox's signature when the message was claimed, sent after `body`.
    pub signature: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateOutboundRequest {
    pub idempotency_key: String,
    pub to: Vec<String>,
    #[serde(default)]
    pub cc: Vec<String>,
    #[serde(default)]
    pub bcc: Vec<String>,
    pub subject: String,
    pub body: String,
    /// Files to attach, each a name, a MIME type and the bytes in base64.
    #[serde(default)]
    pub attachments: Vec<crate::mail::OutgoingAttachment>,
    /// Send even to an address a stored bounce report says delivery to failed
    /// for good; without it such a send is refused before anything is claimed.
    #[serde(default)]
    pub allow_bounced: bool,
    /// Expect an answer by this RFC 3339 instant or span from now (`+3d`)
    /// once the send ends `sent`, as `message followup set` would.
    #[serde(default)]
    pub follow_up: Option<String>,
}

/// What a sync and the service report live with the mailbox they describe.
pub use super::mailbox::{
    HealthResponse, MailboxSyncResult, SkarbiecItemMetadata, StatusResponse, SyncAllSummary,
    SyncSummary,
};
