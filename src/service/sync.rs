//! Polling: one mailbox, every due mailbox, or all of them.

use super::AppState;
use crate::{
    error::AppError,
    mail,
    models::{Mailbox, MailboxSyncResult, SyncAllSummary, SyncSummary},
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

    pub(super) async fn sync_due(&self) -> Result<SyncAllSummary, AppError> {
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

    pub(super) async fn sync_mailboxes(
        &self,
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
            mailboxes: results,
        })
    }
}
