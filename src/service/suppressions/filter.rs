//! Which suppressed entries `message suppressed` lists: those holding a
//! text, or those added within a window, checked before the list is read.

use crate::{error::AppError, mail::Suppression};
use chrono::{DateTime, Utc};
use serde::Deserialize;

/// Which suppressed entries are listed; the default lists every one.
#[derive(Debug, Default, Clone, Deserialize)]
pub struct SuppressionFilter {
    /// Only the entries whose entry or reason contains this, case aside.
    pub contains: Option<String>,
    /// Only the entries added at or after this RFC 3339 instant, `YYYY-MM-DD`
    /// day or span back such as `-7d`: what was suppressed lately.
    pub since: Option<String>,
    /// Only the entries added before this.
    pub until: Option<String>,
}

impl SuppressionFilter {
    /// Keep the entries this filter admits, in their order. A blank text, an
    /// unreadable instant or a window whose start is not before its end is
    /// `MESSAGE_FILTER_INVALID`; an entry whose stored `added_at` is not
    /// RFC 3339 is an internal error naming it.
    pub(super) fn narrow(&self, entries: Vec<Suppression>) -> Result<Vec<Suppression>, AppError> {
        let contains = super::super::messages::filter_value(self.contains.as_deref(), "contains")?;
        let since = super::super::filters::instant(self.since.as_deref(), "since")?;
        let until = super::super::filters::instant(self.until.as_deref(), "until")?;
        if let (Some(from), Some(to)) = (since, until) {
            if from >= to {
                return Err(AppError::invalid(
                    "MESSAGE_FILTER_INVALID",
                    format!("since {from} is not before until {to}; no entry could have been added between them"),
                ));
            }
        }
        let mut kept = Vec::new();
        for entry in entries {
            if let Some(part) = contains.as_deref() {
                let holds = entry.entry.contains(part)
                    || entry
                        .reason
                        .as_deref()
                        .is_some_and(|reason| reason.to_lowercase().contains(part));
                if !holds {
                    continue;
                }
            }
            if since.is_some() || until.is_some() {
                let added: DateTime<Utc> = DateTime::parse_from_rfc3339(&entry.added_at)
                    .map_err(|error| {
                        AppError::internal(format!(
                            "suppressed entry {} stores the instant {:?}, which is not RFC 3339: {error}",
                            entry.entry, entry.added_at
                        ))
                    })?
                    .with_timezone(&Utc);
                if since.is_some_and(|from| added < from) || until.is_some_and(|to| added >= to) {
                    continue;
                }
            }
            kept.push(entry);
        }
        Ok(kept)
    }
}
