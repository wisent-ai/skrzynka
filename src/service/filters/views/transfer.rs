//! Saved views carried between installations: the views a listing keeps,
//! written as one JSON document naming each view's mailbox by address, and
//! such a document read back, every view checked before any is kept.

use super::{checked_view, SavedFilters, SavedView};
use crate::{error::AppError, service::AppState};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// One view as an export writes it and an import reads it: its name, the
/// mailbox it keeps to by address (or ID), and its filters, whose
/// `mailbox_id` an export leaves empty and an import refuses.
#[derive(Debug, Serialize, Deserialize)]
pub struct CarriedView {
    pub name: String,
    #[serde(default)]
    pub mailbox: Option<String>,
    pub filters: SavedFilters,
}

/// What `message view import` did: every view kept, in file order; one
/// already of that name was replaced, as `--save-view` replaces it.
#[derive(Debug, Serialize)]
pub struct ImportedViews {
    pub views: Vec<SavedView>,
}

impl AppState {
    /// The views `views` keeps for `contains` as the JSON document
    /// `import_views` reads: `[{name, mailbox, filters}]` in name order.
    pub fn view_document(
        &self,
        organization_id: &str,
        contains: Option<&str>,
    ) -> Result<String, AppError> {
        let mut carried = Vec::new();
        for view in self.views(organization_id, contains)? {
            let mut filters = view.filters;
            let mailbox = match filters.mailbox_id.take() {
                Some(id) => Some(self.database.get_mailbox(organization_id, id)?.email),
                None => None,
            };
            carried.push(CarriedView {
                name: view.name,
                mailbox,
                filters,
            });
        }
        serde_json::to_string_pretty(&carried).map_err(|error| {
            AppError::internal(format!("the views could not be written as JSON: {error}"))
        })
    }

    /// Keep every view of a document `view_document` wrote. The whole
    /// document is read, every mailbox resolved by ID or address and every
    /// view checked as `--save-view` checks one before any is kept: a
    /// document that is not such a list, holds none, names one view twice,
    /// sets `filters.mailbox_id` or holds a view a save refuses is
    /// refused with that code (`VIEW_INVALID` for the document itself) and
    /// nothing changes.
    pub fn import_views(
        &self,
        organization_id: &str,
        text: &str,
    ) -> Result<ImportedViews, AppError> {
        let carried: Vec<CarriedView> = serde_json::from_str(text).map_err(|error| {
            AppError::invalid(
                "VIEW_INVALID",
                format!("the import is not a list of {{name, mailbox, filters}} views ({error}); nothing was imported"),
            )
        })?;
        if carried.is_empty() {
            return Err(AppError::invalid(
                "VIEW_INVALID",
                "the import holds no view; export some with message view export",
            ));
        }
        let mut seen = BTreeSet::new();
        let mut checked = Vec::new();
        for view in carried {
            let refused = |refusal: AppError| {
                AppError::new(
                    refusal.status,
                    refusal.code,
                    format!(
                        "the view {:?}: {}; nothing was imported",
                        view.name, refusal.message
                    ),
                    refusal.retryable,
                )
            };
            if view.filters.mailbox_id.is_some() {
                return Err(refused(AppError::invalid(
                    "VIEW_INVALID",
                    "it sets filters.mailbox_id; name its mailbox in mailbox, by ID or address",
                )));
            }
            let mut filters = view.filters.clone();
            filters.mailbox_id = match view.mailbox.as_deref() {
                Some(selector) => Some(
                    self.resolve_mailbox(organization_id, selector)
                        .map_err(refused)?
                        .id,
                ),
                None => None,
            };
            let name = checked_view(&view.name, &filters).map_err(refused)?;
            if !seen.insert(name.clone()) {
                return Err(AppError::invalid(
                    "VIEW_INVALID",
                    format!("the import names the view {name:?} twice; nothing was imported"),
                ));
            }
            checked.push((name, filters));
        }
        let mut views = Vec::new();
        for (name, filters) in &checked {
            views.push(self.database.save_view(organization_id, name, filters)?);
        }
        Ok(ImportedViews { views })
    }
}
