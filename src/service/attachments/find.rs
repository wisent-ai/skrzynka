//! Every file the stored mail carries, as the imports recorded them, found
//! across messages: by name, type, size, sender and mailbox. Nothing is read
//! from a provider; a message imported before its files were recorded names
//! none until `message attachment list` records them.

use super::super::{messages::filter_value, AppState, MessageFilters};
use crate::error::AppError;
use serde::Serialize;
use uuid::Uuid;

/// Which recorded files a search keeps; each absent one is no filter.
#[derive(Debug, Default, Clone)]
pub struct FileFilter {
    /// Only this mailbox's mail.
    pub mailbox_id: Option<Uuid>,
    /// Only files whose recorded name contains this, case aside.
    pub name: Option<String>,
    /// Only files whose content type contains this, case aside (`pdf`, `image/`).
    pub content_type: Option<String>,
    /// Only files of at least this many recorded bytes.
    pub over: Option<u64>,
    /// Only files of mail whose From header contains this, case aside.
    pub sender: Option<String>,
    /// Only files of mail received at or after this RFC 3339 instant,
    /// `YYYY-MM-DD` day or span back such as `-30d`.
    pub since: Option<String>,
    /// Only files of mail received before this.
    pub until: Option<String>,
    /// Only files of mail carrying this tag, as `message list --tag` keeps it.
    pub tag: Option<String>,
    /// List the largest files first, those of no recorded size last, to
    /// find what takes the room; otherwise newest message first.
    pub largest_first: bool,
}

/// One recorded file and the message carrying it.
#[derive(Debug, Serialize)]
pub struct StoredFile {
    pub message_id: Uuid,
    pub mailbox_id: Uuid,
    pub sender: String,
    pub subject: Option<String>,
    pub received_at: String,
    /// The file's index among the message's attachments, as `message
    /// attachment save --index` takes it.
    pub index: usize,
    pub filename: Option<String>,
    pub content_type: String,
    pub size_bytes: Option<u64>,
}

impl AppState {
    /// Every file the stored mail of the organization, or of one mailbox,
    /// carries, newest message first and in each message's own order, or
    /// with `largest_first` the largest first; blocked senders', snoozed and
    /// muted mail included. A blank text filter, an unreadable instant or a
    /// backwards window is `MESSAGE_FILTER_INVALID`. A file whose name or
    /// size was not recorded passes no name or size filter.
    pub fn find_files(&self, organization_id: &str, filter: &FileFilter) -> Result<Vec<StoredFile>, AppError> {
        let name = filter_value(filter.name.as_deref(), "name")?;
        let content_type = filter_value(filter.content_type.as_deref(), "type")?;
        let sender = filter_value(filter.sender.as_deref(), "sender")?;
        let carrying = MessageFilters {
            with_attachments: true,
            include_blocked: true,
            include_snoozed: true,
            include_muted: true,
            since: filter.since.as_deref(),
            until: filter.until.as_deref(),
            tag: filter.tag.as_deref(),
            ..MessageFilters::default()
        };
        let mut found = Vec::new();
        for message in self.list_messages(organization_id, filter.mailbox_id, carrying, None, None)? {
            if !sender.as_deref().is_none_or(|part| message.sender.to_lowercase().contains(part)) {
                continue;
            }
            for file in message.attachments.iter().flatten() {
                let named = name.as_deref().is_none_or(|part| {
                    file.filename.as_deref().is_some_and(|written| written.to_lowercase().contains(part))
                });
                let typed = content_type.as_deref().is_none_or(|part| file.content_type.to_lowercase().contains(part));
                let large = filter.over.is_none_or(|least| file.size_bytes.is_some_and(|size| size >= least));
                if named && typed && large {
                    found.push(StoredFile {
                        message_id: message.id,
                        mailbox_id: message.mailbox_id,
                        sender: message.sender.clone(),
                        subject: message.subject.clone(),
                        received_at: message.received_at.clone(),
                        index: file.index,
                        filename: file.filename.clone(),
                        content_type: file.content_type.clone(),
                        size_bytes: file.size_bytes,
                    });
                }
            }
        }
        if filter.largest_first {
            // Stable: files of one size keep their newest-first order.
            found.sort_by(|first, second| second.size_bytes.cmp(&first.size_bytes));
        }
        Ok(found)
    }
}
