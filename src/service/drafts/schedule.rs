//! Scheduled drafts: a draft given a time and a key is sent then by every
//! running `skrzynka serve`. The send is the same `draft send`, so the key
//! makes two services, or a service started again mid-send, one send.

use super::{super::AppState, DraftFilter, DraftSendRequest};
use crate::{
    error::AppError,
    mail::{Draft, DraftChange},
    models::{DeliveryStatus, OutboundMessage},
};
use chrono::{DateTime, Utc};
use uuid::Uuid;

impl AppState {
    /// Schedule the draft for `send_at` (RFC 3339), which must lie ahead,
    /// under `idempotency_key`. A draft addressed to a suppressed recipient
    /// is refused with `OUTBOUND_RECIPIENT_SUPPRESSED`, and one addressed to
    /// an address `message bounced` lists with `OUTBOUND_RECIPIENT_BOUNCED`,
    /// now, since its scheduled send would be refused with nobody there.
    pub fn schedule_draft(
        &self,
        organization_id: &str,
        id: Uuid,
        send_at: &str,
        idempotency_key: &str,
    ) -> Result<Draft, AppError> {
        let at = instant(send_at).map_err(|error| {
            AppError::invalid(
                "DRAFT_SCHEDULE_INVALID",
                format!("{send_at:?} is not an RFC 3339 instant: {error}"),
            )
        })?;
        if at <= Utc::now() {
            return Err(AppError::invalid(
                "DRAFT_SCHEDULE_INVALID",
                format!("{at} has already passed; schedule a later instant or send the draft now"),
            ));
        }
        let key = super::super::outbound::checked_key(idempotency_key)?;
        let draft = self.database.get_draft(organization_id, id)?;
        let named: Vec<String> = super::addressed(&draft).collect();
        self.refuse_suppressed(organization_id, &named, &format!("draft {id}"))?;
        let listed = self.bounced_addresses(organization_id)?;
        let bounced: Vec<String> = super::addressed(&draft)
            .filter(|address| listed.iter().any(|entry| entry.address == *address))
            .collect();
        if !bounced.is_empty() {
            return Err(AppError::conflict(
                "OUTBOUND_RECIPIENT_BOUNCED",
                format!(
                    "draft {id} is addressed to {}, which a stored bounce report says failed for good, and a scheduled send is always held to the reports; correct the address, clear it with `skrzynka message bounced --clear <ADDRESS>`, or send the draft now with --allow-bounced",
                    bounced.join(", ")
                ),
            ));
        }
        self.database
            .schedule_draft(organization_id, id, Some(&at.to_rfc3339()), Some(key))
    }

    /// Send a new message later: keep `change` as a draft of the mailbox
    /// `selector` names and schedule it for `send_at` under
    /// `idempotency_key`, as `message send --at` asks. A schedule that is
    /// refused removes the draft again, so a refused send leaves nothing.
    pub fn schedule_new_draft(
        &self,
        organization_id: &str,
        selector: &str,
        change: DraftChange,
        send_at: &str,
        idempotency_key: &str,
    ) -> Result<Draft, AppError> {
        let draft = self.create_draft(organization_id, selector, change)?;
        match self.schedule_draft(organization_id, draft.id, send_at, idempotency_key) {
            Ok(scheduled) => Ok(scheduled),
            Err(refusal) => match self.delete_draft(organization_id, draft.id) {
                Ok(()) => Err(refusal),
                Err(kept) => Err(AppError::new(
                    refusal.status,
                    refusal.code,
                    format!(
                        "{}; the draft {} written for it could not be removed ({}: {}), so remove it with `skrzynka message draft delete {}`",
                        refusal.message, draft.id, kept.code, kept.message, draft.id
                    ),
                    refusal.retryable,
                )),
            },
        }
    }

    /// Take the draft off its schedule; it stays a draft.
    pub fn unschedule_draft(&self, organization_id: &str, id: Uuid) -> Result<Draft, AppError> {
        self.database.schedule_draft(organization_id, id, None, None)
    }

    /// Take every scheduled draft mentioning `filter.contains` (of its
    /// mailbox when it names one) off its schedule, such as a staggered
    /// merge to hold back, and answer them unscheduled; they stay drafts.
    /// A filter naming no text is `MESSAGE_FILTER_INVALID`, so nothing
    /// unschedules every draft at once; none scheduled is `NOT_FOUND`.
    pub fn unschedule_drafts(&self, organization_id: &str, filter: &DraftFilter) -> Result<Vec<Draft>, AppError> {
        if filter.contains.is_none() {
            return Err(AppError::invalid(
                "MESSAGE_FILTER_INVALID",
                "name the text the scheduled drafts contain, or unschedule one draft by its ID",
            ));
        }
        let scheduled = DraftFilter { scheduled: true, ..filter.clone() };
        let listed = self.list_drafts(organization_id, &scheduled)?;
        if listed.is_empty() {
            return Err(AppError::not_found("scheduled draft"));
        }
        listed.iter().map(|draft| self.unschedule_draft(organization_id, draft.id)).collect()
    }

    /// Send every draft whose time has come. One that does not end `sent`
    /// is taken off its schedule with why; nothing is tried again unasked.
    /// A scheduled send is held to the bounce reports: no operator is there
    /// to say an address that failed for good should be tried again.
    async fn send_due_drafts(&self) -> Result<(), AppError> {
        for due in self.database.due_drafts(&Utc::now().to_rfc3339())? {
            let request = DraftSendRequest {
                idempotency_key: due.send_key,
                allow_bounced: false,
                follow_up: None,
            };
            let failure = match self.send_draft(&due.organization_id, due.id, request).await
            {
                Ok(sent) if sent.status == DeliveryStatus::Sent => continue,
                Ok(sent) => not_sent(&sent),
                Err(error) => format!("{}: {}", error.code, error.message),
            };
            tracing::warn!(draft = %due.id, cause = %failure, "a scheduled draft was not sent");
            self.database.record_schedule_failure(due.id, &failure)?;
        }
        Ok(())
    }

    /// The earliest scheduled time, read as an instant.
    fn next_due(&self) -> Result<Option<DateTime<Utc>>, AppError> {
        match self.database.next_scheduled_draft()? {
            None => Ok(None),
            Some(stored) => instant(&stored).map(Some).map_err(|error| {
                AppError::internal(format!(
                    "a draft is scheduled for {stored:?}, which is not an RFC 3339 instant: {error}"
                ))
            }),
        }
    }

    /// Send scheduled drafts for as long as the service runs: at each
    /// draft's time, and again whenever any process changes a schedule.
    pub(in crate::service) fn start_scheduling(&self) -> Result<(), AppError> {
        let subscription = self.database.subscribe_drafts()?;
        let state = self.clone();
        tokio::spawn(async move {
            let mut subscription = subscription;
            loop {
                if let Err(error) = state.send_due_drafts().await {
                    tracing::error!(code = error.code, message = %error.message, "scheduled drafts could not be sent");
                }
                let due = match state.next_due() {
                    Ok(due) => due,
                    Err(error) => {
                        tracing::error!(code = error.code, message = %error.message, "the next scheduled draft could not be read; waiting for the next schedule change");
                        None
                    }
                };
                let database = state.database.clone();
                let waited = tokio::task::spawn_blocking(move || {
                    database.next_draft_announcement(subscription, due)
                })
                .await;
                subscription = match waited {
                    Ok(Ok(subscription)) => subscription,
                    Ok(Err(ended)) => match state.database.subscribe_drafts() {
                        Ok(subscription) => {
                            tracing::warn!(code = ended.code, message = %ended.message, "draft announcements stopped; subscribed again");
                            subscription
                        }
                        Err(error) => {
                            tracing::error!(code = error.code, message = %error.message, "draft announcements cannot be received; scheduled drafts are not sent until this service starts again");
                            return;
                        }
                    },
                    Err(stopped) => {
                        tracing::error!(error = %stopped, "the draft scheduler stopped; scheduled drafts are not sent until this service starts again");
                        return;
                    }
                };
            }
        });
        Ok(())
    }
}

/// An RFC 3339 instant as written, in UTC.
pub(super) fn instant(written: &str) -> Result<DateTime<Utc>, chrono::ParseError> {
    DateTime::parse_from_rfc3339(written.trim()).map(|at| at.with_timezone(&Utc))
}

/// Why a scheduled send that ended in a row was not sent, from the row.
fn not_sent(sent: &OutboundMessage) -> String {
    let status = sent.status.as_str();
    match (&sent.error_code, &sent.error_message) {
        (Some(code), Some(message)) => {
            format!("{code}: outbound message {} ended {status}: {message}", sent.id)
        }
        (Some(code), None) => format!("{code}: outbound message {} ended {status}", sent.id),
        (None, Some(message)) => format!("outbound message {} ended {status}: {message}", sent.id),
        (None, None) => format!(
            "outbound message {} ended {status} and records no cause; read it with `skrzynka message outbound --id {}`",
            sent.id, sent.id
        ),
    }
}
