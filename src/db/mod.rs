//! Skrzynka's state in the fleet database `skrzynka`: schema, connection, and the
//! per-entity row operations in the sub-modules, which extend `Database` with mailbox,
//! message, reply and outbound methods.

use crate::{error::AppError, models::SmtpSecurity};
use axum::http::StatusCode;
use std::{str::FromStr, sync::Arc};
use uuid::Uuid;

mod mailboxes;
mod messages;
mod outbound;
mod replies;
pub mod sql;

use sql::{params, Client, OptionalExtension};

/// The schema this build writes. A database that records a newer one was written by a
/// newer Skrzynka, which this build refuses to touch.
pub const SCHEMA_VERSION: u32 = 5;
/// The database's name in Stado.
pub const DATABASE_NAME: &str = "skrzynka";
const SCHEMA: &str = include_str!("sql/schema.sql");

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
    client: Arc<Client>,
}

impl Database {
    /// Connect to the fleet database, create the tables it lacks, and settle sends a
    /// stopped process left in `sending`.
    pub fn open() -> Result<Self, AppError> {
        let database = Self {
            client: Arc::new(sql::connect()?),
        };
        {
            let session = database.lock()?;
            session.execute_batch(SCHEMA)?;
            let version = session
                .query_row(
                    "SELECT value FROM schema_meta WHERE key='schema_version'",
                    params![],
                    |row| row.get::<_, i64>(0),
                )
                .optional()?
                .unwrap_or_default();
            if version > i64::from(SCHEMA_VERSION) {
                return Err(AppError::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "DATABASE_SCHEMA_UNSUPPORTED",
                    format!(
                        "database schema {version} is not supported by this build (expected at most {SCHEMA_VERSION})"
                    ),
                    false,
                ));
            }
            session.execute(
                "INSERT INTO schema_meta (key, value) VALUES ('schema_version', $1)
                 ON CONFLICT (key) DO UPDATE SET value = excluded.value",
                [i64::from(SCHEMA_VERSION)],
            )?;
        }
        database.recover_interrupted_sends()?;
        Ok(database)
    }

    pub fn name(&self) -> &'static str {
        DATABASE_NAME
    }

    fn lock(&self) -> Result<&Client, AppError> {
        Ok(&self.client)
    }

    pub fn counts(&self, organization_id: &str) -> Result<(usize, usize, usize), AppError> {
        let session = self.lock()?;
        let count = |sql: &str| -> Result<usize, AppError> {
            let value = session.query_row(sql, [organization_id], |row| row.get::<_, i64>(0))?;
            usize::try_from(value).map_err(|_| AppError::internal("a count was negative"))
        };
        let mailboxes = count("SELECT COUNT(*) FROM mailboxes WHERE organization_id=$1")?;
        let enabled = count("SELECT COUNT(*) FROM mailboxes WHERE organization_id=$1 AND enabled")?;
        let messages = count(
            "SELECT COUNT(*) FROM messages
             JOIN mailboxes ON mailboxes.id=messages.mailbox_id
             WHERE mailboxes.organization_id=$1",
        )?;
        Ok((mailboxes, enabled, messages))
    }
}

fn parse_uuid(value: String) -> sql::Result<Uuid> {
    Uuid::parse_str(&value).map_err(|error| conversion_error(0, error))
}

fn parse_enum<T>(value: String, column: usize) -> sql::Result<T>
where
    T: FromStr,
    T::Err: std::error::Error + Send + Sync + 'static,
{
    value
        .parse()
        .map_err(|error| conversion_error(column, error))
}

fn checked_u16(value: i64, column: usize) -> sql::Result<u16> {
    u16::try_from(value).map_err(|error| conversion_error(column, error))
}

fn checked_u32(value: i64, column: usize) -> sql::Result<u32> {
    u32::try_from(value).map_err(|error| conversion_error(column, error))
}

fn checked_u64(value: i64, column: usize) -> sql::Result<u64> {
    u64::try_from(value).map_err(|error| conversion_error(column, error))
}

fn conversion_error(column: usize, error: impl std::error::Error) -> sql::Error {
    sql::Error::Conversion(format!(
        "column {column} holds a value Skrzynka cannot read: {error}"
    ))
}

fn is_unique_constraint(error: &sql::Error) -> bool {
    error.is_unique_violation()
}
