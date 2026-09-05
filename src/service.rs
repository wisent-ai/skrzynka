use crate::{
    auth::AuthVerifier,
    db::{Database, MailboxConfig},
    error::AppError,
    gmail::{
        GmailOAuthBroker, GmailOAuthCallback, GmailOAuthFlowSnapshot, GmailOAuthFlowStatus,
        GmailProfile, StartGmailOAuthRequest, StartGmailOAuthResponse,
    },
    mail,
    models::{
        CreateMailboxRequest, CreateOutboundRequest, CreateReplyRequest, DeliveryStatus,
        ImportItemCounts, Mailbox, MailboxImportResult, MailboxImportSource, MailboxImportState,
        MailboxSyncResult, Message, OutboundMessage, ReplyAttempt, SkarbiecItemMetadata,
        SmtpSecurity, StatusResponse, SyncAllSummary, SyncSummary, UpdateMailboxRequest,
    },
    skarbiec::{ResolvedCredentials, SkarbiecResolver, GOOGLE_ADMIN_DELEGATION_URL},
};
use chrono::Utc;
use lettre::Address;
use serde::Serialize;
use std::{str::FromStr, sync::Arc, time::Duration};
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Clone)]
pub struct AppState {
    pub auth_verifier: AuthVerifier,
    pub database: Database,
    resolver: SkarbiecResolver,
    gmail_oauth: GmailOAuthBroker,
    pub poll_interval_seconds: u64,
    operation_lock: Arc<Mutex<()>>,
}

#[derive(Serialize)]
pub struct GmailOAuthStatusResponse {
    pub flow_id: Uuid,
    pub status: &'static str,
    pub expires_at: String,
    pub mailbox: Option<Mailbox>,
    pub error: Option<GmailOAuthStatusError>,
}

#[derive(Serialize)]
pub struct GmailOAuthStatusError {
    pub code: &'static str,
    pub message: String,
    pub retryable: bool,
}

#[derive(Serialize)]
pub struct GmailDelegationStatus {
    pub configured: bool,
    pub service_account: Option<String>,
    pub client_id: Option<String>,
    pub scope: &'static str,
    pub admin_console_url: &'static str,
}

impl AppState {
    pub fn new(
        database: Database,
        resolver: SkarbiecResolver,
        poll_interval_seconds: u64,
        callback_base_url: &str,
    ) -> Result<Self, AppError> {
        if !(15..=86_400).contains(&poll_interval_seconds) {
            return Err(AppError::invalid(
                "POLL_INTERVAL_INVALID",
                "poll interval must be between 15 and 86400 seconds",
            ));
        }
        let gmail_oauth = GmailOAuthBroker::new(resolver.clone(), callback_base_url)?;
        let auth_verifier = AuthVerifier::from_environment()?;
        Ok(Self {
            auth_verifier,
            database,
            resolver,
            gmail_oauth,
            poll_interval_seconds,
            operation_lock: Arc::new(Mutex::new(())),
        })
    }

    pub async fn status(&self, organization_id: &str) -> Result<StatusResponse, AppError> {
        let (mailbox_count, enabled_mailbox_count, message_count) =
            self.database.counts(organization_id)?;
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

    pub async fn list_gmail_profiles(&self) -> Result<Vec<GmailProfile>, AppError> {
        self.gmail_oauth.profiles().await
    }

    /// Delegation is reportable, never failing: a missing service-account item
    /// is a state the Connect surface must render, not an error.
    pub async fn gmail_delegation_status(&self) -> GmailDelegationStatus {
        match self.resolver.google_service_account().await {
            Ok(account) => GmailDelegationStatus {
                configured: true,
                service_account: Some(account.client_email),
                client_id: Some(account.client_id),
                scope: "https://mail.google.com/",
                admin_console_url: GOOGLE_ADMIN_DELEGATION_URL,
            },
            Err(_) => GmailDelegationStatus {
                configured: false,
                service_account: None,
                client_id: None,
                scope: "https://mail.google.com/",
                admin_console_url: GOOGLE_ADMIN_DELEGATION_URL,
            },
        }
    }

    /// Connect a Workspace mailbox through domain-wide delegation: prove the
    /// grant by minting a token for the address, persist the credential bundle
    /// in Skarbiec, then create or return the mailbox.
    ///
    /// The grant itself is not Skrzynka's to perform. It exists only in the
    /// Workspace admin console, so a missing grant is reported as
    /// `GOOGLE_DELEGATION_NOT_GRANTED` with the client ID, the scope and the
    /// console URL the administrator needs — never attempted from here.
    pub async fn connect_gmail_delegated(
        &self,
        organization_id: &str,
        email: &str,
        display_name: Option<String>,
    ) -> Result<Mailbox, AppError> {
        let email = email.trim();
        Address::from_str(email).map_err(|_| {
            AppError::invalid("GMAIL_PROFILE_INVALID", "email is not a valid address")
        })?;
        let probe_key = format!("delegation-probe:{email}");
        self.resolver
            .delegated_access_token(&probe_key, email)
            .await?;
        let item_id = self.resolver.save_gmail_delegation(email).await?;
        if let Some(mailbox) = self
            .database
            .list_mailboxes(organization_id)?
            .into_iter()
            .find(|mailbox| mailbox.skarbiec_item_id == item_id)
        {
            return Ok(mailbox);
        }
        self.create_mailbox(
            organization_id,
            CreateMailboxRequest {
                skarbiec_item_id: item_id,
                display_name: display_name.or_else(|| Some(email.to_string())),
                email: Some(email.to_string()),
                imap_host: None,
                imap_port: None,
                smtp_host: None,
                smtp_port: None,
                smtp_security: Some(SmtpSecurity::Starttls),
                poll_interval_seconds: None,
            },
        )
        .await
    }

    /// Connect one Gmail account with an app-specific password supplied
    /// directly to the CLI. Authentication is proved before the credential is
    /// written to Skarbiec or a mailbox row is created.
    pub async fn connect_gmail_app_password(
        &self,
        organization_id: &str,
        email: &str,
        password: &str,
        display_name: Option<String>,
        mailbox_selector: Option<&str>,
    ) -> Result<Mailbox, AppError> {
        let email = validated_gmail_email(email)?;
        if password.is_empty() {
            return Err(AppError::invalid(
                "GMAIL_APP_PASSWORD_INPUT_INVALID",
                "Google app-specific password supplied through stdin must not be empty",
            ));
        }
        let target = mailbox_selector
            .map(|selector| self.resolve_mailbox(organization_id, selector))
            .transpose()?;
        let item_id = SkarbiecResolver::gmail_app_password_item_id(&email)?;
        verify_gmail_app_password(&email, password, &item_id).await?;
        let item_id = self
            .resolver
            .save_gmail_app_password(&email, password)
            .await?;
        let result = match target {
            Some(mailbox) => self.attach_gmail_password_mailbox(mailbox, item_id.clone()),
            None => {
                self.ensure_gmail_password_mailbox(
                    organization_id,
                    item_id.clone(),
                    email.clone(),
                    display_name,
                )
                .await
            }
        };
        result.map_err(|error| {
            AppError::new(
                error.status,
                error.code,
                format!(
                    "Google app-specific password was saved in Skarbiec item '{item_id}', but mailbox '{email}' was not created or updated: {}",
                    error.message
                ),
                error.retryable,
            )
        })
    }

    /// Connect an existing password item selected in Skarbiec Desktop. The
    /// loopback request carries only the item ID; the secret is resolved inside
    /// Skrzynka and never enters the API payload or response.
    pub async fn connect_gmail_app_password_item(
        &self,
        organization_id: &str,
        skarbiec_item_id: &str,
        display_name: Option<String>,
        mailbox_selector: Option<&str>,
    ) -> Result<Mailbox, AppError> {
        let target = mailbox_selector
            .map(|selector| self.resolve_mailbox(organization_id, selector))
            .transpose()?;
        let credentials = self.resolver.resolve_credentials(skarbiec_item_id).await?;
        let (email, password) = match credentials {
            ResolvedCredentials::Password { username, password } => {
                (validated_gmail_email(&username)?, password)
            }
            ResolvedCredentials::OAuth2 { .. } => {
                return Err(AppError::invalid(
                    "GMAIL_APP_PASSWORD_ITEM_INVALID",
                    "selected Skarbiec item does not contain a password credential",
                ));
            }
        };
        verify_gmail_app_password(&email, &password, skarbiec_item_id).await?;
        match target {
            Some(mailbox) => {
                self.attach_gmail_password_mailbox(mailbox, skarbiec_item_id.to_string())
            }
            None => {
                self.ensure_gmail_password_mailbox(
                    organization_id,
                    skarbiec_item_id.to_string(),
                    email,
                    display_name,
                )
                .await
            }
        }
    }

    fn attach_gmail_password_mailbox(
        &self,
        mut mailbox: Mailbox,
        skarbiec_item_id: String,
    ) -> Result<Mailbox, AppError> {
        if mailbox.smtp_skarbiec_item_id.is_none() {
            mailbox.smtp_skarbiec_item_id = Some(mailbox.skarbiec_item_id.clone());
        }
        mailbox.skarbiec_item_id = skarbiec_item_id;
        mailbox.imap_host = "imap.gmail.com".to_string();
        mailbox.imap_port = 993;
        mailbox.enabled = true;
        self.database.update_mailbox(&mailbox)
    }
    async fn ensure_gmail_password_mailbox(
        &self,
        organization_id: &str,
        skarbiec_item_id: String,
        email: String,
        display_name: Option<String>,
    ) -> Result<Mailbox, AppError> {
        let mut matches = self
            .database
            .list_mailboxes(organization_id)?
            .into_iter()
            .filter(|mailbox| {
                mailbox.skarbiec_item_id == skarbiec_item_id
                    || mailbox.email.eq_ignore_ascii_case(&email)
            })
            .collect::<Vec<_>>();
        if matches.len() > 1 {
            let ids = matches
                .iter()
                .map(|mailbox| mailbox.id.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            return Err(AppError::conflict(
                "MAILBOX_SELECTOR_AMBIGUOUS",
                format!(
                    "{email} names {} mailboxes ({ids}); select one by id",
                    matches.len()
                ),
            ));
        }
        if let Some(mut mailbox) = matches.pop() {
            mailbox.skarbiec_item_id = skarbiec_item_id;
            mailbox.imap_host = "imap.gmail.com".to_string();
            mailbox.imap_port = 993;
            mailbox.enabled = true;
            return self.database.update_mailbox(&mailbox);
        }
        self.create_mailbox(
            organization_id,
            CreateMailboxRequest {
                skarbiec_item_id,
                display_name: display_name.or_else(|| Some(email.clone())),
                email: Some(email),
                imap_host: Some("imap.gmail.com".to_string()),
                imap_port: Some(993),
                smtp_host: Some("smtp.gmail.com".to_string()),
                smtp_port: Some(587),
                smtp_security: Some(SmtpSecurity::Starttls),
                poll_interval_seconds: None,
            },
        )
        .await
    }

    pub async fn start_gmail_oauth(
        &self,
        organization_id: &str,
        request: StartGmailOAuthRequest,
    ) -> Result<StartGmailOAuthResponse, AppError> {
        self.gmail_oauth.start(organization_id, request).await
    }

    pub async fn complete_gmail_oauth_callback(
        &self,
        callback: GmailOAuthCallback,
    ) -> Result<Mailbox, AppError> {
        let authorization = self.gmail_oauth.complete_callback(callback).await?;
        self.ensure_gmail_mailbox(&authorization).await
    }

    pub async fn gmail_oauth_status(
        &self,
        organization_id: &str,
        flow_id: Uuid,
    ) -> Result<GmailOAuthStatusResponse, AppError> {
        let snapshot = self.gmail_oauth.status(flow_id, organization_id).await?;
        self.gmail_status_response(snapshot).await
    }

    async fn gmail_status_response(
        &self,
        snapshot: GmailOAuthFlowSnapshot,
    ) -> Result<GmailOAuthStatusResponse, AppError> {
        let (status, mailbox, error) = match snapshot.status {
            GmailOAuthFlowStatus::Pending => ("pending", None, None),
            GmailOAuthFlowStatus::Processing => ("processing", None, None),
            GmailOAuthFlowStatus::Completed(authorization) => (
                "completed",
                Some(self.ensure_gmail_mailbox(&authorization).await?),
                None,
            ),
            GmailOAuthFlowStatus::Failed(failure) => (
                "failed",
                None,
                Some(GmailOAuthStatusError {
                    code: failure.code,
                    message: failure.message,
                    retryable: failure.retryable,
                }),
            ),
        };
        Ok(GmailOAuthStatusResponse {
            flow_id: snapshot.flow_id,
            status,
            expires_at: snapshot.expires_at.to_rfc3339(),
            mailbox,
            error,
        })
    }

    async fn ensure_gmail_mailbox(
        &self,
        authorization: &crate::gmail::GmailAuthorization,
    ) -> Result<Mailbox, AppError> {
        if let Some(mailbox) = self
            .database
            .list_mailboxes(&authorization.organization_id)?
            .into_iter()
            .find(|mailbox| mailbox.skarbiec_item_id == authorization.credential_item_id)
        {
            return Ok(mailbox);
        }
        self.create_mailbox(
            &authorization.organization_id,
            CreateMailboxRequest {
                skarbiec_item_id: authorization.credential_item_id.clone(),
                display_name: Some(authorization.email.clone()),
                email: Some(authorization.email.clone()),
                imap_host: None,
                imap_port: None,
                smtp_host: None,
                smtp_port: None,
                smtp_security: Some(SmtpSecurity::Starttls),
                poll_interval_seconds: None,
            },
        )
        .await
    }
    pub async fn create_mailbox(
        &self,
        organization_id: &str,
        mut request: CreateMailboxRequest,
    ) -> Result<Mailbox, AppError> {
        if request.poll_interval_seconds.is_none() {
            request.poll_interval_seconds = Some(self.poll_interval_seconds);
        }
        let mut config = self.resolver.resolve_mailbox_config(&request).await?;
        config.organization_id = organization_id.to_string();
        self.database.create_mailbox(&config)
    }

    /// Adopt an existing IMAP mailbox by Skarbiec item reference and import one
    /// bounded provider page. Credentials are resolved only inside Skrzynka;
    /// the provider page is fully fetched and normalized before SQLite commits
    /// the mailbox, messages, and cursor in one transaction.
    pub async fn import_mailbox(
        &self,
        organization_id: &str,
        mut request: CreateMailboxRequest,
    ) -> Result<MailboxImportResult, AppError> {
        let _guard = self.operation_lock.lock().await;
        if request.poll_interval_seconds.is_none() {
            request.poll_interval_seconds = Some(self.poll_interval_seconds);
        }
        let mut config = self.resolver.resolve_mailbox_config(&request).await?;
        config.organization_id = organization_id.to_string();
        let existing = self
            .database
            .list_mailboxes(organization_id)?
            .into_iter()
            .find(|mailbox| mailbox.skarbiec_item_id == config.skarbiec_item_id);
        if let Some(mailbox) = existing.as_ref() {
            if !mailbox_matches_config(mailbox, &config) {
                return Err(AppError::conflict(
                    "MAILBOX_IMPORT_PROFILE_CONFLICT",
                    "the Skarbiec item is already attached with different mailbox settings; no import data was changed",
                ));
            }
        }
        let credentials = self
            .resolver
            .resolve_credentials(&config.skarbiec_item_id)
            .await?;
        let create_mailbox = existing.is_none();
        let mailbox_state = if create_mailbox {
            MailboxImportState::Imported
        } else {
            MailboxImportState::Unchanged
        };
        let mailbox = existing.unwrap_or_else(|| mailbox_from_config(&config));
        let source_item_id = config.skarbiec_item_id.clone();
        let database = self.database.clone();
        let (mailbox, imported, unchanged, fetched) = tokio::task::spawn_blocking(move || {
            let fetched = mail::fetch_messages(&mailbox, &credentials)?;
            let (mailbox, imported, unchanged) = database.commit_mailbox_import(
                &mailbox,
                create_mailbox,
                &fetched.messages,
                fetched.last_uid,
            )?;
            Ok::<_, AppError>((mailbox, imported, unchanged, fetched))
        })
        .await
        .map_err(|_| AppError::internal("mailbox import task stopped unexpectedly"))??;
        if imported + unchanged > 0 {
            if let Err(error) = crate::onboarding::record_mailbox_import_completed() {
                tracing::warn!(%error, "mailbox import persisted but first-use evidence could not be recorded");
            }
        }

        Ok(MailboxImportResult {
            applied: true,
            source: MailboxImportSource {
                kind: "imap_skarbiec_item",
                skarbiec_item_id: source_item_id,
            },
            mailbox_state,
            mailbox,
            messages: ImportItemCounts {
                imported,
                unchanged,
                conflicting: 0,
                rejected: fetched.skipped,
            },
            rejected_by_reason: fetched.rejected_by_reason,
            has_more: fetched.has_more,
        })
    }

    pub fn list_mailboxes(&self, organization_id: &str) -> Result<Vec<Mailbox>, AppError> {
        self.database.list_mailboxes(organization_id)
    }

    pub fn get_mailbox(&self, organization_id: &str, id: Uuid) -> Result<Mailbox, AppError> {
        self.database.get_mailbox(organization_id, id)
    }

    pub fn update_mailbox(
        &self,
        organization_id: &str,
        id: Uuid,
        request: UpdateMailboxRequest,
    ) -> Result<Mailbox, AppError> {
        let mut mailbox = self.database.get_mailbox(organization_id, id)?;
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

    pub fn delete_mailbox(&self, organization_id: &str, id: Uuid) -> Result<(), AppError> {
        self.database.delete_mailbox(organization_id, id)
    }

    pub async fn sync_mailbox(
        &self,
        organization_id: &str,
        id: Uuid,
    ) -> Result<SyncSummary, AppError> {
        self.database.get_mailbox(organization_id, id)?;
        self.sync_mailbox_internal(id).await
    }

    async fn sync_mailbox_internal(&self, id: Uuid) -> Result<SyncSummary, AppError> {
        let _guard = self.operation_lock.lock().await;
        let mailbox = self.database.get_mailbox_internal(id)?;
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
            let (mailbox, received, _) = database.commit_mailbox_import(
                &mailbox,
                false,
                &fetched.messages,
                fetched.last_uid,
            )?;
            Ok::<_, AppError>(SyncSummary {
                mailbox_id: mailbox.id,
                received,
                skipped: fetched.skipped,
                last_uid: fetched.last_uid,
                completed_at: mailbox
                    .last_sync_at
                    .unwrap_or_else(|| Utc::now().to_rfc3339()),
            })
        })
        .await
        .map_err(|_| AppError::internal("mailbox synchronization task stopped unexpectedly"))?;
        if let Err(error) = &result {
            let _ = self
                .database
                .record_sync_failure(id, error.code, &error.message);
        }
        if let Ok(summary) = &result {
            if summary.received > 0 {
                if let Err(error) = crate::onboarding::record_mailbox_import_completed() {
                    tracing::warn!(%error, "mailbox synchronization persisted but first-use evidence could not be recorded");
                }
            }
        }
        result
    }

    pub async fn sync_all(&self, organization_id: &str) -> Result<SyncAllSummary, AppError> {
        let mailboxes = self
            .database
            .list_mailboxes(organization_id)?
            .into_iter()
            .filter(|mailbox| mailbox.enabled)
            .collect::<Vec<_>>();
        self.sync_mailboxes(mailboxes).await
    }

    async fn sync_due(&self) -> Result<SyncAllSummary, AppError> {
        let now = Utc::now();
        let mailboxes = self
            .database
            .list_all_mailboxes()?
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
            match self.sync_mailbox_internal(mailbox.id).await {
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
        organization_id: &str,
        mailbox_id: Option<Uuid>,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<Message>, AppError> {
        self.database
            .list_messages(organization_id, mailbox_id, limit.clamp(1, 500), offset)
    }

    pub fn get_message(&self, organization_id: &str, id: Uuid) -> Result<Message, AppError> {
        self.database.get_message(organization_id, id)
    }

    pub fn list_replies(
        &self,
        organization_id: &str,
        message_id: Uuid,
    ) -> Result<Vec<ReplyAttempt>, AppError> {
        self.database.list_replies(organization_id, message_id)
    }

    pub async fn reply(
        &self,
        organization_id: &str,
        message_id: Uuid,
        request: CreateReplyRequest,
    ) -> Result<ReplyAttempt, AppError> {
        validate_reply_request(&request)?;
        let (attempt, created) = self.database.begin_reply(
            organization_id,
            message_id,
            request.idempotency_key.trim(),
            request.body.trim_end(),
        )?;
        if !created {
            return Ok(attempt);
        }
        let attempt =
            self.database
                .update_reply(attempt.id, DeliveryStatus::Sending, None, None, None)?;
        let message = self.database.get_message(organization_id, message_id)?;
        let mailbox = self
            .database
            .get_mailbox(organization_id, message.mailbox_id)?;
        let credentials = match self
            .resolver
            .resolve_credentials(mailbox.outbound_skarbiec_item_id())
            .await
        {
            Ok(credentials) => credentials,
            Err(error) => {
                let _ = self.database.update_reply(
                    attempt.id,
                    DeliveryStatus::Failed,
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
                DeliveryStatus::Sent,
                Some(&provider_message_id),
                None,
                None,
            ),
            Ok(Err(error)) if error.code == "SMTP_UNCERTAIN" => self.database.update_reply(
                attempt.id,
                DeliveryStatus::Uncertain,
                None,
                Some("REPLY_UNCERTAIN"),
                Some(&error.message),
            ),
            Ok(Err(error)) => {
                let _ = self.database.update_reply(
                    attempt.id,
                    DeliveryStatus::Failed,
                    None,
                    Some(error.code),
                    Some(&error.message),
                );
                Err(error)
            }
            Err(_) => self.database.update_reply(
                attempt.id,
                DeliveryStatus::Uncertain,
                None,
                Some("REPLY_UNCERTAIN"),
                Some("send task stopped before terminal SMTP evidence was recorded"),
            ),
        }
    }

    pub fn list_outbound(
        &self,
        organization_id: &str,
        mailbox_id: Option<Uuid>,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<OutboundMessage>, AppError> {
        self.database
            .list_outbound(organization_id, mailbox_id, limit.clamp(1, 500), offset)
    }

    pub fn get_outbound(
        &self,
        organization_id: &str,
        id: Uuid,
    ) -> Result<OutboundMessage, AppError> {
        self.database.get_outbound(organization_id, id)
    }

    /// A selector is either the mailbox id or the address itself. Operators
    /// know the address they send from; nothing should make them look up a
    /// UUID before they can use it.
    ///
    /// Two mailboxes can legitimately carry one address — the same account
    /// reached through a different Skarbiec item, say a provider relay beside a
    /// delegated Gmail row. Picking the first of them would send from whichever
    /// one sorted earlier, so an address that names more than one mailbox is
    /// refused with both ids instead.
    pub fn resolve_mailbox(
        &self,
        organization_id: &str,
        selector: &str,
    ) -> Result<Mailbox, AppError> {
        let selector = selector.trim();
        if let Ok(id) = Uuid::parse_str(selector) {
            return self.database.get_mailbox(organization_id, id);
        }
        let mut matches = self
            .database
            .list_mailboxes(organization_id)?
            .into_iter()
            .filter(|mailbox| mailbox.email.eq_ignore_ascii_case(selector))
            .collect::<Vec<_>>();
        if matches.len() > 1 {
            let ids = matches
                .iter()
                .map(|mailbox| mailbox.id.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            return Err(AppError::conflict(
                "MAILBOX_SELECTOR_AMBIGUOUS",
                format!(
                    "{selector} names {} mailboxes ({ids}); select one by id",
                    matches.len()
                ),
            ));
        }
        matches.pop().ok_or_else(|| AppError::not_found("mailbox"))
    }

    /// Originate mail from one mailbox. The row is claimed by its idempotency
    /// key before anything leaves this process, so repeating the same call
    /// returns the first attempt instead of handing the provider a second copy.
    pub async fn send_outbound(
        &self,
        organization_id: &str,
        mailbox_id: Uuid,
        request: CreateOutboundRequest,
    ) -> Result<OutboundMessage, AppError> {
        let normalized = validate_outbound_request(&request)?;
        let (outbound, created) = self.database.begin_outbound(
            organization_id,
            mailbox_id,
            request.idempotency_key.trim(),
            &normalized.recipients,
            normalized.cc.as_deref(),
            &normalized.subject,
            &normalized.body,
        )?;
        if !created {
            return Ok(outbound);
        }
        let outbound = self.database.update_outbound(
            outbound.id,
            DeliveryStatus::Sending,
            None,
            None,
            None,
        )?;
        let mailbox = self.database.get_mailbox(organization_id, mailbox_id)?;
        let credentials = match self
            .resolver
            .resolve_credentials(mailbox.outbound_skarbiec_item_id())
            .await
        {
            Ok(credentials) => credentials,
            Err(error) => {
                let _ = self.database.update_outbound(
                    outbound.id,
                    DeliveryStatus::Failed,
                    None,
                    Some(error.code),
                    Some(&error.message),
                );
                return Err(error);
            }
        };
        let pending = outbound.clone();
        let result = tokio::task::spawn_blocking(move || {
            mail::send_outbound(&mailbox, &credentials, &pending)
        })
        .await;
        match result {
            Ok(Ok(provider_message_id)) => self.database.update_outbound(
                outbound.id,
                DeliveryStatus::Sent,
                Some(&provider_message_id),
                None,
                None,
            ),
            Ok(Err(error)) if error.code == "SMTP_UNCERTAIN" => self.database.update_outbound(
                outbound.id,
                DeliveryStatus::Uncertain,
                None,
                Some("OUTBOUND_UNCERTAIN"),
                Some(&error.message),
            ),
            Ok(Err(error)) => {
                let _ = self.database.update_outbound(
                    outbound.id,
                    DeliveryStatus::Failed,
                    None,
                    Some(error.code),
                    Some(&error.message),
                );
                Err(error)
            }
            Err(_) => self.database.update_outbound(
                outbound.id,
                DeliveryStatus::Uncertain,
                None,
                Some("OUTBOUND_UNCERTAIN"),
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

fn mailbox_from_config(config: &MailboxConfig) -> Mailbox {
    let now = Utc::now().to_rfc3339();
    Mailbox {
        id: Uuid::new_v4(),
        organization_id: config.organization_id.clone(),
        skarbiec_item_id: config.skarbiec_item_id.clone(),
        smtp_skarbiec_item_id: config.smtp_skarbiec_item_id.clone(),
        display_name: config.display_name.clone(),
        email: config.email.clone(),
        imap_host: config.imap_host.clone(),
        imap_port: config.imap_port,
        smtp_host: config.smtp_host.clone(),
        smtp_port: config.smtp_port,
        smtp_security: config.smtp_security,
        poll_interval_seconds: config.poll_interval_seconds,
        enabled: true,
        last_uid: 0,
        last_sync_at: None,
        last_error_code: None,
        last_error_message: None,
        created_at: now.clone(),
        updated_at: now,
    }
}

fn mailbox_matches_config(mailbox: &Mailbox, config: &MailboxConfig) -> bool {
    mailbox.organization_id == config.organization_id
        && mailbox.skarbiec_item_id == config.skarbiec_item_id
        && mailbox.smtp_skarbiec_item_id == config.smtp_skarbiec_item_id
        && mailbox.display_name == config.display_name
        && mailbox.email == config.email
        && mailbox.imap_host == config.imap_host
        && mailbox.imap_port == config.imap_port
        && mailbox.smtp_host == config.smtp_host
        && mailbox.smtp_port == config.smtp_port
        && mailbox.smtp_security == config.smtp_security
        && mailbox.poll_interval_seconds == config.poll_interval_seconds
}

fn validated_gmail_email(email: &str) -> Result<String, AppError> {
    let email = email.trim();
    Address::from_str(email)
        .map_err(|_| AppError::invalid("GMAIL_PROFILE_INVALID", "email is not a valid address"))?;
    Ok(email.to_string())
}

async fn verify_gmail_app_password(
    email: &str,
    password: &str,
    skarbiec_item_id: &str,
) -> Result<(), AppError> {
    let email = email.to_string();
    let password = password.to_string();
    let skarbiec_item_id = skarbiec_item_id.to_string();
    tokio::task::spawn_blocking(move || {
        mail::verify_gmail_app_password(&email, &password, &skarbiec_item_id)
    })
    .await
    .map_err(|_| AppError::internal("Gmail credential verification stopped unexpectedly"))?
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

struct NormalizedOutbound {
    recipients: String,
    cc: Option<String>,
    subject: String,
    body: String,
}

fn validate_outbound_request(
    request: &CreateOutboundRequest,
) -> Result<NormalizedOutbound, AppError> {
    let key = request.idempotency_key.trim();
    if key.is_empty() || key.len() > 200 || key.chars().any(char::is_whitespace) {
        return Err(AppError::invalid(
            "IDEMPOTENCY_KEY_INVALID",
            "idempotency_key must contain 1 to 200 non-whitespace characters",
        ));
    }
    let subject = request.subject.trim().to_string();
    if subject.is_empty() || subject.chars().count() > 500 {
        return Err(AppError::invalid(
            "OUTBOUND_SUBJECT_INVALID",
            "subject must contain between 1 and 500 characters",
        ));
    }
    if request.body.trim().is_empty() {
        return Err(AppError::invalid(
            "OUTBOUND_BODY_INVALID",
            "outbound body must not be empty",
        ));
    }
    if request.body.len() > 256 * 1024 {
        return Err(AppError::invalid(
            "OUTBOUND_BODY_TOO_LARGE",
            "outbound body exceeds the 256 KiB limit",
        ));
    }
    let recipients = normalize_addresses(&request.to)?;
    if recipients.is_empty() {
        return Err(AppError::invalid(
            "OUTBOUND_RECIPIENT_INVALID",
            "at least one recipient address is required",
        ));
    }
    let cc = normalize_addresses(&request.cc)?;
    Ok(NormalizedOutbound {
        recipients: recipients.join(", "),
        cc: (!cc.is_empty()).then(|| cc.join(", ")),
        subject,
        body: request.body.trim_end().to_string(),
    })
}

/// Recipients are bare addresses, deduplicated in the order given. A display
/// name is refused here rather than at the provider, where the refusal would
/// arrive after the row was already claimed.
fn normalize_addresses(values: &[String]) -> Result<Vec<String>, AppError> {
    let mut addresses = Vec::with_capacity(values.len());
    for value in values {
        for candidate in value.split(',') {
            let candidate = candidate.trim();
            if candidate.is_empty() {
                continue;
            }
            Address::from_str(candidate).map_err(|_| {
                AppError::invalid(
                    "OUTBOUND_RECIPIENT_INVALID",
                    format!("{candidate} is not a valid email address"),
                )
            })?;
            if !addresses.iter().any(|existing| existing == candidate) {
                addresses.push(candidate.to_string());
            }
        }
    }
    Ok(addresses)
}
