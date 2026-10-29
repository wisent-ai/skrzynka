//! Scheduling many drafts at once, such as the drafts of a mail merge: each
//! listed draft not scheduled yet goes out at the first instant, or spread
//! out by a gap after one another, so a mailbox sends at a steady pace a
//! receiving provider trusts. Each is scheduled as `draft schedule` would,
//! claimed by its own key; one refused leaves none scheduled.

use super::{
    super::{snoozes::span_from_now, AppState},
    schedule::instant,
    DraftFilter,
};
use crate::{error::AppError, mail::Draft};
use chrono::TimeDelta;

impl AppState {
    /// Schedule every draft `filter` lists that is not scheduled yet, the
    /// earliest written first: the first at `send_at`, each next `every`
    /// later (a span such as `5m`, `1h` or `1d`), each claimed by
    /// `<idempotency_key>-<draft id>`, so the same command again claims the
    /// same sends. None listed, an unreadable instant or span, or a key that
    /// is blank or holds whitespace is refused before anything is scheduled;
    /// a draft `draft schedule` refuses (a suppressed or bounced recipient,
    /// an instant passed) unschedules the ones scheduled before it and ends
    /// with that refusal.
    pub fn schedule_drafts(
        &self,
        organization_id: &str,
        filter: &DraftFilter,
        send_at: &str,
        every: Option<&str>,
        idempotency_key: &str,
    ) -> Result<Vec<Draft>, AppError> {
        let refused = |message: String| {
            AppError::invalid(
                "DRAFT_SCHEDULE_INVALID",
                format!("{message}; nothing was scheduled"),
            )
        };
        let key = super::super::outbound::checked_key(idempotency_key)?;
        let mut at = instant(send_at)
            .map_err(|error| refused(format!("{send_at:?} is not an RFC 3339 instant: {error}")))?;
        let gap = match every {
            Some(span) => span_from_now(span.trim(), '+', "DRAFT_SCHEDULE_INVALID")?,
            None => TimeDelta::zero(),
        };
        let mut drafts: Vec<Draft> = self
            .list_drafts(organization_id, filter)?
            .into_iter()
            .filter(|draft| draft.send_at.is_none())
            .collect();
        if drafts.is_empty() {
            return Err(refused(
                "no draft these filters list is waiting to be scheduled".to_string(),
            ));
        }
        drafts.reverse();
        let mut scheduled = Vec::new();
        for draft in drafts {
            match self.schedule_draft(
                organization_id,
                draft.id,
                &at.to_rfc3339(),
                &format!("{key}-{}", draft.id),
            ) {
                Ok(done) => scheduled.push(done),
                Err(error) => {
                    for done in &scheduled {
                        self.unschedule_draft(organization_id, done.id)?;
                    }
                    let message = format!(
                        "{}; the drafts scheduled before it were unscheduled",
                        error.message
                    );
                    return Err(AppError { message, ..error });
                }
            }
            at += gap;
        }
        Ok(scheduled)
    }
}
