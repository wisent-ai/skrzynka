//! Sending every draft a narrowed listing keeps now, such as a mail merge
//! reviewed and ready, each claimed by its own key so the same request again
//! sends nothing twice; a draft one send refuses does not stop the others.

use super::super::{outbound::checked_key, AppState, DraftFilter, DraftSendRequest};
use crate::{error::AppError, models::OutboundMessage};
use serde::Serialize;
use uuid::Uuid;

/// What sending the listed drafts did.
#[derive(Debug, Serialize)]
pub struct DraftsSent {
    /// The outbound row of every draft sent, in the listing's order; a draft
    /// whose send the provider accepted is removed.
    pub sent: Vec<OutboundMessage>,
    /// Every draft that was not sent, and why; it is kept.
    pub refused: Vec<RefusedDraft>,
}

/// One draft that was not sent: its ID and the refusal.
#[derive(Debug, Serialize)]
pub struct RefusedDraft {
    pub draft_id: Uuid,
    pub code: &'static str,
    pub message: String,
}

impl AppState {
    /// Send now every draft `filter` keeps whose text, addresses or subject
    /// contain its `contains`, each as `message draft send` sends one, claimed
    /// by `<idempotency_key>-<draft id>`, with `allow_bounced` and `follow_up`
    /// applied to every one. A blank key or a filter without `contains` is
    /// refused before anything is read (`IDEMPOTENCY_KEY_INVALID`,
    /// `MESSAGE_FILTER_INVALID`), so nothing sends every draft at once; one
    /// keeping no draft is `NOT_FOUND`. A draft a send refuses is named and
    /// kept; the others are still sent.
    pub async fn send_drafts(
        &self,
        organization_id: &str,
        filter: &DraftFilter,
        idempotency_key: &str,
        allow_bounced: bool,
        follow_up: Option<&str>,
    ) -> Result<DraftsSent, AppError> {
        let key = checked_key(idempotency_key)?.to_string();
        if filter.contains.is_none() {
            return Err(AppError::invalid(
                "MESSAGE_FILTER_INVALID",
                "name the text the drafts to send contain, or send one draft by its ID",
            ));
        }
        let listed = self.list_drafts(organization_id, filter)?;
        if listed.is_empty() {
            return Err(AppError::not_found("draft"));
        }
        let mut outcome = DraftsSent { sent: Vec::new(), refused: Vec::new() };
        for draft in listed {
            let request = DraftSendRequest {
                idempotency_key: format!("{key}-{}", draft.id),
                allow_bounced,
                follow_up: follow_up.map(String::from),
            };
            match self.send_draft(organization_id, draft.id, request).await {
                Ok(sent) => outcome.sent.push(sent),
                Err(error) => outcome.refused.push(RefusedDraft { draft_id: draft.id, code: error.code, message: error.message }),
            }
        }
        Ok(outcome)
    }
}
