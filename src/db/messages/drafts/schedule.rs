//! A draft's schedule: when a running service sends it, under which key,
//! and why the last scheduled send did not end `sent`.

use super::super::super::sql::{params, OptionalExtension};
use super::super::super::{parse_uuid, Database};
use crate::{error::AppError, mail::Draft};
use uuid::Uuid;

/// A draft whose time has come: its organization, ID and key.
pub struct DueDraft {
    pub organization_id: String,
    pub id: Uuid,
    pub send_key: String,
}

impl Database {
    /// Schedule the draft for `send_at` (RFC 3339) under `send_key`, or
    /// unschedule it when both are absent, clearing any earlier error.
    pub fn schedule_draft(
        &self,
        organization_id: &str,
        id: Uuid,
        send_at: Option<&str>,
        send_key: Option<&str>,
    ) -> Result<Draft, AppError> {
        self.get_draft(organization_id, id)?;
        self.lock()?.execute(
            "UPDATE drafts SET send_at=$2, send_key=$3, schedule_error=NULL WHERE id=$1",
            params![id.to_string(), send_at, send_key],
        )?;
        self.get_draft(organization_id, id)
    }

    /// Every scheduled draft whose time is at or before `now` (RFC 3339).
    pub fn due_drafts(&self, now: &str) -> Result<Vec<DueDraft>, AppError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT mailboxes.organization_id AS organization_id, drafts.id AS id,
                    drafts.send_key AS send_key
             FROM drafts JOIN mailboxes ON mailboxes.id=drafts.mailbox_id
             WHERE drafts.send_at IS NOT NULL AND drafts.send_at <= $1
             ORDER BY drafts.send_at",
        )?;
        let rows = statement.query_map(params![now], |row| {
            Ok(DueDraft {
                organization_id: row.get("organization_id")?,
                id: parse_uuid(row.get::<_, String>("id")?)?,
                send_key: row.get("send_key")?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// The earliest time any draft is scheduled for, absent when none is.
    pub fn next_scheduled_draft(&self) -> Result<Option<String>, AppError> {
        self.lock()?
            .query_row(
                "SELECT MIN(send_at) AS send_at FROM drafts WHERE send_at IS NOT NULL",
                params![],
                |row| row.get::<_, Option<String>>("send_at"),
            )
            .optional()
            .map(Option::flatten)
            .map_err(Into::into)
    }

    /// A scheduled send that did not end `sent`: unschedule the draft and
    /// keep why, so it is not tried again unasked.
    pub fn record_schedule_failure(&self, id: Uuid, error: &str) -> Result<(), AppError> {
        self.lock()?.execute(
            "UPDATE drafts SET send_at=NULL, schedule_error=$2 WHERE id=$1",
            params![id.to_string(), error],
        )?;
        Ok(())
    }
}
