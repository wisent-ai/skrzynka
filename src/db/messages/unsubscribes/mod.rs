//! Unsubscribe attempt rows: claimed by idempotency key, updated as the list
//! answers, and read per message or for the whole organization.

use super::super::sql::{self, params, OptionalExtension, Row};
use super::super::{is_unique_constraint, parse_uuid, Database};
use crate::{
    error::AppError,
    mail::{UnsubscribeAttempt, UnsubscribeMethod},
    models::DeliveryStatus,
};
use chrono::Utc;
use stado_database::sync::Tx;
use uuid::Uuid;

/// The columns every attempt read selects, by the names `attempt_from_row` reads.
const COLUMNS: &str = "unsubscribe_attempts.id, unsubscribe_attempts.message_id,
        unsubscribe_attempts.idempotency_key, unsubscribe_attempts.method,
        unsubscribe_attempts.target, unsubscribe_attempts.status,
        unsubscribe_attempts.provider_message_id, unsubscribe_attempts.error_code,
        unsubscribe_attempts.error_message, unsubscribe_attempts.created_at,
        unsubscribe_attempts.updated_at, unsubscribe_attempts.completed_at";

/// The organization scope every attempt read joins through.
const SCOPE: &str = "FROM unsubscribe_attempts
        JOIN messages ON messages.id=unsubscribe_attempts.message_id
        JOIN mailboxes ON mailboxes.id=messages.mailbox_id";

impl Database {
    /// An unsubscribe request is claimed by its idempotency key before the
    /// list is asked. The same key for another message, method or address is
    /// a different request and is refused.
    pub fn begin_unsubscribe(
        &self,
        organization_id: &str,
        message_id: Uuid,
        idempotency_key: &str,
        method: UnsubscribeMethod,
        target: &str,
    ) -> Result<(UnsubscribeAttempt, bool), AppError> {
        let message = self.get_message(organization_id, message_id)?;
        if let Some(existing) = self.unsubscribe_by_key(organization_id, idempotency_key)? {
            if existing.message_id != message_id
                || existing.method != method
                || existing.target != target
            {
                return Err(AppError::conflict(
                    "IDEMPOTENCY_KEY_REUSED",
                    "idempotency key already belongs to a different unsubscribe request",
                ));
            }
            return Ok((existing, false));
        }
        let id = Uuid::new_v4();
        let now = Utc::now().to_rfc3339();
        let insert = |transaction: &Tx<'_>| {
            transaction.execute(
                "INSERT INTO unsubscribe_attempts (
                    id, message_id, idempotency_key, method, target, status, created_at, updated_at
                 ) VALUES ($1, $2, $3, $4, $5, 'pending', $6, $6)",
                params![
                    id.to_string(),
                    message_id.to_string(),
                    idempotency_key,
                    method.as_str(),
                    target,
                    now,
                ],
            )
        };
        // Only an unsubscribe that sends mail is a send the mailbox's limit
        // counts; a one-click request sends none.
        let result = match method {
            UnsubscribeMethod::Mailto => {
                self.claim_send(&message.mailbox_id.to_string(), insert)?
            }
            UnsubscribeMethod::OneClick => {
                let connection = self.lock()?;
                let transaction = connection.transaction()?;
                insert(&transaction).and_then(|inserted| {
                    transaction.commit()?;
                    Ok(inserted)
                })
            }
        };
        match result {
            Ok(_) => Ok((self.get_unsubscribe(id)?, true)),
            Err(error) if is_unique_constraint(&error) => {
                let existing = self
                    .unsubscribe_by_key(organization_id, idempotency_key)?
                    .ok_or_else(|| {
                        AppError::conflict(
                            "IDEMPOTENCY_KEY_REUSED",
                            "idempotency key already belongs to an unsubscribe request of another organization",
                        )
                    })?;
                Ok((existing, false))
            }
            Err(error) => Err(error.into()),
        }
    }

    pub fn update_unsubscribe(
        &self,
        id: Uuid,
        status: DeliveryStatus,
        provider_message_id: Option<&str>,
        error_code: Option<&str>,
        error_message: Option<&str>,
    ) -> Result<UnsubscribeAttempt, AppError> {
        let now = Utc::now().to_rfc3339();
        let completed_at = (status == DeliveryStatus::Sent).then_some(now.as_str());
        self.lock()?.execute(
            "UPDATE unsubscribe_attempts SET status=$2, provider_message_id=$3,
                    error_code=$4, error_message=$5, updated_at=$6,
                    completed_at=COALESCE($7, completed_at) WHERE id=$1",
            params![
                id.to_string(),
                status.as_str(),
                provider_message_id,
                error_code,
                error_message,
                now,
                completed_at,
            ],
        )?;
        self.get_unsubscribe(id)
    }

    pub fn get_unsubscribe(&self, id: Uuid) -> Result<UnsubscribeAttempt, AppError> {
        self.lock()?
            .query_row(
                &format!("SELECT {COLUMNS} FROM unsubscribe_attempts WHERE id=$1"),
                [id.to_string()],
                attempt_from_row,
            )
            .optional()?
            .ok_or_else(|| AppError::not_found("unsubscribe attempt"))
    }

    /// Every unsubscribe attempt of the organization, newest first.
    pub fn list_unsubscribes(
        &self,
        organization_id: &str,
    ) -> Result<Vec<UnsubscribeAttempt>, AppError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(&format!(
            "SELECT {COLUMNS} {SCOPE}
             WHERE mailboxes.organization_id=$1
             ORDER BY unsubscribe_attempts.created_at DESC"
        ))?;
        let rows = statement.query_map(params![organization_id], attempt_from_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    fn unsubscribe_by_key(
        &self,
        organization_id: &str,
        key: &str,
    ) -> Result<Option<UnsubscribeAttempt>, AppError> {
        self.lock()?
            .query_row(
                &format!(
                    "SELECT {COLUMNS} {SCOPE}
                     WHERE unsubscribe_attempts.idempotency_key=$1
                       AND mailboxes.organization_id=$2"
                ),
                params![key, organization_id],
                attempt_from_row,
            )
            .optional()
            .map_err(Into::into)
    }
}

/// One attempt, read by the names of the columns every attempt query selects.
fn attempt_from_row(row: &Row<'_>) -> sql::Result<UnsubscribeAttempt> {
    Ok(UnsubscribeAttempt {
        id: parse_uuid(row.get::<_, String>("id")?)?,
        message_id: parse_uuid(row.get::<_, String>("message_id")?)?,
        idempotency_key: row.get("idempotency_key")?,
        method: UnsubscribeMethod::parse(&row.get::<_, String>("method")?).map_err(|error| {
            sql::Error::Conversion(format!("column method holds no unsubscribe method: {error}"))
        })?,
        target: row.get("target")?,
        status: row
            .get::<_, String>("status")?
            .parse::<DeliveryStatus>()
            .map_err(|error| {
                sql::Error::Conversion(format!("column status holds no delivery status: {error}"))
            })?,
        provider_message_id: row.get("provider_message_id")?,
        error_code: row.get("error_code")?,
        error_message: row.get("error_message")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
        completed_at: row.get("completed_at")?,
    })
}
