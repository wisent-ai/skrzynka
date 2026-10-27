//! Answering again where a reply failed: every reply a narrowed listing keeps
//! that ended `failed`, such as those refused while a mailbox's server was
//! down, sent again to the same message as written, each claimed by its own
//! key so the same request again sends nothing twice.

use super::super::{outbound::{checked_key, stored_addresses}, AppState, ReplyFilter};
use crate::{
    error::AppError,
    models::{CreateReplyRequest, DeliveryStatus, ReplyAttempt},
};
use serde::Serialize;
use uuid::Uuid;

/// What answering the listed failures again did.
#[derive(Debug, Serialize)]
pub struct RepliedAgain {
    /// The new attempt of every reply sent again, in the listing's order.
    pub sent: Vec<ReplyAttempt>,
    /// Every failed reply not sent again, and why.
    pub refused: Vec<RefusedReply>,
}

/// One failed reply that was not sent again: its ID and the refusal.
#[derive(Debug, Serialize)]
pub struct RefusedReply {
    pub reply_id: Uuid,
    pub code: &'static str,
    pub message: String,
}

impl AppState {
    /// Send again every reply `filter` keeps that ended `failed`, to the
    /// message it answered, with its text as sent (a quote included), its
    /// reply-to-all choice and its copies, each claimed by
    /// `<idempotency_key>-<reply id>`. A key that is blank or holds
    /// whitespace is refused before anything is read; a listing narrowed by
    /// no mailbox, text, file name or window is `OUTBOUND_RESEND_INVALID`, so
    /// nothing answers every failure ever again at once; one keeping no failed
    /// reply is `NOT_FOUND`. A reply that carried files is refused for itself
    /// (their bytes are not kept), so is one whose message has been answered
    /// since by a reply that ended `sent`, and so is one a send refuses now;
    /// the others are still sent.
    pub async fn resend_failed_replies(
        &self,
        organization_id: &str,
        filter: &ReplyFilter,
        idempotency_key: &str,
    ) -> Result<RepliedAgain, AppError> {
        let key = checked_key(idempotency_key)?.to_string();
        let narrowed = [&filter.since, &filter.until, &filter.contains, &filter.attachment].iter().any(|bound| bound.is_some());
        if filter.mailbox_id.is_none() && !narrowed {
            return Err(AppError::invalid(
                "OUTBOUND_RESEND_INVALID",
                "narrow the replies by mailbox, text, file name or window first; nothing answers every failed reply again at once",
            ));
        }
        let failed: Vec<ReplyAttempt> = self
            .all_replies(organization_id, filter)?
            .into_iter()
            .filter(|attempt| attempt.status == DeliveryStatus::Failed)
            .collect();
        if failed.is_empty() {
            return Err(AppError::not_found("failed reply"));
        }
        let mut outcome = RepliedAgain { sent: Vec::new(), refused: Vec::new() };
        for attempt in failed {
            let id = attempt.id;
            match self.reply_again(organization_id, &key, attempt).await {
                Ok(sent) => outcome.sent.push(sent),
                Err(error) => outcome.refused.push(RefusedReply { reply_id: id, code: error.code, message: error.message }),
            }
        }
        Ok(outcome)
    }

    /// Send one failed reply again under `<key>-<its id>`, unless it carried
    /// files or its message was answered since.
    async fn reply_again(&self, organization_id: &str, key: &str, attempt: ReplyAttempt) -> Result<ReplyAttempt, AppError> {
        if !attempt.attachments.is_empty() {
            return Err(AppError::invalid(
                "OUTBOUND_RESEND_INVALID",
                format!("reply {} carried files whose bytes Skrzynka does not keep; answer the message again and attach them", attempt.id),
            ));
        }
        let answered = self
            .list_replies(organization_id, attempt.message_id)?
            .iter()
            .any(|other| other.status == DeliveryStatus::Sent);
        if answered {
            return Err(AppError::invalid(
                "OUTBOUND_RESEND_INVALID",
                format!("message {} has been answered since by a reply that was sent; reply {} is not sent again", attempt.message_id, attempt.id),
            ));
        }
        let request = CreateReplyRequest {
            idempotency_key: format!("{key}-{}", attempt.id),
            body: attempt.body,
            attachments: Vec::new(),
            reply_all: attempt.reply_all,
            quote: false,
            cc: attempt.cc.as_deref().map(stored_addresses).into_iter().flatten().collect(),
            bcc: attempt.bcc.as_deref().map(stored_addresses).into_iter().flatten().collect(),
            automatic: false,
        };
        self.reply(organization_id, attempt.message_id, request).await
    }
}
