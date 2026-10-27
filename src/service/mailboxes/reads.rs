//! Mailboxes as an operator reads them: each with the command that recovers
//! it and the credential source its Skarbiec item is observed in, and the
//! same mailboxes as the table `mailbox list --csv` writes.

use super::super::{
    tables::{header, optional, row},
    AppState, MailboxState,
};
use crate::{error::AppError, models::Mailbox};
use uuid::Uuid;

/// The table's header: the fields of a mailbox an inventory reads.
const TABLE_HEADER: &[&str] = &[
    "id", "email", "display_name", "enabled", "imap_host", "smtp_host", "last_uid", "last_sync_at", "last_error_code",
    "last_error_message", "created_at",
];

impl Mailbox {
    /// The mailboxes as a CSV table (RFC 4180): one row each under the
    /// header, in the order given, an instant or error a mailbox does not
    /// hold left empty. No Skarbiec item, password or token is written.
    pub fn table(mailboxes: &[Mailbox]) -> String {
        let mut table = header(TABLE_HEADER);
        for mailbox in mailboxes {
            table.push_str(&row(&[
                mailbox.id.to_string(),
                mailbox.email.clone(),
                mailbox.display_name.clone(),
                mailbox.enabled.to_string(),
                mailbox.imap_host.clone(),
                mailbox.smtp_host.clone(),
                mailbox.last_uid.to_string(),
                optional(&mailbox.last_sync_at),
                optional(&mailbox.last_error_code),
                optional(&mailbox.last_error_message),
                mailbox.created_at.clone(),
            ]));
        }
        table
    }
}

impl AppState {
    /// Every mailbox of the organization, or with `state` only those in it,
    /// each with its recovery command and observed credential source.
    pub async fn list_mailboxes(&self, organization_id: &str, state: Option<MailboxState>) -> Result<Vec<Mailbox>, AppError> {
        self.reconcile_mailboxes(Some(organization_id)).await?;
        let mut mailboxes: Vec<Mailbox> = self
            .database
            .list_mailboxes(organization_id)?
            .into_iter()
            .filter(|mailbox| state.is_none_or(|wanted| wanted.holds(mailbox)))
            .collect();
        if !mailboxes.is_empty() {
            let source = self.resolver.credential_source().await;
            let items = self.resolver.list_inventory().await;
            for mailbox in &mut mailboxes {
                mailbox.recovery_command =
                    mailbox.recovery_command(self.resolver.status_executable());
                let mut observed = source.clone();
                observed.observe_item(&mailbox.skarbiec_item_id, &items);
                mailbox.credential_source = Some(observed);
            }
        }
        Ok(mailboxes)
    }

    pub async fn get_mailbox(&self, organization_id: &str, id: Uuid) -> Result<Mailbox, AppError> {
        let mut mailbox = self.database.get_mailbox(organization_id, id)?;
        mailbox.recovery_command = mailbox.recovery_command(self.resolver.status_executable());
        let mut source = self.resolver.credential_source().await;
        source.observe_item(
            &mailbox.skarbiec_item_id,
            &self.resolver.list_inventory().await,
        );
        mailbox.credential_source = Some(source);
        Ok(mailbox)
    }
}
