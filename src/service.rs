use crate::{
    db::Database,
    error::AppError,
    mail,
    models::{
        CreateMailboxRequest, CreateReplyRequest, Mailbox, MailboxSyncResult, Message,
        ReplyAttempt, ReplyStatus, SkarbiecItemMetadata, StatusResponse, SyncAllSummary,
        SyncSummary, UpdateMailboxRequest,
    },
    skarbiec::SkarbiecResolver,
};
use chrono::Utc;
use lettre::Address;
use std::{str::FromStr, sync::Arc, time::Duration};
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Clone)]
pub struct AppState {
    pub database: Database,
    resolver: SkarbiecResolver,
    pub poll_interval_seconds: u64,
    operation_lock: Arc<Mutex<()>>,
}

impl AppState {
    pub fn new(
        database: Database,
        resolver: SkarbiecResolver,
        poll_interval_seconds: u64,
    ) -> Result<Self, AppError> {
        if !(15..=86_400).contains(&poll_interval_seconds) {
            return Err(AppError::invalid(
                "POLL_INTERVAL_INVALID",
                "poll interval must be between 15 and 86400 seconds",
            ));
        }
        Ok(Self {
            database,
            resolver,
            poll_interval_seconds,
            operation_lock: Arc::new(Mutex::new(())),
        })
    }

    pub async fn status(&self) -> Result<StatusResponse, AppError> {
        let (mailbox_count, enabled_mailbox_count, message_count) = self.database.counts()?;
        let skarbiec_available = self.resolver.is_available().await;
        Ok(StatusResponse {
            product: "skrzynka",
            version: env!("CARGO_PKG_VERSION"),
            database_path: self.database.path().display().to_string(),
            schema_version: crate::db::SCHEMA_VERSION,
            mailbox_count,
            enabled_mailbox_count,
            message_count,
            poll_interval_seconds: self.poll_interval_seconds,
            skarbiec_available,
        })
    }

    pub async fn list_skarbiec_items(&self) -> Result<Vec<SkarbiecItemMetadata>, AppError> {
        self.resolver.list_items().await
    }

    pub async fn create_mailbox(
        &self,
        mut request: CreateMailboxRequest,
    ) -> Result<Mailbox, AppError> {
        if request.poll_interval_seconds.is_none() {
            request.poll_interval_seconds = Some(self.poll_interval_seconds);
        }
        let config = self.resolver.resolve_mailbox_config(&request).await?;
        self.database.create_mailbox(&config)
    }

    pub fn list_mailboxes(&self) -> Result<Vec<Mailbox>, AppError> {
        self.database.list_mailboxes()
    }

    pub fn get_mailbox(&self, id: Uuid) -> Result<Mailbox, AppError> {
        self.database.get_mailbox(id)
    }

    pub fn update_mailbox(
        &self,
        id: Uuid,
        request: UpdateMailboxRequest,
    ) -> Result<Mailbox, AppError> {
        let mut mailbox = self.database.get_mailbox(id)?;
        if let Some(value) = request.display_name {
            mailbox.display_name = value.trim().to_string();
        }
        if let Some(value) = request.email {
            mailbox.email = value.trim().to_string();
        }
        if let Some(value) = request.imap_host {
            mailbox.imap_host = value.trim().to_string();
        }
        if let Some(value) = request.imap_port {
            mailbox.imap_port = value;
        }
        if let Some(value) = request.smtp_host {
            mailbox.smtp_host = value.trim().to_string();
        }
        if let Some(value) = request.smtp_port {
            mailbox.smtp_port = value;
        }
        if let Some(value) = request.smtp_security {
            mailbox.smtp_security = value;
        }
        if let Some(value) = request.poll_interval_seconds {
            mailbox.poll_interval_seconds = value;
        }
        if let Some(value) = request.enabled {
            mailbox.enabled = value;
        }
        validate_mailbox(&mailbox)?;
        self.database.update_mailbox(&mailbox)
    }

    pub fn delete_mailbox(&self, id: Uuid) -> Result<(), AppError> {
        self.database.delete_mailbox(id)
    }

    pub async fn sync_mailbox(&self, id: Uuid) -> Result<SyncSummary, AppError> {
        let _guard = self.operation_lock.lock().await;
        let mailbox = self.database.get_mailbox(id)?;
        let credentials = match self
            .resolver
            .resolve_credentials(&mailbox.skarbiec_item_id)
            .await
        {
            Ok(credentials) => credentials,
            Err(error) => {
                let _ = self
                    .database
                    .record_sync_failure(id, error.code, &error.message);
                return Err(error);
            }
        };
        let database = self.database.clone();
        let result = tokio::task::spawn_blocking(move || {
            let fetched = mail::fetch_messages(&mailbox, &credentials)?;
            let mut received = 0usize;
            for message in &fetched.messages {
                if database.insert_message(mailbox.id, message)? {
                    received += 1;
                }
            }
            database.record_sync_success(mailbox.id, fetched.last_uid)?;
            Ok::<_, AppError>(SyncSummary {
                mailbox_id: mailbox.id,
                received,
                skipped: fetched.skipped,
                last_uid: fetched.last_uid,
                completed_at: Utc::now().to_rfc3339(),
            })
        })
        .await
        .map_err(|_| AppError::internal("mailbox synchronization task stopped unexpectedly"))?;
        if let Err(error) = &result {
            let _ = self
                .database
                .record_sync_failure(id, error.code, &error.message);
        }
        result
    }

    pub async fn sync_all(&self) -> Result<SyncAllSummary, AppError> {
        let mailboxes = self
            .database
            .list_mailboxes()?
            .into_iter()
            .filter(|mailbox| mailbox.enabled)
            .collect::<Vec<_>>();
        self.sync_mailboxes(mailboxes).await
    }

    async fn sync_due(&self) -> Result<SyncAllSummary, AppError> {
        let now = Utc::now();
        let mailboxes = self
            .database
            .list_mailboxes()?
            .into_iter()
            .filter(|mailbox| {
                if !mailbox.enabled {
                    return false;
                }
                mailbox
                    .last_sync_at
                    .as_deref()
                    .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
                    .map(|last| {
                        now.signed_duration_since(last.with_timezone(&Utc))
                            .num_seconds()
                            >= mailbox.poll_interval_seconds as i64
                    })
                    .unwrap_or(true)
            })
            .collect::<Vec<_>>();
        self.sync_mailboxes(mailboxes).await
    }

    async fn sync_mailboxes(&self, mailboxes: Vec<Mailbox>) -> Result<SyncAllSummary, AppError> {
        let mut results = Vec::with_capacity(mailboxes.len());
        for mailbox in mailboxes {
            match self.sync_mailbox(mailbox.id).await {
                Ok(summary) => results.push(MailboxSyncResult {
                    mailbox_id: mailbox.id,
                    ok: true,
                    summary: Some(summary),
                    error_code: None,
                    error_message: None,
                }),
                Err(error) => results.push(MailboxSyncResult {
                    mailbox_id: mailbox.id,
                    ok: false,
                    summary: None,
                    error_code: Some(error.code.to_string()),
                    error_message: Some(error.message),
                }),
            }
        }
        Ok(SyncAllSummary {
            completed_at: Utc::now().to_rfc3339(),
            mailboxes: results,
        })
    }

    pub fn list_messages(
        &self,
        mailbox_id: Option<Uuid>,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<Message>, AppError> {
        self.database
            .list_messages(mailbox_id, limit.clamp(1, 500), offset)
    }

    pub fn get_message(&self, id: Uuid) -> Result<Message, AppError> {
        self.database.get_message(id)
    }

    pub fn list_replies(&self, message_id: Uuid) -> Result<Vec<ReplyAttempt>, AppError> {
        self.database.list_replies(message_id)
    }

    pub async fn reply(
        &self,
        message_id: Uuid,
        request: CreateReplyRequest,
    ) -> Result<ReplyAttempt, AppError> {
        validate_reply_request(&request)?;
        let (attempt, created) = self.database.begin_reply(
            message_id,
            request.idempotency_key.trim(),
            request.body.trim_end(),
        )?;
        if !created {
            return Ok(attempt);
        }
        let attempt =
            self.database
                .update_reply(attempt.id, ReplyStatus::Sending, None, None, None)?;
        let message = self.database.get_message(message_id)?;
        let mailbox = self.database.get_mailbox(message.mailbox_id)?;
        let credentials = match self
            .resolver
            .resolve_credentials(&mailbox.skarbiec_item_id)
            .await
        {
            Ok(credentials) => credentials,
            Err(error) => {
                let _ = self.database.update_reply(
                    attempt.id,
                    ReplyStatus::Failed,
                    None,
                    Some(error.code),
                    Some(&error.message),
                );
                return Err(error);
            }
        };
        let body = request.body.trim_end().to_string();
        let result = tokio::task::spawn_blocking(move || {
            mail::send_reply(&mailbox, &credentials, &message, &body)
        })
        .await;
        match result {
            Ok(Ok(provider_message_id)) => self.database.update_reply(
                attempt.id,
                ReplyStatus::Sent,
                Some(&provider_message_id),
                None,
                None,
            ),
            Ok(Err(error)) if error.code == "SMTP_UNCERTAIN" => self.database.update_reply(
                attempt.id,
                ReplyStatus::Uncertain,
                None,
                Some("REPLY_UNCERTAIN"),
                Some(&error.message),
            ),
            Ok(Err(error)) => {
                let _ = self.database.update_reply(
                    attempt.id,
                    ReplyStatus::Failed,
                    None,
                    Some(error.code),
                    Some(&error.message),
                );
                Err(error)
            }
            Err(_) => self.database.update_reply(
                attempt.id,
                ReplyStatus::Uncertain,
                None,
                Some("REPLY_UNCERTAIN"),
                Some("send task stopped before terminal SMTP evidence was recorded"),
            ),
        }
    }

    pub fn start_polling(self) {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(15));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            interval.tick().await;
            loop {
                interval.tick().await;
                match self.sync_due().await {
                    Ok(summary) => {
                        let failed = summary.mailboxes.iter().filter(|result| !result.ok).count();
                        tracing::info!(
                            mailboxes = summary.mailboxes.len(),
                            failed,
                            "mailbox poll completed"
                        );
                    }
                    Err(error) => tracing::error!(code = error.code, "mailbox poll failed"),
                }
            }
        });
    }
}

fn validate_mailbox(mailbox: &Mailbox) -> Result<(), AppError> {
    if mailbox.display_name.is_empty() || mailbox.display_name.chars().count() > 200 {
        return Err(AppError::invalid(
            "MAILBOX_PROFILE_INVALID",
            "display_name must contain between 1 and 200 characters",
        ));
    }
    Address::from_str(&mailbox.email).map_err(|_| {
        AppError::invalid("MAILBOX_PROFILE_INVALID", "email is not a valid address")
    })?;
    for (name, value) in [
        ("imap_host", mailbox.imap_host.as_str()),
        ("smtp_host", mailbox.smtp_host.as_str()),
    ] {
        if value.is_empty()
            || value.len() > 253
            || value.contains("://")
            || value.chars().any(char::is_whitespace)
        {
            return Err(AppError::invalid(
                "MAILBOX_PROFILE_INVALID",
                format!("{name} must be a hostname without a URL scheme"),
            ));
        }
    }
    if mailbox.imap_port == 0 || mailbox.smtp_port == 0 {
        return Err(AppError::invalid(
            "MAILBOX_PROFILE_INVALID",
            "mail server ports must be nonzero",
        ));
    }
    if !(15..=86_400).contains(&mailbox.poll_interval_seconds) {
        return Err(AppError::invalid(
            "MAILBOX_PROFILE_INVALID",
            "poll_interval_seconds must be between 15 and 86400",
        ));
    }
    Ok(())
}

fn validate_reply_request(request: &CreateReplyRequest) -> Result<(), AppError> {
    let key = request.idempotency_key.trim();
    if key.is_empty() || key.len() > 200 || key.chars().any(char::is_whitespace) {
        return Err(AppError::invalid(
            "IDEMPOTENCY_KEY_INVALID",
            "idempotency_key must contain 1 to 200 non-whitespace characters",
        ));
    }
    if request.body.trim().is_empty() {
        return Err(AppError::invalid(
            "REPLY_BODY_INVALID",
            "reply body must not be empty",
        ));
    }
    if request.body.len() > 256 * 1024 {
        return Err(AppError::invalid(
            "REPLY_BODY_TOO_LARGE",
            "reply body exceeds the 256 KiB limit",
        ));
    }
    Ok(())
}
