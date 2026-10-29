//! Forwarding a stored message: a new message from the mailbox that received
//! it, carrying the operator's text and the original, whole as the provider
//! holds it, attached as `message/rfc822`. It is sent as originated mail, so
//! it is claimed by its idempotency key, counted against the mailbox's send
//! limit and read back with `message outbound`.

use super::AppState;
use crate::{
    error::AppError,
    mail::{prepare_attachments, OutgoingAttachment},
    models::{CreateOutboundRequest, OutboundMessage},
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::Deserialize;
use uuid::Uuid;

mod view;
pub use view::{ForwardViewRequest, RefusedMessage};

/// The prefix a forwarded message's subject carries before the original's.
const SUBJECT_PREFIX: &str = "Fwd:";
/// The type the original travels as (RFC 2046 section 5.2.1).
const ORIGINAL_TYPE: &str = "message/rfc822";

/// What `message forward` sends: to whom, the operator's own text, files sent
/// after the original, and the key that makes a repeated request the same send.
#[derive(Debug, Clone, Deserialize)]
pub struct ForwardRequest {
    pub idempotency_key: String,
    pub to: Vec<String>,
    #[serde(default)]
    pub cc: Vec<String>,
    /// Blind copies: sent to, never named in the headers.
    #[serde(default)]
    pub bcc: Vec<String>,
    pub body: String,
    /// Files of the operator's own, attached after the original.
    #[serde(default)]
    pub attachments: Vec<OutgoingAttachment>,
    /// Forward even to an address a stored bounce report says delivery to failed for good.
    #[serde(default)]
    pub allow_bounced: bool,
    /// Expect an answer by this RFC 3339 instant or span from now once sent.
    #[serde(default)]
    pub follow_up: Option<String>,
}

impl AppState {
    /// Forward stored message `message_id` from the mailbox that received
    /// it. The operator's files are checked first, so one that could never be
    /// sent is refused before the provider is read; the original is read from
    /// the provider now and must still be the stored message; the subject is
    /// the original's after `Fwd:`, or `Fwd:` alone for a message that states none.
    pub async fn forward_message(
        &self,
        organization_id: &str,
        message_id: Uuid,
        request: ForwardRequest,
    ) -> Result<OutboundMessage, AppError> {
        prepare_attachments(&request.attachments)?;
        // An unreadable follow-up is refused before the provider is read.
        request
            .follow_up
            .as_deref()
            .map(super::follow_ups::due_instant)
            .transpose()?;
        let message = self.database.get_message(organization_id, message_id)?;
        let source = self.message_source(organization_id, message_id).await?;
        let subject = match message.subject.as_deref().map(str::trim) {
            Some(stated) if !stated.is_empty() => format!("{SUBJECT_PREFIX} {stated}"),
            _ => SUBJECT_PREFIX.to_string(),
        };
        let original = OutgoingAttachment {
            filename: format!("{message_id}.eml"),
            content_type: ORIGINAL_TYPE.to_string(),
            content_base64: STANDARD.encode(&source),
        };
        self.send_outbound(
            organization_id,
            message.mailbox_id,
            CreateOutboundRequest {
                idempotency_key: request.idempotency_key,
                to: request.to,
                cc: request.cc,
                bcc: request.bcc,
                subject,
                body: request.body,
                attachments: std::iter::once(original)
                    .chain(request.attachments)
                    .collect(),
                allow_bounced: request.allow_bounced,
                follow_up: request.follow_up,
            },
        )
        .await
    }
}
