//! Mailbox records: creation, adoption of an existing IMAP mailbox, reads, updates, deletion.

use super::AppState;
use crate::{
    db::MailboxConfig,
    error::AppError,
    mail,
    models::{
        CreateMailboxRequest, ImportItemCounts, Mailbox, MailboxImportResult, MailboxImportSource,
        MailboxImportState, UpdateMailboxRequest, MAX_DISPLAY_NAME_CHARS, MAX_HOST_LENGTH,
        MAX_POLL_INTERVAL_SECONDS, MIN_POLL_INTERVAL_SECONDS,
    },
};
use chrono::Utc;
use lettre::Address;
use std::str::FromStr;
use uuid::Uuid;

impl AppState {
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

fn validate_mailbox(mailbox: &Mailbox) -> Result<(), AppError> {
    if mailbox.display_name.is_empty()
        || mailbox.display_name.chars().count() > MAX_DISPLAY_NAME_CHARS
    {
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
            || value.len() > MAX_HOST_LENGTH
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
    if !(MIN_POLL_INTERVAL_SECONDS..=MAX_POLL_INTERVAL_SECONDS)
        .contains(&mailbox.poll_interval_seconds)
    {
        return Err(AppError::invalid(
            "MAILBOX_PROFILE_INVALID",
            "poll_interval_seconds must be between 15 and 86400",
        ));
    }
    Ok(())
}
