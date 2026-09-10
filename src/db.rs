use crate::{
    error::AppError,
    models::{
        DeliveryStatus, Mailbox, Message, NewMessage, OutboundMessage, ReplyAttempt, SmtpSecurity,
    },
};
use axum::http::StatusCode;
use chrono::Utc;
use rusqlite::{params, Connection, OptionalExtension, Row};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    str::FromStr,
    sync::{Arc, Mutex, MutexGuard},
};
use uuid::Uuid;

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

    /// Commit a newly adopted mailbox (when `create_mailbox` is true), every
    /// validated message, and the source cursor in one SQLite transaction.
    /// Existing UIDs are compared before mutation: identical rows are
    /// unchanged, while a different payload for the same provider UID refuses
    /// the entire page as a conflict.
    pub fn commit_mailbox_import(
        &self,
        mailbox: &Mailbox,
        create_mailbox: bool,
        messages: &[NewMessage],
        last_uid: u32,
    ) -> Result<(Mailbox, usize, usize), AppError> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        let mut unique_messages: HashMap<u32, &NewMessage> = HashMap::new();
        let mut unchanged = 0usize;
        for message in messages {
            match unique_messages.get(&message.external_uid) {
                Some(existing) if *existing == message => {
                    unchanged += 1;
                }
                Some(_) => {
                    return Err(AppError::conflict(
                        "MAILBOX_IMPORT_CONFLICT",
                        format!(
                            "provider UID {} occurs with different message data; no import data was changed",
                            message.external_uid
                        ),
                    ));
                }
                None => {
                    unique_messages.insert(message.external_uid, message);
                }
            }
        }

        let mut existing_uids = HashSet::new();
        for message in unique_messages.values().copied() {
            let existing = transaction
                .query_row(
                    "SELECT external_uid, provider_message_id, in_reply_to,
                            references_header, sender, reply_to, recipients,
                            subject, sent_at, body_text, snippet
                     FROM messages WHERE mailbox_id=?1 AND external_uid=?2",
                    params![mailbox.id.to_string(), i64::from(message.external_uid)],
                    |row| {
                        Ok(NewMessage {
                            external_uid: checked_u32(row.get::<_, i64>(0)?, 0)?,
                            message_id: row.get(1)?,
                            in_reply_to: row.get(2)?,
                            references: row.get(3)?,
                            sender: row.get(4)?,
                            reply_to: row.get(5)?,
                            recipients: row.get(6)?,
                            subject: row.get(7)?,
                            sent_at: row.get(8)?,
                            body_text: row.get(9)?,
                            snippet: row.get(10)?,
                        })
                    },
                )
                .optional()?;
            if let Some(existing) = existing {
                if &existing != message {
                    return Err(AppError::conflict(
                        "MAILBOX_IMPORT_CONFLICT",
                        format!(
                            "provider UID {} conflicts with retained mailbox data; no import data was changed",
                            message.external_uid
                        ),
                    ));
                }
                existing_uids.insert(message.external_uid);
                unchanged += 1;
            }
        }

        if create_mailbox {
            let inserted = transaction.execute(
                "INSERT INTO mailboxes (
                    id, organization_id, skarbiec_item_id, smtp_skarbiec_item_id,
                    display_name, email, imap_host, imap_port,
                    smtp_host, smtp_port, smtp_security, poll_interval_seconds,
                    enabled, last_uid, created_at, updated_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 1, 0, ?13, ?13)",
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
                    mailbox.created_at,
                ],
            );
            match inserted {
                Ok(_) => {}
                Err(error) if is_unique_constraint(&error) => {
                    return Err(AppError::conflict(
                        "MAILBOX_ALREADY_EXISTS",
                        "a mailbox already uses this Skarbiec item; no import data was changed",
                    ));
                }
                Err(error) => return Err(error.into()),
            }
        }

        let received_at = Utc::now().to_rfc3339();
        let mut imported = 0usize;
        for message in unique_messages.values().copied() {
            if existing_uids.contains(&message.external_uid) {
                continue;
            }
            transaction.execute(
                "INSERT INTO messages (
                    id, mailbox_id, external_uid, provider_message_id, in_reply_to,
                    references_header, sender, reply_to, recipients, subject,
                    sent_at, received_at, body_text, snippet
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                params![
                    Uuid::new_v4().to_string(),
                    mailbox.id.to_string(),
                    i64::from(message.external_uid),
                    message.message_id,
                    message.in_reply_to,
                    message.references,
                    message.sender,
                    message.reply_to,
                    message.recipients,
                    message.subject,
                    message.sent_at,
                    received_at,
                    message.body_text,
                    message.snippet,
                ],
            )?;
            imported += 1;
        }
        let completed_at = Utc::now().to_rfc3339();
        transaction.execute(
            "UPDATE mailboxes SET last_uid=?2, last_sync_at=?3,
                    last_error_code=NULL, last_error_message=NULL, updated_at=?3
             WHERE id=?1",
            params![mailbox.id.to_string(), i64::from(last_uid), completed_at],
        )?;
        transaction.commit()?;
        drop(connection);
        Ok((
            self.get_mailbox_internal(mailbox.id)?,
            imported,
            unchanged,
        ))
    }

    pub fn list_messages(
        &self,
        organization_id: &str,
        mailbox_id: Option<Uuid>,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<Message>, AppError> {
        let connection = self.lock()?;
        let sql = if mailbox_id.is_some() {
            "SELECT messages.id, messages.mailbox_id, messages.external_uid,
                    messages.provider_message_id, messages.in_reply_to,
                    messages.references_header, messages.sender, messages.reply_to,
                    messages.recipients, messages.subject, messages.sent_at,
                    messages.received_at, messages.body_text, messages.snippet
             FROM messages JOIN mailboxes ON mailboxes.id=messages.mailbox_id
             WHERE messages.mailbox_id=?1 AND mailboxes.organization_id=?2
             ORDER BY messages.received_at DESC LIMIT ?3 OFFSET ?4"
        } else {
            "SELECT messages.id, messages.mailbox_id, messages.external_uid,
                    messages.provider_message_id, messages.in_reply_to,
                    messages.references_header, messages.sender, messages.reply_to,
                    messages.recipients, messages.subject, messages.sent_at,
                    messages.received_at, messages.body_text, messages.snippet
             FROM messages JOIN mailboxes ON mailboxes.id=messages.mailbox_id
             WHERE mailboxes.organization_id=?1
             ORDER BY messages.received_at DESC LIMIT ?2 OFFSET ?3"
        };
        let mut statement = connection.prepare(sql)?;
        let rows = if let Some(mailbox_id) = mailbox_id {
            statement.query_map(
                params![
                    mailbox_id.to_string(),
                    organization_id,
                    i64::from(limit),
                    i64::from(offset)
                ],
                message_from_row,
            )?
        } else {
            statement.query_map(
                params![organization_id, i64::from(limit), i64::from(offset)],
                message_from_row,
            )?
        };
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    pub fn get_message(&self, organization_id: &str, id: Uuid) -> Result<Message, AppError> {
        self.lock()?
            .query_row(
                "SELECT messages.id, messages.mailbox_id, messages.external_uid,
                        messages.provider_message_id, messages.in_reply_to,
                        messages.references_header, messages.sender, messages.reply_to,
                        messages.recipients, messages.subject, messages.sent_at,
                        messages.received_at, messages.body_text, messages.snippet
                 FROM messages JOIN mailboxes ON mailboxes.id=messages.mailbox_id
                 WHERE messages.id=?1 AND mailboxes.organization_id=?2",
                params![id.to_string(), organization_id],
                message_from_row,
            )
            .optional()?
            .ok_or_else(|| AppError::not_found("message"))
    }

    pub fn begin_reply(
        &self,
        organization_id: &str,
        message_id: Uuid,
        idempotency_key: &str,
        body: &str,
    ) -> Result<(ReplyAttempt, bool), AppError> {
        self.get_message(organization_id, message_id)?;
        if let Some(existing) = self.get_reply_by_key(organization_id, idempotency_key)? {
            if existing.message_id != message_id || existing.body != body {
                return Err(AppError::conflict(
                    "IDEMPOTENCY_KEY_REUSED",
                    "idempotency key already belongs to a different reply request",
                ));
            }
            return Ok((existing, false));
        }
        let id = Uuid::new_v4();
        let now = Utc::now().to_rfc3339();
        let result = self.lock()?.execute(
            "INSERT INTO reply_attempts (
                id, message_id, idempotency_key, body, status, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, 'pending', ?5, ?5)",
            params![
                id.to_string(),
                message_id.to_string(),
                idempotency_key,
                body,
                now,
            ],
        );
        match result {
            Ok(_) => Ok((self.get_reply(id)?, true)),
            Err(error) if is_unique_constraint(&error) => {
                let existing = self
                    .get_reply_by_key(organization_id, idempotency_key)?
                    .ok_or_else(|| {
                        AppError::conflict("IDEMPOTENCY_CONFLICT", "reply request already exists")
                    })?;
                Ok((existing, false))
            }
            Err(error) => Err(error.into()),
        }
    }

    pub fn update_reply(
        &self,
        id: Uuid,
        status: DeliveryStatus,
        provider_message_id: Option<&str>,
        error_code: Option<&str>,
        error_message: Option<&str>,
    ) -> Result<ReplyAttempt, AppError> {
        let now = Utc::now().to_rfc3339();
        let sent_at = (status == DeliveryStatus::Sent).then_some(now.as_str());
        self.lock()?.execute(
            "UPDATE reply_attempts SET status=?2, provider_message_id=?3,
                    error_code=?4, error_message=?5, updated_at=?6,
                    sent_at=COALESCE(?7, sent_at) WHERE id=?1",
            params![
                id.to_string(),
                status.as_str(),
                provider_message_id,
                error_code,
                error_message,
                now,
                sent_at,
            ],
        )?;
        self.get_reply(id)
    }

    pub fn get_reply(&self, id: Uuid) -> Result<ReplyAttempt, AppError> {
        self.lock()?
            .query_row(
                "SELECT id, message_id, idempotency_key, body, status,
                        provider_message_id, error_code, error_message,
                        created_at, updated_at, sent_at
                 FROM reply_attempts WHERE id=?1",
                [id.to_string()],
                reply_from_row,
            )
            .optional()?
            .ok_or_else(|| AppError::not_found("reply attempt"))
    }

    pub fn list_replies(
        &self,
        organization_id: &str,
        message_id: Uuid,
    ) -> Result<Vec<ReplyAttempt>, AppError> {
        self.get_message(organization_id, message_id)?;
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT reply_attempts.id, reply_attempts.message_id,
                    reply_attempts.idempotency_key, reply_attempts.body,
                    reply_attempts.status, reply_attempts.provider_message_id,
                    reply_attempts.error_code, reply_attempts.error_message,
                    reply_attempts.created_at, reply_attempts.updated_at,
                    reply_attempts.sent_at
             FROM reply_attempts
             JOIN messages ON messages.id=reply_attempts.message_id
             JOIN mailboxes ON mailboxes.id=messages.mailbox_id
             WHERE reply_attempts.message_id=?1 AND mailboxes.organization_id=?2
             ORDER BY reply_attempts.created_at DESC",
        )?;
        let rows = statement.query_map(
            params![message_id.to_string(), organization_id],
            reply_from_row,
        )?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    fn get_reply_by_key(
        &self,
        organization_id: &str,
        key: &str,
    ) -> Result<Option<ReplyAttempt>, AppError> {
        self.lock()?
            .query_row(
                "SELECT reply_attempts.id, reply_attempts.message_id,
                        reply_attempts.idempotency_key, reply_attempts.body,
                        reply_attempts.status, reply_attempts.provider_message_id,
                        reply_attempts.error_code, reply_attempts.error_message,
                        reply_attempts.created_at, reply_attempts.updated_at,
                        reply_attempts.sent_at
                 FROM reply_attempts
                 JOIN messages ON messages.id=reply_attempts.message_id
                 JOIN mailboxes ON mailboxes.id=messages.mailbox_id
                 WHERE reply_attempts.idempotency_key=?1 AND mailboxes.organization_id=?2",
                params![key, organization_id],
                reply_from_row,
            )
            .optional()
            .map_err(Into::into)
    }

    /// Outbound mail is claimed the same way a reply is: the idempotency key is
    /// the unique row, so a repeated request returns the first attempt instead
    /// of handing the provider a second copy.
    pub fn begin_outbound(
        &self,
        organization_id: &str,
        mailbox_id: Uuid,
        idempotency_key: &str,
        recipients: &str,
        cc: Option<&str>,
        subject: &str,
        body: &str,
    ) -> Result<(OutboundMessage, bool), AppError> {
        self.get_mailbox(organization_id, mailbox_id)?;
        if let Some(existing) = self.get_outbound_by_key(organization_id, idempotency_key)? {
            if existing.mailbox_id != mailbox_id
                || existing.recipients != recipients
                || existing.cc.as_deref() != cc
                || existing.subject != subject
                || existing.body != body
            {
                return Err(AppError::conflict(
                    "IDEMPOTENCY_KEY_REUSED",
                    "idempotency key already belongs to a different outbound message",
                ));
            }
            return Ok((existing, false));
        }
        let id = Uuid::new_v4();
        let now = Utc::now().to_rfc3339();
        let result = self.lock()?.execute(
            "INSERT INTO outbound_messages (
                id, mailbox_id, idempotency_key, recipients, cc, subject, body,
                status, created_at, updated_at
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'pending', ?8, ?8)",
            params![
                id.to_string(),
                mailbox_id.to_string(),
                idempotency_key,
                recipients,
                cc,
                subject,
                body,
                now,
            ],
        );
        match result {
            Ok(_) => Ok((self.get_outbound_internal(id)?, true)),
            Err(error) if is_unique_constraint(&error) => {
                let existing = self
                    .get_outbound_by_key(organization_id, idempotency_key)?
                    .ok_or_else(|| {
                        AppError::conflict(
                            "IDEMPOTENCY_CONFLICT",
                            "outbound message request already exists",
                        )
                    })?;
                Ok((existing, false))
            }
            Err(error) => Err(error.into()),
        }
    }

    pub fn update_outbound(
        &self,
        id: Uuid,
        status: DeliveryStatus,
        provider_message_id: Option<&str>,
        error_code: Option<&str>,
        error_message: Option<&str>,
    ) -> Result<OutboundMessage, AppError> {
        let now = Utc::now().to_rfc3339();
        let sent_at = (status == DeliveryStatus::Sent).then_some(now.as_str());
        self.lock()?.execute(
            "UPDATE outbound_messages SET status=?2, provider_message_id=?3,
                    error_code=?4, error_message=?5, updated_at=?6,
                    sent_at=COALESCE(?7, sent_at) WHERE id=?1",
            params![
                id.to_string(),
                status.as_str(),
                provider_message_id,
                error_code,
                error_message,
                now,
                sent_at,
            ],
        )?;
        self.get_outbound_internal(id)
    }

    pub fn get_outbound(
        &self,
        organization_id: &str,
        id: Uuid,
    ) -> Result<OutboundMessage, AppError> {
        self.lock()?
            .query_row(
                "SELECT outbound_messages.id, outbound_messages.mailbox_id,
                        outbound_messages.idempotency_key, outbound_messages.recipients,
                        outbound_messages.cc, outbound_messages.subject,
                        outbound_messages.body, outbound_messages.status,
                        outbound_messages.provider_message_id,
                        outbound_messages.error_code, outbound_messages.error_message,
                        outbound_messages.created_at, outbound_messages.updated_at,
                        outbound_messages.sent_at
                 FROM outbound_messages
                 JOIN mailboxes ON mailboxes.id=outbound_messages.mailbox_id
                 WHERE outbound_messages.id=?1 AND mailboxes.organization_id=?2",
                params![id.to_string(), organization_id],
                outbound_from_row,
            )
            .optional()?
            .ok_or_else(|| AppError::not_found("outbound message"))
    }

    pub fn list_outbound(
        &self,
        organization_id: &str,
        mailbox_id: Option<Uuid>,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<OutboundMessage>, AppError> {
        let connection = self.lock()?;
        let sql = if mailbox_id.is_some() {
            "SELECT outbound_messages.id, outbound_messages.mailbox_id,
                    outbound_messages.idempotency_key, outbound_messages.recipients,
                    outbound_messages.cc, outbound_messages.subject,
                    outbound_messages.body, outbound_messages.status,
                    outbound_messages.provider_message_id,
                    outbound_messages.error_code, outbound_messages.error_message,
                    outbound_messages.created_at, outbound_messages.updated_at,
                    outbound_messages.sent_at
             FROM outbound_messages
             JOIN mailboxes ON mailboxes.id=outbound_messages.mailbox_id
             WHERE outbound_messages.mailbox_id=?1 AND mailboxes.organization_id=?2
             ORDER BY outbound_messages.created_at DESC LIMIT ?3 OFFSET ?4"
        } else {
            "SELECT outbound_messages.id, outbound_messages.mailbox_id,
                    outbound_messages.idempotency_key, outbound_messages.recipients,
                    outbound_messages.cc, outbound_messages.subject,
                    outbound_messages.body, outbound_messages.status,
                    outbound_messages.provider_message_id,
                    outbound_messages.error_code, outbound_messages.error_message,
                    outbound_messages.created_at, outbound_messages.updated_at,
                    outbound_messages.sent_at
             FROM outbound_messages
             JOIN mailboxes ON mailboxes.id=outbound_messages.mailbox_id
             WHERE mailboxes.organization_id=?1
             ORDER BY outbound_messages.created_at DESC LIMIT ?2 OFFSET ?3"
        };
        let mut statement = connection.prepare(sql)?;
        let rows = if let Some(mailbox_id) = mailbox_id {
            statement.query_map(
                params![
                    mailbox_id.to_string(),
                    organization_id,
                    i64::from(limit),
                    i64::from(offset)
                ],
                outbound_from_row,
            )?
        } else {
            statement.query_map(
                params![organization_id, i64::from(limit), i64::from(offset)],
                outbound_from_row,
            )?
        };
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    fn get_outbound_internal(&self, id: Uuid) -> Result<OutboundMessage, AppError> {
        self.lock()?
            .query_row(
                "SELECT id, mailbox_id, idempotency_key, recipients, cc, subject,
                        body, status, provider_message_id, error_code,
                        error_message, created_at, updated_at, sent_at
                 FROM outbound_messages WHERE id=?1",
                [id.to_string()],
                outbound_from_row,
            )
            .optional()?
            .ok_or_else(|| AppError::not_found("outbound message"))
    }

    fn get_outbound_by_key(
        &self,
        organization_id: &str,
        key: &str,
    ) -> Result<Option<OutboundMessage>, AppError> {
        self.lock()?
            .query_row(
                "SELECT outbound_messages.id, outbound_messages.mailbox_id,
                        outbound_messages.idempotency_key, outbound_messages.recipients,
                        outbound_messages.cc, outbound_messages.subject,
                        outbound_messages.body, outbound_messages.status,
                        outbound_messages.provider_message_id,
                        outbound_messages.error_code, outbound_messages.error_message,
                        outbound_messages.created_at, outbound_messages.updated_at,
                        outbound_messages.sent_at
                 FROM outbound_messages
                 JOIN mailboxes ON mailboxes.id=outbound_messages.mailbox_id
                 WHERE outbound_messages.idempotency_key=?1
                   AND mailboxes.organization_id=?2",
                params![key, organization_id],
                outbound_from_row,
            )
            .optional()
            .map_err(Into::into)
    }

    /// A process that dies mid-send leaves `sending` behind. That row lost its
    /// terminal SMTP evidence, so it becomes `uncertain` for a human to settle
    /// against the provider's Sent mail — never an automatic resend.
    fn recover_interrupted_sends(&self) -> Result<(), AppError> {
        let now = Utc::now().to_rfc3339();
        let connection = self.lock()?;
        connection.execute(
            "UPDATE reply_attempts SET status='uncertain',
                    error_code='REPLY_UNCERTAIN',
                    error_message='send was interrupted before terminal SMTP evidence was recorded',
                    updated_at=?1 WHERE status='sending'",
            [&now],
        )?;
        connection.execute(
            "UPDATE outbound_messages SET status='uncertain',
                    error_code='OUTBOUND_UNCERTAIN',
                    error_message='send was interrupted before terminal SMTP evidence was recorded',
                    updated_at=?1 WHERE status='sending'",
            [&now],
        )?;
        Ok(())
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
        enabled: row.get::<_, i64>(12)? != 0,
        last_uid: checked_u32(row.get::<_, i64>(13)?, 13)?,
        last_sync_at: row.get(14)?,
        last_error_code: row.get(15)?,
        last_error_message: row.get(16)?,
        created_at: row.get(17)?,
        updated_at: row.get(18)?,
    })
}

fn message_from_row(row: &Row<'_>) -> rusqlite::Result<Message> {
    Ok(Message {
        id: parse_uuid(row.get::<_, String>(0)?)?,
        mailbox_id: parse_uuid(row.get::<_, String>(1)?)?,
        external_uid: checked_u32(row.get::<_, i64>(2)?, 2)?,
        message_id: row.get(3)?,
        in_reply_to: row.get(4)?,
        references: row.get(5)?,
        sender: row.get(6)?,
        reply_to: row.get(7)?,
        recipients: row.get(8)?,
        subject: row.get(9)?,
        sent_at: row.get(10)?,
        received_at: row.get(11)?,
        body_text: row.get(12)?,
        snippet: row.get(13)?,
    })
}

fn reply_from_row(row: &Row<'_>) -> rusqlite::Result<ReplyAttempt> {
    Ok(ReplyAttempt {
        id: parse_uuid(row.get::<_, String>(0)?)?,
        message_id: parse_uuid(row.get::<_, String>(1)?)?,
        idempotency_key: row.get(2)?,
        body: row.get(3)?,
        status: parse_enum(row.get::<_, String>(4)?, 4)?,
        provider_message_id: row.get(5)?,
        error_code: row.get(6)?,
        error_message: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
        sent_at: row.get(10)?,
    })
}

fn outbound_from_row(row: &Row<'_>) -> rusqlite::Result<OutboundMessage> {
    Ok(OutboundMessage {
        id: parse_uuid(row.get::<_, String>(0)?)?,
        mailbox_id: parse_uuid(row.get::<_, String>(1)?)?,
        idempotency_key: row.get(2)?,
        recipients: row.get(3)?,
        cc: row.get(4)?,
        subject: row.get(5)?,
        body: row.get(6)?,
        status: parse_enum(row.get::<_, String>(7)?, 7)?,
        provider_message_id: row.get(8)?,
        error_code: row.get(9)?,
        error_message: row.get(10)?,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
        sent_at: row.get(13)?,
    })
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
