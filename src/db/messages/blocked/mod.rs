//! Senders the operator blocked, per organization.

use super::super::sql::params;
use super::super::Database;
use crate::error::AppError;
use chrono::Utc;

impl Database {
    /// Every blocked entry of the organization, in address order.
    pub fn blocked_senders(&self, organization_id: &str) -> Result<Vec<String>, AppError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT address FROM blocked_senders WHERE organization_id=$1 ORDER BY address",
        )?;
        let rows = statement.query_map(params![organization_id], |row| {
            row.get::<_, String>("address")
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Block `address`; blocking one already blocked is no change.
    pub fn block_sender(&self, organization_id: &str, address: &str) -> Result<(), AppError> {
        self.lock()?.execute(
            "INSERT INTO blocked_senders (organization_id, address, created_at)
             VALUES ($1, $2, $3) ON CONFLICT (organization_id, address) DO NOTHING",
            params![organization_id, address, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    /// Unblock `address`.
    pub fn unblock_sender(&self, organization_id: &str, address: &str) -> Result<(), AppError> {
        self.lock()?.execute(
            "DELETE FROM blocked_senders WHERE organization_id=$1 AND address=$2",
            params![organization_id, address],
        )?;
        Ok(())
    }
}
