//! Addresses the operator cleared after they bounced: the instant each was
//! cleared, before which a stored bounce report about it no longer counts.

use super::super::super::sql::params;
use super::super::super::Database;
use crate::error::AppError;
use std::collections::BTreeMap;

impl Database {
    /// Every cleared address of the organization with the instant it was cleared.
    pub fn bounce_clearances(
        &self,
        organization_id: &str,
    ) -> Result<BTreeMap<String, String>, AppError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT address, cleared_at FROM bounce_clearances WHERE organization_id=$1",
        )?;
        let rows = statement.query_map(params![organization_id], |row| {
            Ok((
                row.get::<_, String>("address")?,
                row.get::<_, String>("cleared_at")?,
            ))
        })?;
        rows.collect::<Result<BTreeMap<_, _>, _>>()
            .map_err(Into::into)
    }

    /// Clear `address` as of `cleared_at`, moving an earlier clearance forward.
    pub fn clear_bounced(
        &self,
        organization_id: &str,
        address: &str,
        cleared_at: &str,
    ) -> Result<(), AppError> {
        self.lock()?.execute(
            "INSERT INTO bounce_clearances (organization_id, address, cleared_at)
             VALUES ($1, $2, $3)
             ON CONFLICT (organization_id, address) DO UPDATE SET cleared_at=EXCLUDED.cleared_at",
            params![organization_id, address, cleared_at],
        )?;
        Ok(())
    }
}
