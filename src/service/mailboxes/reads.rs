//! Mailboxes as an operator reads them: each with the command that recovers
//! it and the credential source its Skarbiec item is observed in.

use super::super::AppState;
use crate::{error::AppError, models::Mailbox};
use uuid::Uuid;

impl AppState {
    pub async fn list_mailboxes(&self, organization_id: &str) -> Result<Vec<Mailbox>, AppError> {
        self.reconcile_mailboxes(Some(organization_id)).await?;
        let mut mailboxes = self.database.list_mailboxes(organization_id)?;
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
