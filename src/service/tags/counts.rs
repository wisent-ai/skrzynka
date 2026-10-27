//! Counting the tags in use: of every stored message, of one mailbox's, of
//! those holding a text, or only on the mail received within a window.

use super::{super::MessageFilters, AppState, TagCount};
use crate::error::AppError;
use std::collections::BTreeMap;
use uuid::Uuid;

/// Which tags `message tags` lists and which messages it counts; the
/// default counts every tag on every stored message.
#[derive(Debug, Default, Clone)]
pub struct TagFilter {
    /// Only this mailbox's messages are counted.
    pub mailbox_id: Option<Uuid>,
    /// Only the tags holding this text, case aside.
    pub contains: Option<String>,
    /// Only messages received at or after this RFC 3339 instant,
    /// `YYYY-MM-DD` day or span back such as `-30d` are counted: the tags
    /// used lately.
    pub since: Option<String>,
    /// Only messages received before this are counted.
    pub until: Option<String>,
}

impl AppState {
    /// Every tag the counted messages carry, most used first; with
    /// `contains`, only the tags holding the text. Blocked senders', snoozed
    /// and muted mail is counted as the stored tags are. A blank text, an
    /// unreadable instant or a backwards window is `MESSAGE_FILTER_INVALID`.
    pub fn list_tags(&self, organization_id: &str, filter: &TagFilter) -> Result<Vec<TagCount>, AppError> {
        let contains = super::super::messages::filter_value(filter.contains.as_deref(), "contains")?;
        let counted = match filter.since.is_some() || filter.until.is_some() {
            false => self.database.list_tags(organization_id, filter.mailbox_id)?,
            true => {
                let window = MessageFilters {
                    include_blocked: true,
                    include_snoozed: true,
                    include_muted: true,
                    since: filter.since.as_deref(),
                    until: filter.until.as_deref(),
                    ..MessageFilters::default()
                };
                // Each tag with the messages in the window carrying it.
                let mut carriers: BTreeMap<String, Vec<Uuid>> = BTreeMap::new();
                for message in self.list_messages(organization_id, filter.mailbox_id, window, None, None)? {
                    for tag in message.tags {
                        carriers.entry(tag).or_default().push(message.id);
                    }
                }
                let mut counted: Vec<(String, i64)> =
                    carriers.into_iter().map(|(tag, messages)| (tag, messages.len() as i64)).collect();
                // Most used first; a stable sort keeps tags of one count in name order.
                counted.sort_by(|first, second| second.1.cmp(&first.1));
                counted
            }
        };
        Ok(counted
            .into_iter()
            .filter(|(tag, _)| contains.as_deref().is_none_or(|part| tag.to_lowercase().contains(part)))
            .map(|(tag, messages)| TagCount { tag, messages })
            .collect())
    }
}

/// The tags listed as a CSV table (RFC 4180) under the header `tag,
/// messages`, in the order given: what `message tags --csv` writes.
pub fn tag_table(tags: &[TagCount]) -> String {
    let mut table = super::super::tables::header(&["tag", "messages"]);
    for counted in tags {
        table.push_str(&super::super::tables::row(&[counted.tag.clone(), counted.messages.to_string()]));
    }
    table
}
