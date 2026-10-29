//! Sending what a narrowed listing keeps at once, each claimed by its own key
//! so the same request again sends nothing twice: here every originated
//! message that ended `failed`, such as the sends refused while a mailbox's
//! server was down, originated again exactly as written; `replies` does the
//! same for failed replies and `drafts` sends listed drafts now.

use super::{
    outbound::{checked_key, stored_addresses},
    AppState, OutboundFilters,
};
use crate::{
    error::AppError,
    models::{CreateOutboundRequest, DeliveryStatus, OutboundMessage},
};
use serde::Serialize;
use uuid::Uuid;

mod drafts;
mod replies;

/// What sending the listed failures again did.
#[derive(Debug, Serialize)]
pub struct SentAgain {
    /// The new outbound row of every message sent again, in the listing's order.
    pub sent: Vec<OutboundMessage>,
    /// Every failed message not sent again, and why.
    pub refused: Vec<RefusedResend>,
}

/// One failed message that was not sent again: its outbound ID and the
/// refusal.
#[derive(Debug, Serialize)]
pub struct RefusedResend {
    pub outbound_id: Uuid,
    pub code: &'static str,
    pub message: String,
}

impl AppState {
    /// Originate again, from the same mailbox, every message the outbound
    /// listing `filters` keeps (from `mailbox_id` when given) that ended
    /// `failed`, with its recipients, copies, subject and text, each claimed
    /// by `<idempotency_key>-<outbound id>`. A key that is blank or holds
    /// whitespace is refused before anything is read; a listing narrowed by
    /// no mailbox, addressee, text, file name or window is
    /// `OUTBOUND_RESEND_INVALID`, so nothing sends every failure ever again
    /// at once; a listing keeping no failed message is `NOT_FOUND`. A message
    /// that carried files is refused
    /// for itself (their bytes are not kept; `draft again` starts one to attach
    /// them), and so is one a send refuses now (a suppressed or bounced
    /// recipient, the send limit reached); the others are still sent.
    pub async fn resend_failed(
        &self,
        organization_id: &str,
        mailbox_id: Option<Uuid>,
        filters: &OutboundFilters,
        idempotency_key: &str,
    ) -> Result<SentAgain, AppError> {
        let key = checked_key(idempotency_key)?.to_string();
        let narrowed = [
            &filters.to,
            &filters.contains,
            &filters.attachment,
            &filters.since,
            &filters.until,
        ]
        .iter()
        .any(|filter| filter.is_some());
        if mailbox_id.is_none() && !narrowed {
            return Err(AppError::invalid(
                "OUTBOUND_RESEND_INVALID",
                "narrow the sent mail by mailbox, addressee, text, file name or window first; nothing sends every failed message again at once",
            ));
        }
        let failed: Vec<OutboundMessage> = self
            .search_outbound(organization_id, mailbox_id, filters, None, None)?
            .into_iter()
            .filter(|row| row.status == DeliveryStatus::Failed)
            .collect();
        if failed.is_empty() {
            return Err(AppError::not_found("failed outbound message"));
        }
        let mut outcome = SentAgain {
            sent: Vec::new(),
            refused: Vec::new(),
        };
        for row in failed {
            let id = row.id;
            match self.send_again(organization_id, &key, row).await {
                Ok(sent) => outcome.sent.push(sent),
                Err(error) => outcome.refused.push(RefusedResend {
                    outbound_id: id,
                    code: error.code,
                    message: error.message,
                }),
            }
        }
        Ok(outcome)
    }

    /// Originate one failed row again under `<key>-<its id>`.
    async fn send_again(
        &self,
        organization_id: &str,
        key: &str,
        row: OutboundMessage,
    ) -> Result<OutboundMessage, AppError> {
        if !row.attachments.is_empty() {
            return Err(AppError::invalid(
                "OUTBOUND_RESEND_INVALID",
                format!(
                    "outbound message {} carried files whose bytes Skrzynka does not keep; start a draft from it with draft again and attach them",
                    row.id
                ),
            ));
        }
        let request = CreateOutboundRequest {
            idempotency_key: format!("{key}-{}", row.id),
            to: stored_addresses(&row.recipients),
            cc: row
                .cc
                .as_deref()
                .map(stored_addresses)
                .into_iter()
                .flatten()
                .collect(),
            bcc: row
                .bcc
                .as_deref()
                .map(stored_addresses)
                .into_iter()
                .flatten()
                .collect(),
            subject: row.subject,
            body: row.body,
            attachments: Vec::new(),
            allow_bounced: false,
            follow_up: None,
        };
        self.send_outbound(organization_id, row.mailbox_id, request)
            .await
    }
}
