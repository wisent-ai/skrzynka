//! Recipients the operator must not send to, per organization: an address,
//! or `@domain` for every address of a domain, with why.

use super::super::sql::{self, params, OptionalExtension, Row};
use super::super::Database;
use crate::{error::AppError, mail::Suppression};
use chrono::Utc;

impl Database {
    /// Every entry of the organization, in entry order.
    pub fn suppressions(&self, organization_id: &str) -> Result<Vec<Suppression>, AppError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT entry, reason, added_at FROM suppressed_recipients WHERE organization_id=$1 ORDER BY entry",
        )?;
        let rows = statement.query_map(params![organization_id], suppression_from_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Store the entry with its reason, replacing the reason of one already
    /// there, and answer it.
    pub fn suppress(&self, organization_id: &str, entry: &str, reason: Option<&str>) -> Result<Suppression, AppError> {
        self.lock()?
            .query_row(
                "INSERT INTO suppressed_recipients (organization_id, entry, reason, added_at)
                 VALUES ($1, $2, $3, $4) ON CONFLICT (organization_id, entry) DO UPDATE SET reason=EXCLUDED.reason
                 RETURNING entry, reason, added_at",
                params![organization_id, entry, reason, Utc::now().to_rfc3339()],
                suppression_from_row,
            )
            .map_err(Into::into)
    }

    /// Remove the entry and answer what it was; absent when there was none.
    pub fn unsuppress(&self, organization_id: &str, entry: &str) -> Result<Option<Suppression>, AppError> {
        self.lock()?
            .query_row(
                "DELETE FROM suppressed_recipients WHERE organization_id=$1 AND entry=$2
                 RETURNING entry, reason, added_at",
                params![organization_id, entry],
                suppression_from_row,
            )
            .optional()
            .map_err(Into::into)
    }
}

fn suppression_from_row(row: &Row<'_>) -> sql::Result<Suppression> {
    Ok(Suppression {
        entry: row.get("entry")?,
        reason: row.get("reason")?,
        added_at: row.get("added_at")?,
    })
}
