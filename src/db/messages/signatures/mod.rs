//! The text a mailbox signs its replies and originated mail with, and its
//! automatic reply (`auto_replies`).

mod auto_replies;

use super::super::sql::{params, OptionalExtension};
use super::super::Database;
use crate::error::AppError;
use chrono::Utc;
use uuid::Uuid;

impl Database {
    /// The mailbox's signature, absent when it signs nothing.
    pub fn signature(
        &self,
        organization_id: &str,
        mailbox_id: Uuid,
    ) -> Result<Option<String>, AppError> {
        self.get_mailbox(organization_id, mailbox_id)?;
        self.lock()?
            .query_row(
                "SELECT signature FROM mailbox_signatures WHERE mailbox_id=$1",
                params![mailbox_id.to_string()],
                |row| row.get::<_, String>("signature"),
            )
            .optional()
            .map_err(Into::into)
    }

    /// Store `signature` for the mailbox, or remove it when absent, and
    /// answer what is stored. Sends already claimed keep the signature they
    /// were claimed with.
    pub fn set_signature(
        &self,
        organization_id: &str,
        mailbox_id: Uuid,
        signature: Option<&str>,
    ) -> Result<Option<String>, AppError> {
        self.get_mailbox(organization_id, mailbox_id)?;
        let id = mailbox_id.to_string();
        match signature {
            Some(signature) => {
                self.lock()?.execute(
                    "INSERT INTO mailbox_signatures (mailbox_id, signature, updated_at)
                     VALUES ($1, $2, $3)
                     ON CONFLICT (mailbox_id) DO UPDATE
                     SET signature=EXCLUDED.signature, updated_at=EXCLUDED.updated_at",
                    params![id, signature, Utc::now().to_rfc3339()],
                )?;
            }
            None => {
                self.lock()?.execute(
                    "DELETE FROM mailbox_signatures WHERE mailbox_id=$1",
                    params![id],
                )?;
            }
        }
        self.signature(organization_id, mailbox_id)
    }
}
