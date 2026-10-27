//! Saved views: a listing's filters kept under a name, so the same listing
//! is one word away (`message list --view <NAME>`, `GET /v1/messages?view=`).
//! A view stores the filters as written and is checked as a listing checks
//! them, when saved and again each time it is read.

use super::MessageFilters;
use crate::{error::AppError, models::Message, service::AppState};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

mod counted;
mod transfer;
pub use counted::CountedView;

/// The filters a view keeps, owned; each as `message list` takes it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedFilters {
    #[serde(default)]
    pub mailbox_id: Option<Uuid>,
    #[serde(default)]
    pub recipient: Option<String>,
    #[serde(default)]
    pub cc: Option<String>,
    #[serde(default)]
    pub sender: Option<String>,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub since: Option<String>,
    #[serde(default)]
    pub until: Option<String>,
    #[serde(default)]
    pub tag: Option<String>,
    #[serde(default)]
    pub list: Option<String>,
    #[serde(default)]
    pub attachment: Option<String>,
    #[serde(default)]
    pub with_attachments: bool,
    #[serde(default)]
    pub bounces: bool,
    #[serde(default)]
    pub include_blocked: bool,
    #[serde(default)]
    pub include_snoozed: bool,
    #[serde(default)]
    pub include_muted: bool,
    #[serde(default)]
    pub unanswered: bool,
    #[serde(default)]
    pub failed_authentication: bool,
    #[serde(default)]
    pub oldest_first: bool,
    #[serde(default)]
    pub snoozed: bool,
    #[serde(default)]
    pub noted: bool,
    #[serde(default)]
    pub muted: bool,
    #[serde(default)]
    pub untagged: bool,
    #[serde(default)]
    pub answered: bool,
    #[serde(default)]
    pub files_over: Option<u64>,
}

impl SavedFilters {
    /// The filters as a listing takes them.
    pub fn filters(&self) -> MessageFilters<'_> {
        MessageFilters {
            recipient: self.recipient.as_deref(),
            cc: self.cc.as_deref(),
            sender: self.sender.as_deref(),
            text: self.text.as_deref(),
            since: self.since.as_deref(),
            until: self.until.as_deref(),
            tag: self.tag.as_deref(),
            list: self.list.as_deref(),
            attachment: self.attachment.as_deref(),
            with_attachments: self.with_attachments,
            bounces: self.bounces,
            include_blocked: self.include_blocked,
            include_snoozed: self.include_snoozed,
            include_muted: self.include_muted,
            unanswered: self.unanswered,
            failed_authentication: self.failed_authentication,
            oldest_first: self.oldest_first,
            snoozed: self.snoozed,
            noted: self.noted,
            muted: self.muted,
            untagged: self.untagged,
            answered: self.answered,
            files_over: self.files_over,
        }
    }
}

/// One view as `message view list` prints it.
#[derive(Debug, Clone, Serialize)]
pub struct SavedView {
    pub name: String,
    pub filters: SavedFilters,
    pub updated_at: String,
}

impl AppState {
    /// Keep `filters` under `name`, replacing a view of the same name. The
    /// filters are checked as a listing checks them, and a view that keeps
    /// every message is refused: `message list` already is that.
    pub fn save_view(
        &self,
        organization_id: &str,
        name: &str,
        filters: &SavedFilters,
    ) -> Result<SavedView, AppError> {
        let name = checked_view(name, filters)?;
        self.database.save_view(organization_id, &name, filters)
    }

    /// Every view of the organization, by name; with `contains` only those whose name holds it, case aside.
    pub fn views(&self, organization_id: &str, contains: Option<&str>) -> Result<Vec<SavedView>, AppError> {
        let part = super::super::messages::filter_value(contains, "contains")?;
        let named = |view: &SavedView| part.as_deref().is_none_or(|text| view.name.to_lowercase().contains(text));
        Ok(self.database.views(organization_id)?.into_iter().filter(named).collect())
    }

    /// One view by name; an unknown name is refused naming the ones there are.
    pub fn view(&self, organization_id: &str, name: &str) -> Result<SavedView, AppError> {
        let name = view_name(name)?;
        match self.database.view(organization_id, &name)? {
            Some(view) => Ok(view),
            None => Err(self.unknown_view(organization_id, &name)?),
        }
    }

    /// Give a view another name, keeping its filters. The new name is held to
    /// the rules a saved one is; a name another view carries is `VIEW_EXISTS`.
    pub fn rename_view(
        &self,
        organization_id: &str,
        name: &str,
        to: &str,
    ) -> Result<SavedView, AppError> {
        let from = view_name(name)?;
        let to = view_name(to)?;
        if from == to {
            return self.view(organization_id, &from);
        }
        self.refuse_taken_view(organization_id, &to)?;
        match self.database.rename_view(organization_id, &from, &to)? {
            Some(view) => Ok(view),
            None => Err(self.unknown_view(organization_id, &from)?),
        }
    }

    /// Keep a view's filters and mailbox under another name too, to change
    /// one without the other. The new name is held to the rules a saved one
    /// is; a name another view carries, the view's own included, is
    /// `VIEW_EXISTS`, and an unknown view `VIEW_NOT_FOUND`.
    pub fn copy_view(&self, organization_id: &str, name: &str, to: &str) -> Result<SavedView, AppError> {
        let source = self.view(organization_id, name)?;
        let to = view_name(to)?;
        self.refuse_taken_view(organization_id, &to)?;
        self.database.save_view(organization_id, &to, &source.filters)
    }

    /// `VIEW_EXISTS` when a view carries `name` already.
    fn refuse_taken_view(&self, organization_id: &str, name: &str) -> Result<(), AppError> {
        match self.database.view(organization_id, name)? {
            Some(other) => Err(AppError::conflict(
                "VIEW_EXISTS",
                format!(
                    "a view {:?} exists already; rename or delete it first, or choose another name",
                    other.name
                ),
            )),
            None => Ok(()),
        }
    }

    /// Delete the view and answer what it kept.
    pub fn delete_view(&self, organization_id: &str, name: &str) -> Result<SavedView, AppError> {
        let name = view_name(name)?;
        match self.database.delete_view(organization_id, &name)? {
            Some(view) => Ok(view),
            None => Err(self.unknown_view(organization_id, &name)?),
        }
    }

    /// The stored messages the view keeps, as `message list` lists them.
    pub fn list_view(
        &self,
        organization_id: &str,
        name: &str,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> Result<Vec<Message>, AppError> {
        let view = self.view(organization_id, name)?;
        self.list_messages(
            organization_id,
            view.filters.mailbox_id,
            view.filters.filters(),
            limit,
            offset,
        )
    }

    /// A listing as `message list` and `GET /v1/messages` take it: the
    /// messages of the view `view` names, which keeps its own filters, so
    /// any other filter beside it is refused; otherwise those of `filters`,
    /// kept first as the view `save_as` names when there is one.
    pub fn list_listing(
        &self,
        organization_id: &str,
        filters: &SavedFilters,
        view: Option<&str>,
        save_as: Option<&str>,
        limit: Option<u32>,
        offset: Option<u32>,
    ) -> Result<Vec<Message>, AppError> {
        if let Some(view) = view {
            if *filters != SavedFilters::default() || save_as.is_some() {
                return Err(AppError::invalid(
                    "MESSAGE_FILTER_INVALID",
                    format!("the view {view:?} keeps its own filters; leave the others out, or save new ones as a view"),
                ));
            }
            return self.list_view(organization_id, view, limit, offset);
        }
        if let Some(name) = save_as {
            self.save_view(organization_id, name, filters)?;
        }
        self.list_messages(organization_id, filters.mailbox_id, filters.filters(), limit, offset)
    }

    fn unknown_view(&self, organization_id: &str, name: &str) -> Result<AppError, AppError> {
        let names: Vec<String> = self
            .database
            .views(organization_id)?
            .into_iter()
            .map(|view| view.name)
            .collect();
        let known = match names.is_empty() {
            true => "there are none yet".to_string(),
            false => format!("there are: {}", names.join(", ")),
        };
        Ok(AppError::new(
            StatusCode::NOT_FOUND,
            "VIEW_NOT_FOUND",
            format!("there is no view {name:?}; {known}"),
            false,
        ))
    }
}

/// A view's name as stored, when `filters` narrow or order a listing as
/// `message list` takes them; a view that keeps every message is
/// `VIEW_INVALID`: `message list` already is that.
fn checked_view(name: &str, filters: &SavedFilters) -> Result<String, AppError> {
    let name = view_name(name)?;
    let narrows = filters.filters().matcher()?.is_some()
        || filters.mailbox_id.is_some()
        || filters.unanswered
        || filters.answered
        || filters.snoozed
        || filters.muted;
    let includes = filters.include_blocked || filters.include_snoozed || filters.include_muted || filters.oldest_first;
    if !narrows && !includes {
        return Err(AppError::invalid(
            "VIEW_INVALID",
            format!("view {name:?} names no filter; name at least one, as message list takes them"),
        ));
    }
    Ok(name)
}

/// A view's name as stored: trimmed, not empty, without a control character
/// or a `/`, which would not survive a URL path.
fn view_name(name: &str) -> Result<String, AppError> {
    let name = name.trim();
    if name.is_empty() || name.contains('/') || name.chars().any(char::is_control) {
        return Err(AppError::invalid(
            "VIEW_INVALID",
            format!("{name:?} is not a view name; name it with visible text, without a / or a control character"),
        ));
    }
    Ok(name.to_string())
}
