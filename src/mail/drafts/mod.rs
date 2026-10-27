//! A draft: a message being written from one mailbox, kept only in
//! Skrzynka until it is sent. Every field but the mailbox may still be empty;
//! sending holds the draft to every rule `message send` holds a message to.

use super::{OutboundAttachment, OutgoingAttachment};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub mod placeholders;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Draft {
    pub id: Uuid,
    pub mailbox_id: Uuid,
    /// Recipient addresses, comma-separated as written.
    pub recipients: String,
    /// Copied addresses, comma-separated as written.
    pub cc: String,
    /// Blind copies, comma-separated as written: sent to, never named in the headers.
    pub bcc: String,
    pub subject: String,
    pub body: String,
    /// The files the draft carries, as they will be sent; kept with the draft
    /// until it is sent or deleted, and never printed.
    #[serde(skip)]
    pub files: Vec<OutgoingAttachment>,
    /// Each file's name, type, decoded size and SHA-256, as the outbound row
    /// of the send will record it.
    pub attachments: Vec<OutboundAttachment>,
    pub created_at: String,
    pub updated_at: String,
    /// When a running `skrzynka serve` sends the draft (RFC 3339), with
    /// `send_key` as its idempotency key; absent when it is not scheduled.
    pub send_at: Option<String>,
    pub send_key: Option<String>,
    /// Why the last scheduled send did not end `sent`, as `CODE: message`.
    pub schedule_error: Option<String>,
}

/// The fields a draft is created with or changed to; an absent field keeps
/// what the draft holds (empty for a new draft).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct DraftChange {
    pub to: Option<Vec<String>>,
    pub cc: Option<Vec<String>>,
    #[serde(default)]
    pub bcc: Option<Vec<String>>,
    pub subject: Option<String>,
    pub body: Option<String>,
    /// The files the draft is to carry, replacing the ones it holds; `[]`
    /// removes them.
    #[serde(default)]
    pub attachments: Option<Vec<OutgoingAttachment>>,
}

/// A text the operator keeps to reuse in replies and new messages, named
/// within the organization; kept only in Skrzynka.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageTemplate {
    pub name: String,
    /// The subject a new message from the template carries when the send
    /// names none; absent for a template of text alone.
    #[serde(default)]
    pub subject: Option<String>,
    pub body: String,
    pub updated_at: String,
    /// Each key the subject's and text's `{{key}}` placeholders name, in order
    /// of first appearance; a send that takes the template gives a value for each.
    #[serde(default)]
    pub placeholders: Vec<String>,
}
