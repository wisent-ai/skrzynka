//! Polling: one mailbox, every due mailbox, or all of them.

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

    /// The background poll. A vault that cannot be read this tick does not
    /// stop mail that is already declared: the tick polls what it has and the
    /// next one reads Skarbiec again.
    pub(super) async fn sync_due(&self) -> Result<SyncAllSummary, AppError> {
        let reconciliation = match self.reconcile_mailboxes(None).await {
            Ok(reconciliation) => reconciliation,
            Err(error) => {
                tracing::warn!(code = error.code, message = %error.message, "Skarbiec mailbox declarations could not be read");
                MailboxReconciliation::default()
            }
        };
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
        self.sync_mailboxes(reconciliation, mailboxes).await
    }

    /// How long until the next enabled mailbox is due; zero when one already is, and this
    /// process's own interval when no mailbox is enabled.
    pub(super) fn next_due_in(&self) -> Result<std::time::Duration, AppError> {
        let now = Utc::now();
        let earliest = self
            .database
            .list_all_mailboxes()?
            .into_iter()
            .filter(|mailbox| mailbox.enabled)
            .map(|mailbox| {
                let interval = i64::try_from(mailbox.poll_interval_seconds).unwrap_or(i64::MAX);
                mailbox
                    .last_sync_at
                    .as_deref()
                    .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
                    .map(|last| {
                        interval.saturating_sub(
                            now.signed_duration_since(last.with_timezone(&Utc))
                                .num_seconds(),
                        )
                    })
                    .unwrap_or(0)
            })
            .min();
        let seconds = match earliest {
            Some(seconds) => u64::try_from(seconds).unwrap_or(0),
            None => self.poll_interval_seconds.unwrap_or_default(),
        };
        Ok(std::time::Duration::from_secs(seconds))
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
