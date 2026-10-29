//! Message templates: texts the operator keeps to reuse, per organization.

use super::super::sql::{self, params, OptionalExtension, Row};
use super::super::Database;
use crate::{error::AppError, mail::MessageTemplate};
use chrono::Utc;

impl Database {
    /// Every template of the organization, in name order.
    pub fn templates(&self, organization_id: &str) -> Result<Vec<MessageTemplate>, AppError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT name, subject, body, updated_at FROM message_templates
             WHERE organization_id=$1 ORDER BY name",
        )?;
        let rows = statement.query_map(params![organization_id], template_from_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// One template by its name; absent when the organization has none of it.
    pub fn template(
        &self,
        organization_id: &str,
        name: &str,
    ) -> Result<Option<MessageTemplate>, AppError> {
        self.lock()?
            .query_row(
                "SELECT name, subject, body, updated_at FROM message_templates
                 WHERE organization_id=$1 AND name=$2",
                params![organization_id, name],
                template_from_row,
            )
            .optional()
            .map_err(Into::into)
    }

    /// Store the template, replacing the subject and text of one of the same name.
    pub fn save_template(
        &self,
        organization_id: &str,
        name: &str,
        subject: Option<&str>,
        body: &str,
    ) -> Result<MessageTemplate, AppError> {
        self.lock()?
            .query_row(
                "INSERT INTO message_templates (organization_id, name, subject, body, updated_at)
                 VALUES ($1, $2, $3, $4, $5) ON CONFLICT (organization_id, name)
                 DO UPDATE SET subject=EXCLUDED.subject, body=EXCLUDED.body, updated_at=EXCLUDED.updated_at
                 RETURNING name, subject, body, updated_at",
                params![organization_id, name, subject, body, Utc::now().to_rfc3339()],
                template_from_row,
            )
            .map_err(Into::into)
    }

    /// Give the template `from` the name `to`, keeping its text; absent when
    /// there is no template `from`, or one named `to` already.
    pub fn rename_template(
        &self,
        organization_id: &str,
        from: &str,
        to: &str,
    ) -> Result<Option<MessageTemplate>, AppError> {
        self.lock()?
            .query_row(
                "UPDATE message_templates SET name=$3, updated_at=$4
                 WHERE organization_id=$1 AND name=$2
                   AND NOT EXISTS (SELECT name FROM message_templates WHERE organization_id=$1 AND name=$3)
                 RETURNING name, subject, body, updated_at",
                params![organization_id, from, to, Utc::now().to_rfc3339()],
                template_from_row,
            )
            .optional()
            .map_err(Into::into)
    }

    /// Delete the template and answer it; absent when there was none.
    pub fn delete_template(
        &self,
        organization_id: &str,
        name: &str,
    ) -> Result<Option<MessageTemplate>, AppError> {
        self.lock()?
            .query_row(
                "DELETE FROM message_templates WHERE organization_id=$1 AND name=$2
                 RETURNING name, subject, body, updated_at",
                params![organization_id, name],
                template_from_row,
            )
            .optional()
            .map_err(Into::into)
    }
}

fn template_from_row(row: &Row<'_>) -> sql::Result<MessageTemplate> {
    let subject: Option<String> = row.get("subject")?;
    let body: String = row.get("body")?;
    let named = [subject.as_deref(), Some(body.as_str())]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join("\n");
    Ok(MessageTemplate {
        name: row.get("name")?,
        placeholders: crate::mail::placeholders::placeholders(&named),
        subject,
        body,
        updated_at: row.get("updated_at")?,
    })
}
