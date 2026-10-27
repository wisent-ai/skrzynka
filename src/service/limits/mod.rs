//! A mailbox's send limit as the operator set it, its warm-up, and what today
//! used of it.

use super::AppState;
use crate::error::AppError;
use serde::Serialize;
use std::num::NonZeroU32;
use uuid::Uuid;

/// What `mailbox limit` answers. `per_day` is the limit in force today,
/// absent for a mailbox without a limit; `warm_up` says how it grows when it
/// does; `sent_today` counts every originated message, reply and unsubscribe
/// mail claimed on `day` (UTC) and not recorded `failed`, whether a limit is
/// set or not; `remaining` is what the limit still admits today.
#[derive(Debug, Serialize)]
pub struct SendLimit {
    pub mailbox_id: Uuid,
    pub per_day: Option<NonZeroU32>,
    pub warm_up: Option<WarmUp>,
    pub day: String,
    pub sent_today: u64,
    pub remaining: Option<u64>,
}

/// A limit that grows: from `from` on the UTC day `since`, by `grow_by`
/// every day after it, until it reaches `up_to` on the UTC day `reaches_up_to_on`.
#[derive(Debug, Serialize)]
pub struct WarmUp {
    pub from: NonZeroU32,
    pub grow_by: NonZeroU32,
    pub up_to: NonZeroU32,
    pub since: String,
    pub reaches_up_to_on: String,
}

/// How a limit should grow, as `mailbox limit --grow-by --up-to` asks it.
#[derive(Debug, Clone, Copy)]
pub struct WarmUpRequest {
    pub grow_by: NonZeroU32,
    pub up_to: NonZeroU32,
}

impl AppState {
    pub fn mailbox_send_limit(
        &self,
        organization_id: &str,
        mailbox_id: Uuid,
    ) -> Result<SendLimit, AppError> {
        let stored = self.database.send_limit(organization_id, mailbox_id)?;
        let warm_up = match stored.limit {
            Some(limit) => match limit.ramp {
                Some(ramp) => Some(WarmUp {
                    reaches_up_to_on: ceiling_day(&ramp.since, limit.per_day, ramp.grow_by, ramp.up_to)?,
                    from: limit.per_day,
                    grow_by: ramp.grow_by,
                    up_to: ramp.up_to,
                    since: ramp.since,
                }),
                None => None,
            },
            None => None,
        };
        Ok(SendLimit {
            mailbox_id,
            per_day: stored.in_force,
            warm_up,
            day: stored.day,
            sent_today: stored.sent_today,
            remaining: stored
                .in_force
                .map(|per_day| u64::from(per_day.get()).saturating_sub(stored.sent_today)),
        })
    }

    /// Set the limit, growing as `warm_up` asks from today when given, or
    /// remove it with `None`, and answer the stored state. A warm-up without
    /// a limit to start from, or whose ceiling is not above it, is
    /// `MAILBOX_LIMIT_INVALID` and changes nothing.
    pub fn set_mailbox_send_limit(
        &self,
        organization_id: &str,
        mailbox_id: Uuid,
        per_day: Option<NonZeroU32>,
        warm_up: Option<WarmUpRequest>,
    ) -> Result<SendLimit, AppError> {
        if let Some(warm_up) = warm_up {
            let Some(per_day) = per_day else {
                return Err(AppError::invalid(
                    "MAILBOX_LIMIT_INVALID",
                    "a warm-up grows a limit; name the count it starts from with per_day (--per-day)",
                ));
            };
            if warm_up.up_to <= per_day {
                return Err(AppError::invalid(
                    "MAILBOX_LIMIT_INVALID",
                    format!(
                        "the warm-up ceiling {} is not above the starting limit {per_day}; name a higher up_to (--up-to), or set the limit alone",
                        warm_up.up_to
                    ),
                ));
            }
        }
        self.database.set_send_limit(
            organization_id,
            mailbox_id,
            per_day,
            warm_up.map(|warm_up| (warm_up.grow_by, warm_up.up_to)),
        )?;
        self.mailbox_send_limit(organization_id, mailbox_id)
    }
}

/// The UTC day a warm-up from `from` on `since`, growing by `grow_by` a
/// day, first allows `up_to`: as many whole days after `since` as the gap
/// takes, the last step rounded up.
fn ceiling_day(since: &str, from: NonZeroU32, grow_by: NonZeroU32, up_to: NonZeroU32) -> Result<String, AppError> {
    let began = since.parse::<chrono::NaiveDate>().map_err(|error| {
        AppError::internal(format!("a send limit warm-up names the day {since:?}, which is not a YYYY-MM-DD day: {error}"))
    })?;
    let days = up_to.get().saturating_sub(from.get()).div_ceil(grow_by.get());
    began
        .checked_add_days(chrono::Days::new(u64::from(days)))
        .map(|day| day.to_string())
        .ok_or_else(|| AppError::internal(format!("a warm-up from {began} reaches past the last day Skrzynka can name")))
}
