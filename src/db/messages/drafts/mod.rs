//! Draft rows: created, changed, read and removed, each scoped to the
//! organization through its mailbox.

use super::super::sql::{self, params, OptionalExtension, Row};
use super::super::{parse_uuid, Database};
use crate::{
    error::AppError,
    mail::{prepare_attachments, Draft, OutgoingAttachment},
};
use chrono::Utc;
use uuid::Uuid;

mod schedule;

/// The columns and organization scope every draft read uses.
const SELECT: &str =
    "SELECT drafts.id, drafts.mailbox_id, drafts.recipients, drafts.cc, drafts.bcc,
        drafts.subject, drafts.body, drafts.attachments, drafts.created_at, drafts.updated_at,
        drafts.send_at, drafts.send_key, drafts.schedule_error
     FROM drafts JOIN mailboxes ON mailboxes.id=drafts.mailbox_id";

impl Database {
    /// Store a new draft from `mailbox_id` and answer it.
    pub fn create_draft(
        &self,
        organization_id: &str,
        mailbox_id: Uuid,
        fields: &Draft,
    ) -> Result<Draft, AppError> {
        self.get_mailbox(organization_id, mailbox_id)?;
        let id = Uuid::new_v4();
        let now = Utc::now().to_rfc3339();
        self.lock()?.execute(
            "INSERT INTO drafts (id, mailbox_id, recipients, cc, bcc, subject, body, attachments, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $9)",
            params![
                id.to_string(),
                mailbox_id.to_string(),
                fields.recipients,
                fields.cc,
                fields.bcc,
                fields.subject,
                fields.body,
                files_json(&fields.files)?,
                now,
            ],
        )?;
        self.get_draft(organization_id, id)
    }

    /// Replace the draft's fields with `fields` and answer it.
    pub fn update_draft(&self, organization_id: &str, fields: &Draft) -> Result<Draft, AppError> {
        self.get_draft(organization_id, fields.id)?;
        self.lock()?.execute(
            "UPDATE drafts SET recipients=$2, cc=$3, bcc=$4, subject=$5, body=$6, attachments=$7, updated_at=$8
             WHERE id=$1",
            params![
                fields.id.to_string(),
                fields.recipients,
                fields.cc,
                fields.bcc,
                fields.subject,
                fields.body,
                files_json(&fields.files)?,
                Utc::now().to_rfc3339(),
            ],
        )?;
        self.get_draft(organization_id, fields.id)
    }

    pub fn get_draft(&self, organization_id: &str, id: Uuid) -> Result<Draft, AppError> {
        self.lock()?
            .query_row(
                &format!("{SELECT} WHERE drafts.id=$1 AND mailboxes.organization_id=$2"),
                params![id.to_string(), organization_id],
                draft_from_row,
            )
            .optional()?
            .ok_or_else(|| AppError::not_found("draft"))
    }

    /// Every draft, or one mailbox's, most recently changed first.
    pub fn list_drafts(
        &self,
        organization_id: &str,
        mailbox_id: Option<Uuid>,
    ) -> Result<Vec<Draft>, AppError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(&format!(
            "{SELECT} WHERE mailboxes.organization_id=$1
               AND ($2::text IS NULL OR drafts.mailbox_id=$2)
             ORDER BY drafts.updated_at DESC"
        ))?;
        let rows = statement.query_map(
            params![organization_id, mailbox_id.map(|id| id.to_string())],
            draft_from_row,
        )?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Remove the draft; one the organization does not hold is not found.
    pub fn delete_draft(&self, organization_id: &str, id: Uuid) -> Result<(), AppError> {
        self.get_draft(organization_id, id)?;
        self.lock()?
            .execute("DELETE FROM drafts WHERE id=$1", params![id.to_string()])?;
        Ok(())
    }
}

/// The draft's files as the `attachments` column keeps them.
fn files_json(files: &[OutgoingAttachment]) -> Result<String, AppError> {
    serde_json::to_string(files).map_err(|cause| {
        AppError::internal(format!(
            "the draft's files could not be written as JSON: {cause}"
        ))
    })
}

fn draft_from_row(row: &Row<'_>) -> sql::Result<Draft> {
    let stored = row.get::<_, String>("attachments")?;
    let files: Vec<OutgoingAttachment> = serde_json::from_str(&stored).map_err(|error| {
        sql::Error::Conversion(format!(
            "column attachments of a draft holds no list of files: {error}"
        ))
    })?;
    let attachments = prepare_attachments(&files)
        .map_err(|error| {
            sql::Error::Conversion(format!(
                "a file the draft keeps no longer reads: {}",
                error.message
            ))
        })?
        .into_iter()
        .map(|prepared| prepared.record)
        .collect();
    Ok(Draft {
        id: parse_uuid(row.get::<_, String>("id")?)?,
        mailbox_id: parse_uuid(row.get::<_, String>("mailbox_id")?)?,
        recipients: row.get("recipients")?,
        cc: row.get("cc")?,
        bcc: row.get("bcc")?,
        subject: row.get("subject")?,
        body: row.get("body")?,
        files,
        attachments,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
        send_at: row.get("send_at")?,
        send_key: row.get("send_key")?,
        schedule_error: row.get("schedule_error")?,
    })
}
