//! Mailboxes as Skarbiec declares them. An item is a mailbox exactly when it
//! carries [`MAILBOX_TAG`]; Skrzynka keeps only the mail it imported and where
//! synchronization stands. Reconciliation brings that state in line with the
//! vault, and declaring or undeclaring edits the tag in Skarbiec and then
//! reconciles, so there is no second list to drift from the first.

use super::AppState;
use crate::{
    db::MailboxConfig,
    error::AppError,
    mail,
    models::{
        ImportItemCounts, Mailbox, MailboxDeclarationRefusal, MailboxImportResult,
        MailboxImportSource, MailboxImportState, MailboxReconciliation,
    },
    skarbiec::MAILBOX_TAG,
};
use chrono::Utc;
use uuid::Uuid;

impl AppState {
    /// Bring Skrzynka's mailboxes in line with what Skarbiec declares.
    ///
    /// A declared item gets a mailbox whose profile is the item's; a mailbox
    /// whose item no longer carries the tag stops being polled and keeps its
    /// mail. New mailboxes are created in `create_for`; the background poll
    /// passes `None`, because only a caller knows which organization it is.
    pub async fn reconcile_mailboxes(
        &self,
        create_for: Option<&str>,
    ) -> Result<MailboxReconciliation, AppError> {
        let declared = self.resolver.declared_mailbox_items().await?;
        let existing = self.database.list_all_mailboxes()?;
        let mut report = MailboxReconciliation {
            declared: declared.len(),
            ..Default::default()
        };
        for item_id in &declared {
            let row = existing
                .iter()
                .find(|mailbox| &mailbox.skarbiec_item_id == item_id);
            let mut config = match self
                .resolver
                .resolve_mailbox_config(item_id, self.poll_interval_seconds)
                .await
            {
                Ok(config) => config,
                Err(error) => {
                    if let Some(mailbox) = row {
                        self.database
                            .record_sync_failure(mailbox.id, error.code, &error.message)?;
                    }
                    report.refused.push(MailboxDeclarationRefusal {
                        skarbiec_item_id: item_id.clone(),
                        code: error.code.to_string(),
                        message: error.message,
                    });
                    continue;
                }
            };
            match row {
                Some(mailbox) => {
                    config.organization_id = mailbox.organization_id.clone();
                    if let Some(refusal) = endpoint_conflict(mailbox, &config) {
                        self.database
                            .record_sync_failure(mailbox.id, &refusal.code, &refusal.message)?;
                        report.refused.push(refusal);
                        continue;
                    }
                    if !mailbox.enabled || !mailbox_matches_config(mailbox, &config) {
                        let mut updated = mailbox.clone();
                        apply_config(&mut updated, &config);
                        updated.enabled = true;
                        self.database.update_mailbox(&updated)?;
                        report.updated.push(mailbox.id);
                    }
                }
                None => {
                    if let Some(organization_id) = create_for {
                        config.organization_id = organization_id.to_string();
                        report.created.push(self.database.create_mailbox(&config)?.id);
                    }
                }
            }
        }
        for mailbox in existing
            .iter()
            .filter(|mailbox| mailbox.enabled && !declared.contains(&mailbox.skarbiec_item_id))
        {
            let mut undeclared = mailbox.clone();
            undeclared.enabled = false;
            self.database.update_mailbox(&undeclared)?;
            self.database.record_sync_failure(
                mailbox.id,
                "MAILBOX_NOT_DECLARED",
                &format!(
                    "Skarbiec item '{}' does not carry {MAILBOX_TAG}; Skrzynka keeps its mail and no longer polls it",
                    mailbox.skarbiec_item_id
                ),
            )?;
            report.undeclared.push(mailbox.id);
        }
        Ok(report)
    }

    /// Declare one Skarbiec item a mailbox and import its first INBOX page.
    ///
    /// The tag is written to Skarbiec first, so the declaration outlives this
    /// process; the page is then fully fetched and normalized before SQLite
    /// commits the mailbox, messages, and cursor in one transaction.
    pub async fn declare_mailbox(
        &self,
        organization_id: &str,
        skarbiec_item_id: &str,
    ) -> Result<MailboxImportResult, AppError> {
        let mut config = self
            .resolver
            .resolve_mailbox_config(skarbiec_item_id, self.poll_interval_seconds)
            .await?;
        let credentials = self.resolver.resolve_credentials(skarbiec_item_id).await?;
        self.resolver
            .set_mailbox_declared(skarbiec_item_id, true)
            .await?;
        let _guard = self.operation_lock.lock().await;
        let existing = self
            .database
            .list_all_mailboxes()?
            .into_iter()
            .find(|mailbox| mailbox.skarbiec_item_id == skarbiec_item_id);
        config.organization_id = existing.as_ref().map_or_else(
            || organization_id.to_string(),
            |mailbox| mailbox.organization_id.clone(),
        );
        if let Some(refusal) = existing
            .as_ref()
            .and_then(|mailbox| endpoint_conflict(mailbox, &config))
        {
            return Err(AppError::conflict(
                "MAILBOX_IMPORT_PROFILE_CONFLICT",
                refusal.message,
            ));
        }
        let create_mailbox = existing.is_none();
        let mailbox_state = match existing.as_ref() {
            None => MailboxImportState::Imported,
            Some(mailbox) if !mailbox.enabled || !mailbox_matches_config(mailbox, &config) => {
                MailboxImportState::Updated
            }
            Some(_) => MailboxImportState::Unchanged,
        };
        let mut mailbox = existing.unwrap_or_else(|| mailbox_from_config(&config));
        apply_config(&mut mailbox, &config);
        mailbox.enabled = true;
        if !create_mailbox {
            mailbox = self.database.update_mailbox(&mailbox)?;
        }
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

    /// Stop treating one mailbox's item as a mailbox: remove the tag in
    /// Skarbiec, then reconcile. The mailbox keeps its mail and stops polling.
    pub async fn undeclare_mailbox(
        &self,
        organization_id: &str,
        id: Uuid,
    ) -> Result<Mailbox, AppError> {
        let mailbox = self.database.get_mailbox(organization_id, id)?;
        self.resolver
            .set_mailbox_declared(&mailbox.skarbiec_item_id, false)
            .await?;
        self.reconcile_mailboxes(None).await?;
        self.database.get_mailbox(organization_id, id)
    }

    pub async fn list_mailboxes(&self, organization_id: &str) -> Result<Vec<Mailbox>, AppError> {
        self.reconcile_mailboxes(Some(organization_id)).await?;
        self.database.list_mailboxes(organization_id)
    }

    pub fn get_mailbox(&self, organization_id: &str, id: Uuid) -> Result<Mailbox, AppError> {
        self.database.get_mailbox(organization_id, id)
    }

    /// Delete one mailbox's local mail. Only a mailbox Skarbiec no longer
    /// declares can go: a declared one would be recreated by the next pass.
    pub fn delete_mailbox(&self, organization_id: &str, id: Uuid) -> Result<(), AppError> {
        let mailbox = self.database.get_mailbox(organization_id, id)?;
        if mailbox.enabled {
            return Err(AppError::conflict(
                "MAILBOX_STILL_DECLARED",
                format!(
                    "Skarbiec item '{}' still carries {MAILBOX_TAG}; undeclare the mailbox before removing its local mail",
                    mailbox.skarbiec_item_id
                ),
            ));
        }
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

/// A declared item whose receiving address or IMAP endpoint changed: the
/// retained UID cursor belongs to the old mailbox and cannot be reused.
fn endpoint_conflict(
    mailbox: &Mailbox,
    config: &MailboxConfig,
) -> Option<MailboxDeclarationRefusal> {
    (mailbox.email != config.email
        || mailbox.imap_host != config.imap_host
        || mailbox.imap_port != config.imap_port)
        .then(|| MailboxDeclarationRefusal {
            skarbiec_item_id: config.skarbiec_item_id.clone(),
            code: "MAILBOX_IMPORT_PROFILE_CONFLICT".to_string(),
            message: "the Skarbiec profile changes the receiving address or IMAP endpoint; the retained UID cursor cannot be reused and no import data was changed".to_string(),
        })
}

fn apply_config(mailbox: &mut Mailbox, config: &MailboxConfig) {
    mailbox.smtp_skarbiec_item_id = config.smtp_skarbiec_item_id.clone();
    mailbox.display_name = config.display_name.clone();
    mailbox.smtp_host = config.smtp_host.clone();
    mailbox.smtp_port = config.smtp_port;
    mailbox.smtp_security = config.smtp_security;
    mailbox.poll_interval_seconds = config.poll_interval_seconds;
}
