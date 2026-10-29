//! Forwarding every message a saved view lists, such as every invoice to an
//! accountant: each message is its own forward, claimed by its own key, so
//! the same request again forwards nothing twice, and one message that
//! cannot be forwarded is named with its refusal while the others still are.

use super::{
    super::{outbound::checked_key, templates::PerMessage},
    AppState, ForwardRequest,
};
use crate::{error::AppError, mail::OutgoingAttachment, models::OutboundMessage};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use uuid::Uuid;

/// What `message forward --view` sends: to whom, the operator's text as a
/// body or a template filled for each message, files sent after each
/// original, and the key each forward's own key is drawn from.
#[derive(Debug, Clone, Deserialize)]
pub struct ForwardViewRequest {
    pub idempotency_key: String,
    pub to: Vec<String>,
    #[serde(default)]
    pub cc: Vec<String>,
    #[serde(default)]
    pub bcc: Vec<String>,
    /// The text sent above every original; or, instead, `template`.
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
    pub allow_bounced: bool,
    #[serde(default)]
    pub follow_up: Option<String>,
}

/// What forwarding a view's messages did.
#[derive(Debug, Serialize)]
pub struct ForwardedView {
    pub view: String,
    /// The outbound row of every forward sent or attempted, in the view's order.
    pub forwarded: Vec<OutboundMessage>,
    /// Every message not forwarded, and why.
    pub refused: Vec<RefusedMessage>,
}

/// One message of a view that was not sent on or answered, and why.
#[derive(Debug, Serialize)]
pub struct RefusedMessage {
    pub message_id: Uuid,
    pub code: &'static str,
    pub message: String,
}

impl AppState {
    /// Forward every message the saved view `view` lists, each from the
    /// mailbox that received it, as `forward_message` forwards one, claimed
    /// by `<idempotency_key>-<message id>`. A key that is blank or holds
    /// whitespace, both or neither of `body` and `template`, and an unknown
    /// view are refused before anything is sent; a message whose forward is
    /// refused (its original no longer at the provider, a recipient
    /// suppressed or bounced, the send limit reached) is named in `refused`.
    pub async fn forward_view(
        &self,
        organization_id: &str,
        view: &str,
        request: ForwardViewRequest,
    ) -> Result<ForwardedView, AppError> {
        let key = checked_key(&request.idempotency_key)?.to_string();
        let text = PerMessage::chosen(
            &request.body,
            &request.template,
            &request.values,
            "FORWARD_BODY_INVALID",
        )?;
        let messages = self.list_view(organization_id, view, None, None)?;
        let mut outcome = ForwardedView {
            view: view.trim().to_string(),
            forwarded: Vec::new(),
            refused: Vec::new(),
        };
        for message in messages {
            match self
                .forward_listed(organization_id, message.id, &key, &text, &request)
                .await
            {
                Ok(sent) => outcome.forwarded.push(sent),
                Err(error) => outcome.refused.push(RefusedMessage {
                    message_id: message.id,
                    code: error.code,
                    message: error.message,
                }),
            }
        }
        Ok(outcome)
    }

    /// Forward one listed message with its own key and text.
    async fn forward_listed(
        &self,
        organization_id: &str,
        message_id: Uuid,
        key: &str,
        text: &PerMessage<'_>,
        request: &ForwardViewRequest,
    ) -> Result<OutboundMessage, AppError> {
        let body = text.for_message(self, organization_id, message_id)?;
        let one = ForwardRequest {
            idempotency_key: format!("{key}-{message_id}"),
            to: request.to.clone(),
            cc: request.cc.clone(),
            bcc: request.bcc.clone(),
            body,
            attachments: request.attachments.clone(),
            allow_bounced: request.allow_bounced,
            follow_up: request.follow_up.clone(),
        };
        self.forward_message(organization_id, message_id, one).await
    }
}
