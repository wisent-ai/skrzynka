//! The local SQLite state: schema, connection, and the per-entity row operations in the
//! sub-modules, which extend `Database` with mailbox, message, reply and outbound methods.

use crate::{error::AppError, models::SmtpSecurity};
use axum::http::StatusCode;
use rusqlite::Connection;
use std::{
    path::{Path, PathBuf},
    str::FromStr,
    sync::{Arc, Mutex, MutexGuard},
};
use uuid::Uuid;

mod mailboxes;
mod messages;
mod outbound;
mod replies;

pub const SCHEMA_VERSION: u32 = 4;

#[derive(Debug, Clone)]
pub struct MailboxConfig {
    pub organization_id: String,
    pub skarbiec_item_id: String,
    pub smtp_skarbiec_item_id: Option<String>,
    pub display_name: String,
    pub email: String,
    pub imap_host: String,
    pub imap_port: u16,
    pub smtp_host: String,
    pub smtp_port: u16,
    pub smtp_security: SmtpSecurity,
    pub poll_interval_seconds: u64,
}

#[derive(Clone)]
pub struct Database {
    path: PathBuf,
    connection: Arc<Mutex<Connection>>,
}

impl Database {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, AppError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                tracing::error!(error = %error, path = %parent.display(), "state directory creation failed");
                AppError::internal("local state directory could not be created")
            })?;
        }
        let connection = Connection::open(&path)?;
        connection.pragma_update(None, "journal_mode", "WAL")?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        connection.busy_timeout(std::time::Duration::from_secs(5))?;
        let version: u32 = connection.query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version > SCHEMA_VERSION {
            return Err(AppError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "DATABASE_SCHEMA_UNSUPPORTED",
                format!(
                    "database schema {version} is not supported by this build (expected at most {SCHEMA_VERSION})"
                ),
                false,
            ));
        }
        connection.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS mailboxes (
                id TEXT PRIMARY KEY,
                organization_id TEXT NOT NULL,
                skarbiec_item_id TEXT NOT NULL UNIQUE,
                smtp_skarbiec_item_id TEXT,
                display_name TEXT NOT NULL,
                email TEXT NOT NULL,
                imap_host TEXT NOT NULL,
                imap_port INTEGER NOT NULL,
                smtp_host TEXT NOT NULL,
                smtp_port INTEGER NOT NULL,
                smtp_security TEXT NOT NULL CHECK (smtp_security IN ('starttls', 'tls')),
                poll_interval_seconds INTEGER NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1,
                last_uid INTEGER NOT NULL DEFAULT 0,
                last_sync_at TEXT,
                last_error_code TEXT,
                last_error_message TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS messages (
                id TEXT PRIMARY KEY,
                mailbox_id TEXT NOT NULL REFERENCES mailboxes(id) ON DELETE CASCADE,
                external_uid INTEGER NOT NULL,
                provider_message_id TEXT,
                in_reply_to TEXT,
                references_header TEXT,
                sender TEXT NOT NULL,
                reply_to TEXT,
                recipients TEXT NOT NULL,
                subject TEXT NOT NULL,
                sent_at TEXT,
                received_at TEXT NOT NULL,
                body_text TEXT NOT NULL,
                snippet TEXT NOT NULL,
                UNIQUE(mailbox_id, external_uid)
            );
            CREATE INDEX IF NOT EXISTS messages_received_idx
                ON messages(received_at DESC);
            CREATE INDEX IF NOT EXISTS messages_mailbox_idx
                ON messages(mailbox_id, received_at DESC);
            CREATE TABLE IF NOT EXISTS reply_attempts (
                id TEXT PRIMARY KEY,
                message_id TEXT NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
                idempotency_key TEXT NOT NULL UNIQUE,
                body TEXT NOT NULL,
                status TEXT NOT NULL CHECK (status IN ('pending', 'sending', 'sent', 'failed', 'uncertain')),
                provider_message_id TEXT,
                error_code TEXT,
                error_message TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                sent_at TEXT
            );
            CREATE INDEX IF NOT EXISTS replies_message_idx
                ON reply_attempts(message_id, created_at DESC);
            CREATE TABLE IF NOT EXISTS outbound_messages (
                id TEXT PRIMARY KEY,
                mailbox_id TEXT NOT NULL REFERENCES mailboxes(id) ON DELETE CASCADE,
                idempotency_key TEXT NOT NULL UNIQUE,
                recipients TEXT NOT NULL,
                cc TEXT,
                subject TEXT NOT NULL,
                body TEXT NOT NULL,
                status TEXT NOT NULL CHECK (status IN ('pending', 'sending', 'sent', 'failed', 'uncertain')),
                provider_message_id TEXT,
                error_code TEXT,
                error_message TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                sent_at TEXT
            );
            CREATE INDEX IF NOT EXISTS outbound_mailbox_idx
                ON outbound_messages(mailbox_id, created_at DESC);
            ",
        )?;
        match version {
            0 => connection.pragma_update(None, "user_version", SCHEMA_VERSION)?,
            1 => {
                connection.execute(
                    "ALTER TABLE mailboxes ADD COLUMN organization_id TEXT NOT NULL DEFAULT 'legacy-local'",
                    [],
                )?;
                connection.execute(
                    "ALTER TABLE mailboxes ADD COLUMN smtp_skarbiec_item_id TEXT",
                    [],
                )?;
                connection.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            }
            2 | 3 => {
                connection.execute(
                    "ALTER TABLE mailboxes ADD COLUMN smtp_skarbiec_item_id TEXT",
                    [],
                )?;
                connection.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            }
            _ => {}
        }
        let database = Self {
            path,
            connection: Arc::new(Mutex::new(connection)),
        };
        database.recover_interrupted_sends()?;
        Ok(database)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn lock(&self) -> Result<MutexGuard<'_, Connection>, AppError> {
        self.connection
            .lock()
            .map_err(|_| AppError::internal("local state lock was poisoned"))
    }

    pub fn counts(&self, organization_id: &str) -> Result<(usize, usize, usize), AppError> {
        let connection = self.lock()?;
        let mailboxes: usize = connection.query_row(
            "SELECT COUNT(*) FROM mailboxes WHERE organization_id=?1",
            [organization_id],
            |row| row.get(0),
        )?;
        let enabled: usize = connection.query_row(
            "SELECT COUNT(*) FROM mailboxes WHERE organization_id=?1 AND enabled=1",
            [organization_id],
            |row| row.get(0),
        )?;
        let messages: usize = connection.query_row(
            "SELECT COUNT(*) FROM messages
             JOIN mailboxes ON mailboxes.id=messages.mailbox_id
             WHERE mailboxes.organization_id=?1",
            [organization_id],
            |row| row.get(0),
        )?;
        Ok((mailboxes, enabled, messages))
    }
}

fn parse_uuid(value: String) -> rusqlite::Result<Uuid> {
    Uuid::parse_str(&value).map_err(|error| conversion_error(0, error))
}

fn parse_enum<T>(value: String, column: usize) -> rusqlite::Result<T>
where
    T: FromStr,
    T::Err: std::error::Error + Send + Sync + 'static,
{
    value
        .parse()
        .map_err(|error| conversion_error(column, error))
}

fn checked_u16(value: i64, column: usize) -> rusqlite::Result<u16> {
    u16::try_from(value).map_err(|error| conversion_error(column, error))
}

fn checked_u32(value: i64, column: usize) -> rusqlite::Result<u32> {
    u32::try_from(value).map_err(|error| conversion_error(column, error))
}

fn checked_u64(value: i64, column: usize) -> rusqlite::Result<u64> {
    u64::try_from(value).map_err(|error| conversion_error(column, error))
}

fn conversion_error(
    column: usize,
    error: impl std::error::Error + Send + Sync + 'static,
) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(column, rusqlite::types::Type::Text, Box::new(error))
}

fn is_unique_constraint(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(code, _)
            if code.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE
    )
}
