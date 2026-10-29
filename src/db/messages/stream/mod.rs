//! The order messages were stored in (`messages.stored_order`, numbered in
//! commit order by `skrzynka_order_stored()` in `inbound.sql`), read by
//! watchers that hand on each new message once (docs/messages/watch.md).

use crate::db::sql::{params, Row};
use crate::db::Database;
use crate::error::AppError;
use uuid::Uuid;

/// One stored message's place in the store order.
pub struct StoredMessage {
    pub stored_order: i64,
    pub message_id: Uuid,
}

fn stored_row(row: &Row<'_>) -> crate::db::sql::Result<StoredMessage> {
    let id: String = row.get("id")?;
    let message_id = Uuid::parse_str(&id).map_err(|error| {
        crate::db::sql::Error::Conversion(format!("{id:?} is not a UUID: {error}"))
    })?;
    Ok(StoredMessage {
        stored_order: row.get("stored_order")?,
        message_id,
    })
}

impl Database {
    /// The organization's last stored message's number; absent when it holds none.
    pub fn last_stored(&self, organization_id: &str) -> Result<Option<i64>, AppError> {
        let connection = self.lock()?;
        let last = connection.query_row(
            "SELECT MAX(messages.stored_order) AS last
             FROM messages JOIN mailboxes ON mailboxes.id=messages.mailbox_id
             WHERE mailboxes.organization_id=$1",
            params![organization_id],
            |row| row.get::<_, Option<i64>>("last"),
        )?;
        Ok(last)
    }

    /// Every message of the organization, or of one mailbox, stored after
    /// `after` (every one when absent), in the order they were stored.
    pub fn stored_after(
        &self,
        organization_id: &str,
        mailbox_id: Option<Uuid>,
        after: Option<i64>,
    ) -> Result<Vec<StoredMessage>, AppError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT messages.id AS id, messages.stored_order AS stored_order
             FROM messages JOIN mailboxes ON mailboxes.id=messages.mailbox_id
             WHERE mailboxes.organization_id=$1
               AND ($2::text IS NULL OR messages.mailbox_id=$2)
               AND ($3::bigint IS NULL OR messages.stored_order > $3)
             ORDER BY messages.stored_order",
        )?;
        let rows = statement.query_map(
            params![organization_id, mailbox_id.map(|id| id.to_string()), after],
            stored_row,
        )?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}
