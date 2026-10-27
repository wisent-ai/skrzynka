//! The record of every change asked of a message at its provider: claimed
//! before the provider is asked, finished with what was observed, and read
//! back newest first.

use super::super::sql::{self, params, OptionalExtension, Row};
use super::super::Database;
use crate::{
    error::AppError,
    mail::{ActionStatus, MessageAction, ProviderAction},
};
use chrono::Utc;
use std::str::FromStr;
use uuid::Uuid;

const SELECT_ACTION: &str = "SELECT message_actions.id, message_actions.message_id,
        message_actions.action, message_actions.folder, message_actions.status,
        message_actions.error_code, message_actions.error_message,
        message_actions.requested_at, message_actions.completed_at
     FROM message_actions
     JOIN messages ON messages.id=message_actions.message_id
     JOIN mailboxes ON mailboxes.id=messages.mailbox_id";

impl Database {
    /// Claim a pending action on one of the organization's messages.
    pub fn begin_message_action(
        &self,
        organization_id: &str,
        message_id: Uuid,
        action: &ProviderAction,
    ) -> Result<MessageAction, AppError> {
        self.get_message(organization_id, message_id)?;
        let id = Uuid::new_v4();
        self.lock()?.execute(
            "INSERT INTO message_actions (id, message_id, action, folder, status, requested_at)
             VALUES ($1, $2, $3, $4, 'pending', $5)",
            params![
                id.to_string(),
                message_id.to_string(),
                action.kind().as_str(),
                action.folder(),
                Utc::now().to_rfc3339(),
            ],
        )?;
        self.get_message_action(organization_id, id)
    }

    /// Finish a claimed action with its observed status and, when it did not
    /// end done, the error that ended it.
    pub fn finish_message_action(
        &self,
        organization_id: &str,
        id: Uuid,
        status: ActionStatus,
        error_code: Option<&str>,
        error_message: Option<&str>,
    ) -> Result<MessageAction, AppError> {
        self.lock()?.execute(
            "UPDATE message_actions SET status=$2, error_code=$3, error_message=$4, completed_at=$5
             WHERE id=$1",
            params![
                id.to_string(),
                status.as_str(),
                error_code,
                error_message,
                Utc::now().to_rfc3339(),
            ],
        )?;
        self.get_message_action(organization_id, id)
    }

    pub fn get_message_action(
        &self,
        organization_id: &str,
        id: Uuid,
    ) -> Result<MessageAction, AppError> {
        self.lock()?
            .query_row(
                &format!(
                    "{SELECT_ACTION} WHERE message_actions.id=$1 AND mailboxes.organization_id=$2"
                ),
                params![id.to_string(), organization_id],
                action_from_row,
            )
            .optional()?
            .ok_or_else(|| AppError::not_found("message action"))
    }

    /// Every action asked of one of the organization's messages, newest first.
    pub fn list_message_actions(
        &self,
        organization_id: &str,
        message_id: Uuid,
    ) -> Result<Vec<MessageAction>, AppError> {
        self.get_message(organization_id, message_id)?;
        let connection = self.lock()?;
        let mut statement = connection.prepare(&format!(
            "{SELECT_ACTION} WHERE message_actions.message_id=$1 AND mailboxes.organization_id=$2
             ORDER BY message_actions.requested_at DESC"
        ))?;
        let rows = statement.query_map(
            params![message_id.to_string(), organization_id],
            action_from_row,
        )?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}

fn action_from_row(row: &Row<'_>) -> sql::Result<MessageAction> {
    Ok(MessageAction {
        id: column(row, "id")?,
        message_id: column(row, "message_id")?,
        action: column(row, "action")?,
        folder: row.get("folder")?,
        status: column(row, "status")?,
        error_code: row.get("error_code")?,
        error_message: row.get("error_message")?,
        requested_at: row.get("requested_at")?,
        completed_at: row.get("completed_at")?,
    })
}

/// A text column read as the value it names, or the refusal naming the column.
fn column<T>(row: &Row<'_>, name: &str) -> sql::Result<T>
where
    T: FromStr,
    T::Err: std::fmt::Display,
{
    let value = row.get::<_, String>(name)?;
    value.parse().map_err(|error| {
        sql::Error::Conversion(format!(
            "column {name} holds {value:?}, which Skrzynka cannot read: {error}"
        ))
    })
}
