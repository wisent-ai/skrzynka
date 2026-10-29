//! Every reply sent from the organization's mailboxes, or one mailbox's, in
//! one listing: the replies `message replies <ID>` shows per message, found
//! without knowing which message each answered.

use super::super::{filters::instant, messages::filter_value, AppState};
use crate::{
    error::AppError,
    models::{DeliveryStatus, ReplyAttempt},
};
use std::str::FromStr;
use uuid::Uuid;

/// Which replies `message replies` without an ID keeps, as written; an
/// absent one is no filter.
#[derive(Debug, Default, Clone)]
pub struct ReplyFilter {
    /// Only the replies to this mailbox's messages.
    pub mailbox_id: Option<Uuid>,
    /// Only those in this delivery status.
    pub status: Option<String>,
    /// Only those claimed at or after this RFC 3339 instant or UTC day.
    pub since: Option<String>,
    /// Only those claimed before this RFC 3339 instant or UTC day.
    pub until: Option<String>,
    /// Only those whose text contains this, case aside.
    pub contains: Option<String>,
    /// Only those that carried a file whose name contains this, case aside.
    pub attachment: Option<String>,
    /// List the oldest claimed first.
    pub oldest_first: bool,
}

impl AppState {
    /// Every reply attempt, newest first or with `oldest_first` oldest first,
    /// narrowed as `filter` says. A blank
    /// or unknown status, a blank text or file name, an unreadable instant,
    /// or `since` not before `until` is `MESSAGE_FILTER_INVALID`.
    pub fn all_replies(
        &self,
        organization_id: &str,
        filter: &ReplyFilter,
    ) -> Result<Vec<ReplyAttempt>, AppError> {
        let status = filter_value(filter.status.as_deref(), "status")?
            .map(|written| {
                DeliveryStatus::from_str(&written).map_err(|error| {
                    AppError::invalid(
                        "MESSAGE_FILTER_INVALID",
                        format!(
                            "the status filter is not a delivery status ({error}); name pending, sending, sent, failed or uncertain"
                        ),
                    )
                })
            })
            .transpose()?;
        let contains = filter_value(filter.contains.as_deref(), "contains")?;
        let attachment = filter_value(filter.attachment.as_deref(), "attachment")?;
        let since = instant(filter.since.as_deref(), "since")?;
        let until = instant(filter.until.as_deref(), "until")?;
        if let (Some(from), Some(to)) = (since, until) {
            if from >= to {
                return Err(AppError::invalid(
                    "MESSAGE_FILTER_INVALID",
                    format!("since {from} is not before until {to}; nothing could be claimed between them"),
                ));
            }
        }
        let mut kept = Vec::new();
        for reply in self
            .database
            .all_replies(organization_id, filter.mailbox_id)?
        {
            if status.is_some_and(|status| reply.status != status) {
                continue;
            }
            if contains
                .as_deref()
                .is_some_and(|part| !reply.body.to_lowercase().contains(part))
            {
                continue;
            }
            let carried = attachment.as_deref().is_none_or(|part| {
                reply
                    .attachments
                    .iter()
                    .any(|file| file.filename.to_lowercase().contains(part))
            });
            if !carried {
                continue;
            }
            if since.is_some() || until.is_some() {
                let claimed = chrono::DateTime::parse_from_rfc3339(&reply.created_at)
                    .map_err(|error| {
                        AppError::internal(format!(
                            "reply attempt {} stores the claim instant {:?}, which is not RFC 3339: {error}",
                            reply.id, reply.created_at
                        ))
                    })?
                    .with_timezone(&chrono::Utc);
                if since.is_some_and(|from| claimed < from) || until.is_some_and(|to| claimed >= to)
                {
                    continue;
                }
            }
            kept.push(reply);
        }
        if filter.oldest_first {
            kept.reverse();
        }
        Ok(kept)
    }
}
