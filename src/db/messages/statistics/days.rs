//! A mailbox's sends counted per UTC day: what a warm-up is read against.

use super::super::super::sql::{self, params, Row};
use super::super::super::Database;
use super::IN_WINDOW;
use crate::{error::AppError, models::DeliveryStatus};
use uuid::Uuid;

/// One count: the UTC day (`YYYY-MM-DD`) sends were claimed on, the status
/// they ended in, and how many, every kind of send together.
pub type DayCount = (String, DeliveryStatus, u64);

impl Database {
    /// Every recorded send of `mailbox_id` claimed in the window, counted by
    /// the UTC day it was claimed on and its status, oldest day first.
    pub fn delivery_days(
        &self,
        organization_id: &str,
        mailbox_id: Uuid,
        window: (Option<String>, Option<String>),
    ) -> Result<Vec<DayCount>, AppError> {
        self.get_mailbox(organization_id, mailbox_id)?;
        let connection = self.lock()?;
        let within = |at: &str| IN_WINDOW.replace("{at}", at);
        let mut statement = connection.prepare(&format!(
            "SELECT day, status, SUM(count)::bigint AS count FROM (
                 SELECT (reply_attempts.created_at::timestamptz AT TIME ZONE 'UTC')::date::text AS day,
                        reply_attempts.status AS status, COUNT(*) AS count
                 FROM reply_attempts JOIN messages ON messages.id=reply_attempts.message_id
                 WHERE messages.mailbox_id=$1 AND {}
                     AND EXISTS (SELECT FROM mailboxes WHERE mailboxes.id=$1 AND mailboxes.organization_id=$2)
                 GROUP BY day, reply_attempts.status
                 UNION ALL
                 SELECT (created_at::timestamptz AT TIME ZONE 'UTC')::date::text, status, COUNT(*)
                 FROM outbound_messages WHERE mailbox_id=$1 AND {}
                 GROUP BY 1, status
                 UNION ALL
                 SELECT (unsubscribe_attempts.created_at::timestamptz AT TIME ZONE 'UTC')::date::text,
                        unsubscribe_attempts.status, COUNT(*)
                 FROM unsubscribe_attempts JOIN messages ON messages.id=unsubscribe_attempts.message_id
                 WHERE messages.mailbox_id=$1 AND {}
                 GROUP BY 1, unsubscribe_attempts.status
             ) AS sends
             GROUP BY day, status
             ORDER BY day",
            within("reply_attempts.created_at"),
            within("outbound_messages.created_at"),
            within("unsubscribe_attempts.created_at"),
        ))?;
        let rows = statement.query_map(
            params![mailbox_id.to_string(), organization_id, window.0, window.1],
            day_from_row,
        )?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}

fn day_from_row(row: &Row<'_>) -> sql::Result<DayCount> {
    let count = row.get::<_, i64>("count")?;
    Ok((
        row.get("day")?,
        row.get::<_, String>("status")?
            .parse::<DeliveryStatus>()
            .map_err(|error| {
                sql::Error::Conversion(format!("column status holds no delivery status: {error}"))
            })?,
        u64::try_from(count).map_err(|error| {
            sql::Error::Conversion(format!("count {count} is not a count: {error}"))
        })?,
    ))
}
