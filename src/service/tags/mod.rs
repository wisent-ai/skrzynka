//! The operator's own labels on stored messages: added and removed through
//! Skrzynka only, never sent to the provider, and read back with every
//! message and as a filter of the listing.

use super::AppState;
use crate::{error::AppError, models::Message};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

mod counts;
pub use counts::{tag_table, TagFilter};

/// What `message tag` changes: tags to add and tags to remove.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct RetagRequest {
    #[serde(default)]
    pub add: Vec<String>,
    #[serde(default)]
    pub remove: Vec<String>,
}

/// One tag the organization uses and how many messages carry it.
#[derive(Debug, Serialize)]
pub struct TagCount {
    pub tag: String,
    pub messages: i64,
}

/// What `message tag --view` changed: the view and every message it listed.
#[derive(Debug, Serialize)]
pub struct RetaggedView {
    pub view: String,
    pub messages: Vec<Uuid>,
}

/// What `message tag --thread` changed: the message named and every message
/// of its conversation.
#[derive(Debug, Serialize)]
pub struct RetaggedThread {
    pub thread: Uuid,
    pub messages: Vec<Uuid>,
}

/// The tags to add and remove as stored: an empty change, and a tag both
/// added and removed, are refused.
fn validated(request: &RetagRequest) -> Result<(Vec<String>, Vec<String>), AppError> {
    let add = normalized(&request.add)?;
    let remove = normalized(&request.remove)?;
    if add.is_empty() && remove.is_empty() {
        return Err(AppError::invalid(
            "MESSAGE_TAG_INVALID",
            "name a tag to add or to remove",
        ));
    }
    if let Some(both) = add.iter().find(|tag| remove.contains(tag)) {
        return Err(AppError::invalid(
            "MESSAGE_TAG_INVALID",
            format!("tag {both:?} is both added and removed; name it once"),
        ));
    }
    Ok((add, remove))
}

impl AppState {
    /// Add and remove tags on one stored message and answer it. A tag is
    /// stored trimmed and in lower case; an empty tag, one with a comma or a
    /// control character, and a tag both added and removed are refused.
    pub fn retag_message(
        &self,
        organization_id: &str,
        message_id: Uuid,
        request: RetagRequest,
    ) -> Result<Message, AppError> {
        let (add, remove) = validated(&request)?;
        self.database
            .retag_message(organization_id, message_id, &add, &remove)
    }

    /// Add and remove tags on every stored message the saved view `view`
    /// lists, and answer which messages they were. The change is checked
    /// once before any message is touched.
    pub fn retag_view(
        &self,
        organization_id: &str,
        view: &str,
        request: RetagRequest,
    ) -> Result<RetaggedView, AppError> {
        let (add, remove) = validated(&request)?;
        let mut messages = Vec::new();
        for message in self.list_view(organization_id, view, None, None)? {
            self.database
                .retag_message(organization_id, message.id, &add, &remove)?;
            messages.push(message.id);
        }
        Ok(RetaggedView {
            view: view.trim().to_string(),
            messages,
        })
    }

    /// Add and remove tags on every stored message of the conversation `id`
    /// belongs to, as `message thread` lists it, and answer which messages
    /// they were: a label on a whole conversation. Later mail of it is not
    /// tagged by this; a tag rule tags arriving mail.
    pub fn retag_thread(&self, organization_id: &str, id: Uuid, request: RetagRequest) -> Result<RetaggedThread, AppError> {
        let (add, remove) = validated(&request)?;
        let mut messages = Vec::new();
        for message in self.message_thread(organization_id, id)? {
            self.database.retag_message(organization_id, message.id, &add, &remove)?;
            messages.push(message.id);
        }
        Ok(RetaggedThread { thread: id, messages })
    }

    /// Rename a tag everywhere the organization uses it: on its messages
    /// (merged into `to` where a message carries both), in its tag rules and
    /// in its saved views. A tag none of them uses is `MESSAGE_TAG_NOT_FOUND`.
    pub fn rename_tag(&self, organization_id: &str, from: &str, to: &str) -> Result<TagChange, AppError> {
        let (from, to) = (normalized_tag(from)?, normalized_tag(to)?);
        if from == to {
            return Err(AppError::invalid(
                "MESSAGE_TAG_INVALID",
                format!("{from:?} is already the tag's name; name another"),
            ));
        }
        let ruled = self.database.tag_rules(organization_id)?.iter().any(|rule| rule.tag == from);
        let viewed = self.views(organization_id, None)?.iter().any(|view| view.filters.tag.as_deref() == Some(from.as_str()));
        if !ruled && !viewed && !self.tag_in_use(organization_id, &from)? {
            return Err(unused(&from));
        }
        let (messages, rules, views) = self.database.rename_tag(organization_id, &from, &to)?;
        Ok(TagChange { tag: from, to: Some(to), messages, rules: Some(rules), views: Some(views) })
    }

    /// Take a tag off every message of the organization. Tag rules and saved
    /// views naming it are kept: a rule tags new mail with it again.
    pub fn remove_tag(&self, organization_id: &str, tag: &str) -> Result<TagChange, AppError> {
        let tag = normalized_tag(tag)?;
        if !self.tag_in_use(organization_id, &tag)? {
            return Err(unused(&tag));
        }
        let messages = self.database.remove_tag(organization_id, &tag)?;
        Ok(TagChange { tag, to: None, messages, rules: None, views: None })
    }

    fn tag_in_use(&self, organization_id: &str, tag: &str) -> Result<bool, AppError> {
        Ok(self.database.list_tags(organization_id, None)?.iter().any(|(used, _)| used == tag))
    }
}

/// What renaming or removing a tag changed: the tag, its new name when it
/// was renamed, how many messages carried it, and, for a rename, how many
/// tag rules and saved views were renamed with it.
#[derive(Debug, Serialize)]
pub struct TagChange {
    pub tag: String,
    pub to: Option<String>,
    pub messages: u64,
    pub rules: Option<u64>,
    pub views: Option<u64>,
}

fn unused(tag: &str) -> AppError {
    AppError::new(
        axum::http::StatusCode::NOT_FOUND,
        "MESSAGE_TAG_NOT_FOUND",
        format!("no message, rule or view of the organization uses the tag {tag:?}; `message tags` lists the ones messages carry"),
        false,
    )
}

/// The tags as stored: trimmed, lower case, each once.
fn normalized(tags: &[String]) -> Result<Vec<String>, AppError> {
    let mut stored: Vec<String> = Vec::new();
    for written in tags {
        let tag = normalized_tag(written)?;
        if !stored.contains(&tag) {
            stored.push(tag);
        }
    }
    Ok(stored)
}

/// One tag as stored: trimmed and lower case, refused when it is empty or
/// holds a comma or a control character.
pub(super) fn normalized_tag(written: &str) -> Result<String, AppError> {
    let tag = written.trim().to_lowercase();
    if tag.is_empty() || tag.contains(',') || tag.chars().any(char::is_control) {
        return Err(AppError::invalid(
            "MESSAGE_TAG_INVALID",
            format!(
                "tag {written:?} is not a tag: write a non-empty name without commas or control characters"
            ),
        ));
    }
    Ok(tag)
}
