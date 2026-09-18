//! Mailbox rows: creation, listing, update, deletion and sync bookkeeping.

use super::{checked_u16, checked_u32, checked_u64, is_unique_constraint, parse_enum, parse_uuid, Database, MailboxConfig};
use crate::{error::AppError, models::Mailbox};
use chrono::Utc;
use rusqlite::{params, OptionalExtension, Row};
use uuid::Uuid;

impl Database {
    pub fn create_mailbox(&self, config: &MailboxConfig) -> Result<Mailbox, AppError> {
        let id = Uuid::new_v4();
        let now = Utc::now().to_rfc3339();
        let result = self.lock()?.execute(
            "INSERT INTO mailboxes (
                id, organization_id, skarbiec_item_id, smtp_skarbiec_item_id,
                display_name, email, imap_host, imap_port,
                smtp_host, smtp_port, smtp_security, poll_interval_seconds,
                enabled, last_uid, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 1, 0, ?13, ?13)",
            params![
                id.to_string(),
                config.organization_id,
                config.skarbiec_item_id,
                config.smtp_skarbiec_item_id,
                config.display_name,
                config.email,
                config.imap_host,
                i64::from(config.imap_port),
                config.smtp_host,
                i64::from(config.smtp_port),
                config.smtp_security.as_str(),
                config.poll_interval_seconds as i64,
                now,
            ],
        );
        match result {
            Ok(_) => self.get_mailbox(&config.organization_id, id),
            Err(error) if is_unique_constraint(&error) => Err(AppError::conflict(
                "MAILBOX_ALREADY_EXISTS",
                "a mailbox already uses this Skarbiec item",
            )),
            Err(error) => Err(error.into()),
        }
    }

    pub fn list_mailboxes(&self, organization_id: &str) -> Result<Vec<Mailbox>, AppError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT id, organization_id, skarbiec_item_id, smtp_skarbiec_item_id,
                    display_name, email, imap_host, imap_port, smtp_host, smtp_port,
                    smtp_security, poll_interval_seconds, enabled, last_uid, last_sync_at,
                    last_error_code, last_error_message, created_at, updated_at
             FROM mailboxes WHERE organization_id=?1
             ORDER BY display_name COLLATE NOCASE, email",
        )?;
        let rows = statement.query_map([organization_id], mailbox_from_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn list_all_mailboxes(&self) -> Result<Vec<Mailbox>, AppError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT id, organization_id, skarbiec_item_id, smtp_skarbiec_item_id,
                    display_name, email, imap_host, imap_port, smtp_host, smtp_port,
                    smtp_security, poll_interval_seconds, enabled, last_uid, last_sync_at,
                    last_error_code, last_error_message, created_at, updated_at
             FROM mailboxes ORDER BY display_name COLLATE NOCASE, email",
        )?;
        let rows = statement.query_map([], mailbox_from_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn get_mailbox(&self, organization_id: &str, id: Uuid) -> Result<Mailbox, AppError> {
        self.lock()?
            .query_row(
                "SELECT id, organization_id, skarbiec_item_id, smtp_skarbiec_item_id,
                        display_name, email, imap_host, imap_port, smtp_host, smtp_port,
                        smtp_security, poll_interval_seconds, enabled, last_uid, last_sync_at,
                        last_error_code, last_error_message, created_at, updated_at
                 FROM mailboxes WHERE id = ?1 AND organization_id = ?2",
                params![id.to_string(), organization_id],
                mailbox_from_row,
            )
            .optional()?
            .ok_or_else(|| AppError::not_found("mailbox"))
    }

    pub fn get_mailbox_internal(&self, id: Uuid) -> Result<Mailbox, AppError> {
        self.lock()?
            .query_row(
                "SELECT id, organization_id, skarbiec_item_id, smtp_skarbiec_item_id,
                        display_name, email, imap_host, imap_port, smtp_host, smtp_port,
                        smtp_security, poll_interval_seconds, enabled, last_uid, last_sync_at,
                        last_error_code, last_error_message, created_at, updated_at
                 FROM mailboxes WHERE id = ?1",
                [id.to_string()],
                mailbox_from_row,
            )
            .optional()?
            .ok_or_else(|| AppError::not_found("mailbox"))
    }

    pub fn update_mailbox(&self, mailbox: &Mailbox) -> Result<Mailbox, AppError> {
        let now = Utc::now().to_rfc3339();
        let changed = self.lock()?.execute(
            "UPDATE mailboxes SET skarbiec_item_id=?3, smtp_skarbiec_item_id=?4,
                    display_name=?5, email=?6, imap_host=?7, imap_port=?8,
                    smtp_host=?9, smtp_port=?10, smtp_security=?11,
                    poll_interval_seconds=?12, enabled=?13, updated_at=?14
             WHERE id=?1 AND organization_id=?2",
            params![
                mailbox.id.to_string(),
                mailbox.organization_id,
                mailbox.skarbiec_item_id,
                mailbox.smtp_skarbiec_item_id,
                mailbox.display_name,
                mailbox.email,
                mailbox.imap_host,
                i64::from(mailbox.imap_port),
                mailbox.smtp_host,
                i64::from(mailbox.smtp_port),
                mailbox.smtp_security.as_str(),
                mailbox.poll_interval_seconds as i64,
                mailbox.enabled as i64,
                now,
            ],
        )?;
        if changed == 0 {
            return Err(AppError::not_found("mailbox"));
        }
        self.get_mailbox(&mailbox.organization_id, mailbox.id)
    }

    pub fn delete_mailbox(&self, organization_id: &str, id: Uuid) -> Result<(), AppError> {
        let changed = self.lock()?.execute(
            "DELETE FROM mailboxes WHERE id=?1 AND organization_id=?2",
            params![id.to_string(), organization_id],
        )?;
        if changed == 0 {
            return Err(AppError::not_found("mailbox"));
        }
        Ok(())
    }

    pub fn record_sync_failure(&self, id: Uuid, code: &str, message: &str) -> Result<(), AppError> {
        let now = Utc::now().to_rfc3339();
        self.lock()?.execute(
            "UPDATE mailboxes SET last_error_code=?2, last_error_message=?3,
                    updated_at=?4 WHERE id=?1",
            params![id.to_string(), code, message, now],
        )?;
        Ok(())
    }
}

fn mailbox_from_row(row: &Row<'_>) -> rusqlite::Result<Mailbox> {
    Ok(Mailbox {
        id: parse_uuid(row.get::<_, String>(0)?)?,
        organization_id: row.get(1)?,
        skarbiec_item_id: row.get(2)?,
        smtp_skarbiec_item_id: row.get(3)?,
        display_name: row.get(4)?,
        email: row.get(5)?,
        imap_host: row.get(6)?,
        imap_port: checked_u16(row.get::<_, i64>(7)?, 7)?,
        smtp_host: row.get(8)?,
        smtp_port: checked_u16(row.get::<_, i64>(9)?, 9)?,
        smtp_security: parse_enum(row.get::<_, String>(10)?, 10)?,
        poll_interval_seconds: checked_u64(row.get::<_, i64>(11)?, 11)?,
        enabled: row.get::<_, bool>(12)?,
        last_uid: checked_u32(row.get::<_, i64>(13)?, 13)?,
        last_sync_at: row.get(14)?,
        last_error_code: row.get(15)?,
        last_error_message: row.get(16)?,
        created_at: row.get(17)?,
        updated_at: row.get(18)?,
    })
}
