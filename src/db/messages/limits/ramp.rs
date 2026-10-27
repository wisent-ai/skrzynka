//! A warm-up: a send limit that grows by a fixed count every UTC day from the
//! day it was set until it reaches a ceiling, so a new address is ramped up
//! without the operator raising the limit by hand each day.

use crate::error::AppError;
use chrono::NaiveDate;
use std::num::NonZeroU32;

/// How a stored limit grows: by `grow_by` a day from the UTC day `since`
/// (`YYYY-MM-DD`) until it reaches `up_to`.
#[derive(Debug, Clone)]
pub struct Ramp {
    pub grow_by: NonZeroU32,
    pub up_to: NonZeroU32,
    pub since: String,
}

/// A mailbox's limit row: the count it starts from and its warm-up, if any.
#[derive(Debug, Clone)]
pub struct LimitRow {
    pub per_day: NonZeroU32,
    pub ramp: Option<Ramp>,
}

impl LimitRow {
    /// The limit in force on the UTC day `day`: the starting count, grown by
    /// the warm-up for every whole day since it was set, never past its
    /// ceiling. A limit without a warm-up is its starting count every day,
    /// and so is a day not after the one the warm-up began on.
    pub fn in_force(&self, mailbox_id: &str, day: &str) -> Result<NonZeroU32, AppError> {
        let Some(ramp) = &self.ramp else {
            return Ok(self.per_day);
        };
        let read = |written: &str| {
            written.parse::<NaiveDate>().map_err(|error| {
                AppError::internal(format!(
                    "mailbox {mailbox_id}'s send limit warm-up names the day {written:?}, which is not a YYYY-MM-DD day: {error}"
                ))
            })
        };
        let (today, began) = (read(day)?, read(&ramp.since)?);
        if today <= began {
            return Ok(self.per_day);
        }
        let elapsed = (today - began).num_days();
        let days = u32::try_from(elapsed).map_err(|error| {
            AppError::internal(format!("{elapsed} days of warm-up is not a count of days: {error}"))
        })?;
        Ok(self
            .per_day
            .saturating_add(ramp.grow_by.get().saturating_mul(days))
            .min(ramp.up_to))
    }
}

/// A stored BIGINT read as a count of at least one message; anything else is
/// a row no command writes, named as such.
pub(super) fn stored_count(mailbox_id: &str, column: &str, stored: i64) -> Result<NonZeroU32, AppError> {
    let count = u32::try_from(stored).map_err(|error| {
        AppError::internal(format!(
            "mailbox {mailbox_id} stores {column} {stored}, which is not a count: {error}"
        ))
    })?;
    NonZeroU32::new(count).ok_or_else(|| {
        AppError::internal(format!(
            "mailbox {mailbox_id} stores {column} of no messages, which no command sets"
        ))
    })
}
