//! Saved views: a listing's filters kept under a name, per organization.

use super::super::sql::{self, params, OptionalExtension, Row};
use super::super::Database;
use crate::{
    error::AppError,
    service::{SavedFilters, SavedView},
};
use chrono::Utc;

impl Database {
    /// Every view of the organization, in name order.
    pub fn views(&self, organization_id: &str) -> Result<Vec<SavedView>, AppError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT name, filters, updated_at FROM saved_views
             WHERE organization_id=$1 ORDER BY name",
        )?;
        let rows = statement.query_map(params![organization_id], stored)?;
        rows.collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(view_from_stored)
            .collect()
    }

    /// One view by name; absent when the organization has none of it.
    pub fn view(&self, organization_id: &str, name: &str) -> Result<Option<SavedView>, AppError> {
        self.lock()?
            .query_row(
                "SELECT name, filters, updated_at FROM saved_views
                 WHERE organization_id=$1 AND name=$2",
                params![organization_id, name],
                stored,
            )
            .optional()?
            .map(view_from_stored)
            .transpose()
    }

    /// Keep the filters under the name, replacing a view of the same name.
    pub fn save_view(
        &self,
        organization_id: &str,
        name: &str,
        filters: &SavedFilters,
    ) -> Result<SavedView, AppError> {
        let written = serde_json::to_string(filters).map_err(|error| {
            AppError::internal(format!(
                "view {name:?} could not be written as JSON: {error}"
            ))
        })?;
        let row = self.lock()?.query_row(
            "INSERT INTO saved_views (organization_id, name, filters, updated_at)
             VALUES ($1, $2, $3, $4) ON CONFLICT (organization_id, name)
             DO UPDATE SET filters=EXCLUDED.filters, updated_at=EXCLUDED.updated_at
             RETURNING name, filters, updated_at",
            params![organization_id, name, written, Utc::now().to_rfc3339()],
            stored,
        )?;
        view_from_stored(row)
    }

    /// Give the view `from` the name `to`, keeping its filters; absent when
    /// there is no view `from`, or one named `to` already.
    pub fn rename_view(
        &self,
        organization_id: &str,
        from: &str,
        to: &str,
    ) -> Result<Option<SavedView>, AppError> {
        self.lock()?
            .query_row(
                "UPDATE saved_views SET name=$3, updated_at=$4
                 WHERE organization_id=$1 AND name=$2
                   AND NOT EXISTS (SELECT name FROM saved_views WHERE organization_id=$1 AND name=$3)
                 RETURNING name, filters, updated_at",
                params![organization_id, from, to, Utc::now().to_rfc3339()],
                stored,
            )
            .optional()?
            .map(view_from_stored)
            .transpose()
    }

    /// Delete the view and answer it; absent when there was none.
    pub fn delete_view(
        &self,
        organization_id: &str,
        name: &str,
    ) -> Result<Option<SavedView>, AppError> {
        self.lock()?
            .query_row(
                "DELETE FROM saved_views WHERE organization_id=$1 AND name=$2
                 RETURNING name, filters, updated_at",
                params![organization_id, name],
                stored,
            )
            .optional()?
            .map(view_from_stored)
            .transpose()
    }
}

/// A view row as stored: name, filters JSON, and when it was saved.
fn stored(row: &Row<'_>) -> sql::Result<(String, String, String)> {
    Ok((
        row.get("name")?,
        row.get("filters")?,
        row.get("updated_at")?,
    ))
}

/// A stored view read back; filters that no longer read are refused with
/// the view's name and the cause, never read as no filter.
fn view_from_stored(
    (name, filters, updated_at): (String, String, String),
) -> Result<SavedView, AppError> {
    let filters = serde_json::from_str(&filters).map_err(|error| {
        AppError::internal(format!(
            "view {name:?} holds filters that do not read: {error}"
        ))
    })?;
    Ok(SavedView {
        name,
        filters,
        updated_at,
    })
}
