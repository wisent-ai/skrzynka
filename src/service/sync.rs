//! Mail reception: one mailbox, all of them, or a mailbox watched for new mail.

use super::AppState;
use crate::{
    error::AppError,
    mail,
    models::{Mailbox, MailboxReconciliation, MailboxSyncResult, SyncAllSummary, SyncSummary},
};
use chrono::Utc;
use uuid::Uuid;

impl AppState {
    pub async fn sync_mailbox(
        &self,
        organization_id: &str,
        id: Uuid,
    ) -> Result<SyncSummary, AppError> {
        self.database.get_mailbox(organization_id, id)?;
        self.sync_mailbox_internal(id).await
    }

    pub(super) async fn sync_mailbox_internal(&self, id: Uuid) -> Result<SyncSummary, AppError> {
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
            let mut current = mailbox;
            let mut received = 0usize;
            let fetched =
                mail::fetch_messages(&current.clone(), &credentials, |messages, last_uid| {
                    let (stored, added, _) =
                        database.commit_mailbox_import(&current, false, messages, last_uid)?;
                    received += added;
                    current = stored;
                    Ok(())
                })?;
            Ok::<_, AppError>(SyncSummary {
                mailbox_id: current.id,
                received,
                skipped: fetched.skipped,
                last_uid: fetched.last_uid,
                completed_at: current
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
        let reconciliation = self.reconcile_mailboxes(Some(organization_id)).await?;
        let mailboxes = self
            .database
            .list_mailboxes(organization_id)?
            .into_iter()
            .filter(|mailbox| mailbox.enabled)
            .collect::<Vec<_>>();
        self.sync_mailboxes(reconciliation, mailboxes).await
    }

    /// One mailbox's reception: import what is waiting, then hold an IMAP IDLE
    /// until the provider announces new mail, and repeat. Ends, with the failure
    /// recorded on the mailbox, when a sync or the IDLE connection fails; the
    /// watching loop starts it again on its next round.
    pub(super) async fn watch_mailbox(&self, id: Uuid) {
        loop {
            if let Err(error) = self.sync_mailbox_internal(id).await {
                tracing::warn!(mailbox = %id, code = error.code, message = %error.message, "mailbox sync failed; its watcher stops until the next round");
                return;
            }
            let mailbox = match self.database.get_mailbox_internal(id) {
                Ok(mailbox) if mailbox.enabled => mailbox,
                Ok(_) => return,
                Err(error) => {
                    tracing::warn!(mailbox = %id, code = error.code, "mailbox could not be read");
                    return;
                }
            };
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
                    return;
                }
            };
            let waited = tokio::task::spawn_blocking(move || {
                mail::wait_for_new_mail(&mailbox, &credentials)
            })
            .await
            .map_err(|_| AppError::internal("mailbox IDLE task stopped unexpectedly"))
            .and_then(|result| result);
            if let Err(error) = waited {
                let _ = self
                    .database
                    .record_sync_failure(id, error.code, &error.message);
                tracing::warn!(mailbox = %id, code = error.code, message = %error.message, "mailbox IDLE ended; its watcher stops until the next round");
                return;
            }
        }
    }

    pub(super) async fn sync_mailboxes(
        &self,
        reconciliation: MailboxReconciliation,
        mailboxes: Vec<Mailbox>,
    ) -> Result<SyncAllSummary, AppError> {
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
            reconciliation,
            mailboxes: results,
        })
    }
}
