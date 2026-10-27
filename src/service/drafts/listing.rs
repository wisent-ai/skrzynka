//! Listing drafts: every draft, or those a filter keeps, most recently
//! changed first, the scheduled ones soonest first.

use super::{super::AppState, addressed};
use crate::{error::AppError, mail::Draft};
use chrono::{DateTime, FixedOffset};
use std::collections::BTreeSet;
use uuid::Uuid;

/// Which drafts a listing keeps, as `message draft list` takes them; the
/// default keeps every draft.
#[derive(Debug, Default, Clone)]
pub struct DraftFilter {
    /// Only this mailbox's drafts.
    pub mailbox_id: Option<Uuid>,
    /// Only those a running service will send, soonest first.
    pub scheduled: bool,
    /// Only those whose To, Cc, Bcc, subject or body contains this, case aside.
    pub contains: Option<String>,
    /// Only those addressed to an address `message bounced` lists, which a
    /// send refuses and a scheduled send never overrides.
    pub bounced: bool,
    /// Only those addressed to a recipient `message suppressed` covers, which
    /// a send and a schedule refuse with no override.
    pub suppressed: bool,
    /// Only those last changed at or after this RFC 3339 instant, `YYYY-MM-DD`
    /// day or span back such as `-7d`.
    pub since: Option<String>,
    /// Only those last changed before this: the drafts left untouched.
    pub until: Option<String>,
    /// Only those carrying at least one file, to find the drafts whose
    /// attachments make them heavy or need checking before they go.
    pub with_attachments: bool,
    /// Only the scheduled drafts a running service will send before this: a
    /// span from now such as `+1d`, or an RFC 3339 instant or `YYYY-MM-DD`
    /// day; what goes out soon, soonest first.
    pub send_by: Option<String>,
}

impl AppState {
    /// Every draft, or one mailbox's, most recently changed first, narrowed
    /// as `filter` says; scheduled ones soonest first. A blank `contains`, an
    /// unreadable instant or bound, or a window whose start is not before its
    /// end is `MESSAGE_FILTER_INVALID`.
    pub fn list_drafts(&self, organization_id: &str, filter: &DraftFilter) -> Result<Vec<Draft>, AppError> {
        let contains = super::super::messages::filter_value(filter.contains.as_deref(), "contains")?;
        let since = super::super::filters::instant(filter.since.as_deref(), "since")?;
        let until = super::super::filters::instant(filter.until.as_deref(), "until")?;
        let send_by = super::super::follow_ups::due_bound(filter.send_by.as_deref(), "send-by")?;
        if let (Some(from), Some(to)) = (since, until) {
            if from >= to {
                return Err(AppError::invalid(
                    "MESSAGE_FILTER_INVALID",
                    format!("since {from} is not before until {to}; no draft could have been changed between them"),
                ));
            }
        }
        let scheduled = filter.scheduled || send_by.is_some();
        let bounced: BTreeSet<String> = match filter.bounced {
            true => self.bounced_addresses(organization_id)?.into_iter().map(|entry| entry.address).collect(),
            false => BTreeSet::new(),
        };
        let suppressed = match filter.suppressed {
            true => self.suppressions(organization_id)?,
            false => Vec::new(),
        };
        let candidates = self
            .database
            .list_drafts(organization_id, filter.mailbox_id)?
            .into_iter()
            .filter(|draft| !scheduled || draft.send_at.is_some())
            .filter(|draft| !filter.with_attachments || !draft.attachments.is_empty())
            .filter(|draft| {
                contains.as_deref().is_none_or(|part| {
                    [&draft.recipients, &draft.cc, &draft.bcc, &draft.subject, &draft.body]
                        .iter()
                        .any(|field| field.to_lowercase().contains(part))
                })
            })
            .filter(|draft| !filter.bounced || addressed(draft).any(|address| bounced.contains(&address)))
            .filter(|draft| {
                !filter.suppressed || addressed(draft).any(|address| suppressed.iter().any(|kept| kept.covers(&address)))
            });
        let mut drafts = Vec::new();
        for draft in candidates {
            if since.is_some() || until.is_some() {
                let changed = stored_instant(&draft, &draft.updated_at, "change")?;
                if !(since.is_none_or(|from| changed >= from) && until.is_none_or(|to| changed < to)) {
                    continue;
                }
            }
            if let (Some(bound), Some(written)) = (send_by, draft.send_at.as_deref()) {
                if stored_instant(&draft, written, "send")? >= bound {
                    continue;
                }
            }
            drafts.push(draft);
        }
        if scheduled {
            drafts.sort_by(|first, second| first.send_at.cmp(&second.send_at));
        }
        Ok(drafts)
    }
}

/// An instant the draft stores, read back; one that is not RFC 3339 is an
/// internal error naming the draft and which instant it is.
fn stored_instant(draft: &Draft, written: &str, which: &str) -> Result<DateTime<FixedOffset>, AppError> {
    DateTime::parse_from_rfc3339(written).map_err(|error| {
        AppError::internal(format!(
            "draft {} stores the {which} instant {written:?}, which is not RFC 3339: {error}",
            draft.id
        ))
    })
}
