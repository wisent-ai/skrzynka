//! Answering every message a saved view lists, such as acknowledging every
//! application or enquiry at once: each message gets its own reply, claimed
//! by its own key, so the same request again sends nothing twice, and one
//! message that cannot be answered is named with its refusal while the
//! others still are.

use super::super::{forward::RefusedMessage, outbound::checked_key, templates::PerMessage, AppState};
use crate::{
    error::AppError,
    mail::OutgoingAttachment,
    models::{CreateReplyRequest, ReplyAttempt},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

/// What `message reply --view` sends to each message: its text as a body or
/// a template filled for that message, files, copies, and the key each
/// reply's own key is drawn from.
#[derive(Debug, Clone, Deserialize)]
pub struct ReplyViewRequest {
    pub idempotency_key: String,
    /// The text of every reply; or, instead, `template`.
    #[serde(default)]
    pub body: Option<String>,
    /// A template filled for each message, its `{{sender}}`, `{{sender_name}}`
    /// and `{{subject}}` from that message where `values` gives none.
    #[serde(default)]
    pub template: Option<String>,
    #[serde(default)]
    pub values: BTreeMap<String, String>,
    #[serde(default)]
    pub attachments: Vec<OutgoingAttachment>,
    #[serde(default)]
    pub reply_all: bool,
    #[serde(default)]
    pub quote: bool,
    #[serde(default)]
    pub cc: Vec<String>,
    #[serde(default)]
    pub bcc: Vec<String>,
}

/// What answering a view's messages did.
#[derive(Debug, Serialize)]
pub struct RepliedView {
    pub view: String,
    /// The attempt of every reply sent or tried, in the view's order.
    pub replied: Vec<ReplyAttempt>,
    /// Every message not answered, and why.
    pub refused: Vec<RefusedMessage>,
}

impl AppState {
    /// Reply to every message the saved view `view` lists, each as `reply`
    /// answers one, claimed by `<idempotency_key>-<message id>`. A key that is
    /// blank or holds whitespace, both or neither of `body` and `template`,
    /// and an unknown view are refused before anything is sent; a message
    /// whose reply is refused (a placeholder nothing fills for it, a
    /// recipient suppressed, the send limit reached) is named in `refused`.
    pub async fn reply_view(&self, organization_id: &str, view: &str, request: ReplyViewRequest) -> Result<RepliedView, AppError> {
        let key = checked_key(&request.idempotency_key)?.to_string();
        let text = PerMessage::chosen(&request.body, &request.template, &request.values, "REPLY_BODY_INVALID")?;
        let messages = self.list_view(organization_id, view, None, None)?;
        let mut outcome = RepliedView { view: view.trim().to_string(), replied: Vec::new(), refused: Vec::new() };
        for message in messages {
            match self.answer_listed(organization_id, message.id, &key, &text, &request).await {
                Ok(attempt) => outcome.replied.push(attempt),
                Err(error) => outcome.refused.push(RefusedMessage { message_id: message.id, code: error.code, message: error.message }),
            }
        }
        Ok(outcome)
    }

    /// Reply to one listed message with its own key and text.
    async fn answer_listed(
        &self,
        organization_id: &str,
        message_id: Uuid,
        key: &str,
        text: &PerMessage<'_>,
        request: &ReplyViewRequest,
    ) -> Result<ReplyAttempt, AppError> {
        let one = CreateReplyRequest {
            idempotency_key: format!("{key}-{message_id}"),
            body: text.for_message(self, organization_id, message_id)?,
            attachments: request.attachments.clone(),
            reply_all: request.reply_all,
            quote: request.quote,
            cc: request.cc.clone(),
            bcc: request.bcc.clone(),
            automatic: false,
        };
        self.reply(organization_id, message_id, one).await
    }
}
