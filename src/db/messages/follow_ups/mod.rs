//! Follow-ups: an originated message the operator expects an answer to by
//! an instant.

use super::super::sql::{params, OptionalExtension};
use super::super::{parse_uuid, Database};
use crate::error::AppError;
use chrono::Utc;
use uuid::Uuid;

impl Database {
    /// Expect an answer to the outbound message by `due_at`, replacing an
    /// earlier instant. The message must belong to the organization.
    pub fn set_follow_up(
        &self,
        organization_id: &str,
        outbound_id: Uuid,
        due_at: &str,
    ) -> Result<(), AppError> {
        self.get_outbound(organization_id, outbound_id)?;
        self.lock()?.execute(
            "INSERT INTO follow_ups (outbound_id, due_at, created_at) VALUES ($1, $2, $3)
             ON CONFLICT (outbound_id) DO UPDATE SET due_at=EXCLUDED.due_at",
            params![outbound_id.to_string(), due_at, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    /// Stop expecting an answer; whether a follow-up was set.
    pub fn clear_follow_up(
        &self,
        organization_id: &str,
        outbound_id: Uuid,
    ) -> Result<bool, AppError> {
        self.get_outbound(organization_id, outbound_id)?;
        let cleared = self
            .lock()?
            .query_row(
                "DELETE FROM follow_ups WHERE outbound_id=$1 RETURNING outbound_id",
                params![outbound_id.to_string()],
                |row| row.get::<_, String>("outbound_id"),
            )
            .optional()?;
        Ok(cleared.is_some())
    }

    /// Every follow-up of the organization as `(outbound_id, due_at)`,
    /// earliest due first.
    pub fn follow_ups(&self, organization_id: &str) -> Result<Vec<(Uuid, String)>, AppError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT follow_ups.outbound_id, follow_ups.due_at FROM follow_ups
             JOIN outbound_messages ON outbound_messages.id=follow_ups.outbound_id
             JOIN mailboxes ON mailboxes.id=outbound_messages.mailbox_id
             WHERE mailboxes.organization_id=$1
             ORDER BY follow_ups.due_at, follow_ups.outbound_id",
        )?;
        let rows = statement.query_map(params![organization_id], |row| {
            Ok((
                parse_uuid(row.get("outbound_id")?)?,
                row.get::<_, String>("due_at")?,
            ))
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}
