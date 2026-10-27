//! A mailbox's send limit: how many messages the operator lets it send in one
//! UTC day. Every claim of a send (an originated message, a reply, an
//! unsubscribe mail) is admitted under the limit row's lock, so two claims at
//! once cannot both take the last place.

use super::super::sql::{self, params, OptionalExtension};
use super::super::Database;
use crate::error::AppError;
use axum::http::StatusCode;
use chrono::Utc;
use stado_database::sync::Tx;
use std::num::NonZeroU32;
use uuid::Uuid;

mod ramp;
pub use ramp::{LimitRow, Ramp};
use ramp::stored_count;

/// The sends of one mailbox claimed since `$2` that count against its limit:
/// every one not recorded `failed`. An unsubscribe counts only when it sends
/// mail; a one-click request sends none.
const SENDS_SINCE: &str = "SELECT
        (SELECT COUNT(*) FROM outbound_messages
          WHERE mailbox_id=$1 AND created_at >= $2 AND status <> 'failed')
      + (SELECT COUNT(*) FROM reply_attempts
          JOIN messages ON messages.id=reply_attempts.message_id
          WHERE messages.mailbox_id=$1 AND reply_attempts.created_at >= $2
            AND reply_attempts.status <> 'failed')
      + (SELECT COUNT(*) FROM unsubscribe_attempts
          JOIN messages ON messages.id=unsubscribe_attempts.message_id
          WHERE messages.mailbox_id=$1 AND unsubscribe_attempts.created_at >= $2
            AND unsubscribe_attempts.method='mailto'
            AND unsubscribe_attempts.status <> 'failed') AS sends";

/// A mailbox's limit as stored, the limit in force today, and what today has
/// used of it.
pub struct StoredSendLimit {
    pub limit: Option<LimitRow>,
    pub in_force: Option<NonZeroU32>,
    pub sent_today: u64,
    pub day: String,
}

impl Database {
    /// The mailbox's send limit and the sends counted against it today.
    pub fn send_limit(
        &self,
        organization_id: &str,
        mailbox_id: Uuid,
    ) -> Result<StoredSendLimit, AppError> {
        self.get_mailbox(organization_id, mailbox_id)?;
        let connection = self.lock()?;
        let transaction = connection.transaction()?;
        let id = mailbox_id.to_string();
        let limit = limit_of(&transaction, &id, false)?;
        let day = today();
        let in_force = limit.as_ref().map(|row| row.in_force(&id, &day)).transpose()?;
        let sent_today = sends_since(&transaction, &id, &day)?;
        transaction.commit()?;
        Ok(StoredSendLimit {
            limit,
            in_force,
            sent_today,
            day,
        })
    }

    /// Set the mailbox's limit to `per_day` sends a UTC day, growing as
    /// `ramp` says (`grow_by` a day up to `up_to`, from today) when given, or
    /// remove it when `per_day` is absent, and answer the stored state.
    /// Setting a warm-up again starts it over from today.
    pub fn set_send_limit(
        &self,
        organization_id: &str,
        mailbox_id: Uuid,
        per_day: Option<NonZeroU32>,
        ramp: Option<(NonZeroU32, NonZeroU32)>,
    ) -> Result<StoredSendLimit, AppError> {
        self.get_mailbox(organization_id, mailbox_id)?;
        let id = mailbox_id.to_string();
        match per_day {
            Some(per_day) => {
                let grow_by = ramp.map(|(grow_by, _)| i64::from(grow_by.get()));
                let up_to = ramp.map(|(_, up_to)| i64::from(up_to.get()));
                let since = ramp.map(|_| today());
                self.lock()?.execute(
                    "INSERT INTO mailbox_send_limits (mailbox_id, per_day, updated_at, ramp_by, ramp_to, ramp_since)
                     VALUES ($1, $2, $3, $4, $5, $6)
                     ON CONFLICT (mailbox_id) DO UPDATE
                     SET per_day=EXCLUDED.per_day, updated_at=EXCLUDED.updated_at,
                         ramp_by=EXCLUDED.ramp_by, ramp_to=EXCLUDED.ramp_to, ramp_since=EXCLUDED.ramp_since",
                    params![id, i64::from(per_day.get()), Utc::now().to_rfc3339(), grow_by, up_to, since],
                )?;
            }
            None => {
                self.lock()?.execute(
                    "DELETE FROM mailbox_send_limits WHERE mailbox_id=$1",
                    params![id],
                )?;
            }
        }
        self.send_limit(organization_id, mailbox_id)
    }

    /// Run `claim`, the insert of one new send from `mailbox_id`, inside a
    /// transaction that first admits it under the mailbox's send limit. A
    /// limit already used up today refuses with `MAILBOX_SEND_LIMIT_REACHED`
    /// and claims nothing. The claim's own database error is answered as it
    /// came, so the caller can tell a repeated idempotency key from a failure.
    pub(in crate::db) fn claim_send<T>(
        &self,
        mailbox_id: &str,
        claim: impl FnOnce(&Tx<'_>) -> sql::Result<T>,
    ) -> Result<sql::Result<T>, AppError> {
        let connection = self.lock()?;
        let transaction = connection.transaction()?;
        if let Some(limit) = limit_of(&transaction, mailbox_id, true)? {
            let day = today();
            let per_day = limit.in_force(mailbox_id, &day)?;
            let sent = sends_since(&transaction, mailbox_id, &day)?;
            if sent >= u64::from(per_day.get()) {
                return Err(AppError::new(
                    StatusCode::TOO_MANY_REQUESTS,
                    "MAILBOX_SEND_LIMIT_REACHED",
                    format!(
                        "mailbox {mailbox_id} has sent {sent} of its {per_day} messages for {day} (UTC); nothing was sent. Send again after the UTC day ends, or raise the limit with `skrzynka mailbox limit {mailbox_id} --per-day <COUNT>`"
                    ),
                    false,
                ));
            }
        }
        match claim(&transaction) {
            Ok(value) => {
                transaction.commit()?;
                Ok(Ok(value))
            }
            Err(error) => Ok(Err(error)),
        }
    }
}

/// The current UTC day as `YYYY-MM-DD`, which every stored RFC 3339 instant
/// of that day sorts at or after.
fn today() -> String {
    Utc::now().date_naive().to_string()
}

/// The mailbox's limit row, locked for the rest of the transaction when `lock`.
fn limit_of(
    transaction: &Tx<'_>,
    mailbox_id: &str,
    lock: bool,
) -> Result<Option<LimitRow>, AppError> {
    let statement = if lock {
        "SELECT per_day, ramp_by, ramp_to, ramp_since FROM mailbox_send_limits WHERE mailbox_id=$1 FOR UPDATE"
    } else {
        "SELECT per_day, ramp_by, ramp_to, ramp_since FROM mailbox_send_limits WHERE mailbox_id=$1"
    };
    let stored = transaction
        .query_row(statement, params![mailbox_id], |row| {
            Ok((
                row.get::<_, i64>("per_day")?,
                row.get::<_, Option<i64>>("ramp_by")?,
                row.get::<_, Option<i64>>("ramp_to")?,
                row.get::<_, Option<String>>("ramp_since")?,
            ))
        })
        .optional()?;
    let Some((per_day, grow_by, up_to, since)) = stored else {
        return Ok(None);
    };
    let ramp = match (grow_by, up_to, since) {
        (Some(grow_by), Some(up_to), Some(since)) => Some(Ramp {
            grow_by: stored_count(mailbox_id, "send limit warm-up step", grow_by)?,
            up_to: stored_count(mailbox_id, "send limit warm-up ceiling", up_to)?,
            since,
        }),
        (None, None, None) => None,
        _ => {
            return Err(AppError::internal(format!(
                "mailbox {mailbox_id} stores part of a send limit warm-up, which no command writes; set the limit again with `skrzynka mailbox limit {mailbox_id}`"
            )))
        }
    };
    Ok(Some(LimitRow {
        per_day: stored_count(mailbox_id, "send limit", per_day)?,
        ramp,
    }))
}

fn sends_since(transaction: &Tx<'_>, mailbox_id: &str, day: &str) -> Result<u64, AppError> {
    let sends = transaction.query_row(SENDS_SINCE, params![mailbox_id, day], |row| {
        row.get::<_, i64>("sends")
    })?;
    u64::try_from(sends).map_err(|error| {
        AppError::internal(format!(
            "mailbox {mailbox_id} counts {sends} sends today, which is not a count: {error}"
        ))
    })
}
