//! How a mailbox's sends ended, counted from what Skrzynka recorded of them:
//! replies, originated mail and unsubscribe requests, by status and refusal,
//! and the delivery status notifications that later reported on them.

use super::super::sql::{self, params, Row};
use super::super::Database;
use crate::{error::AppError, mail::StoredBounce, models::DeliveryStatus};
use uuid::Uuid;

mod days;

/// One count: which kind of send, the status it ended in, the refusal code
/// it carries (absent for one that carries none), and how many.
pub type DeliveryCount = (String, DeliveryStatus, Option<String>, u64);

/// The columns every arm of the bounce query selects, after its kind and send ID.
const BOUNCE_COLUMNS: &str = "message_bounces.message_id AS report_message_id,
        reports.received_at AS received_at, outcomes.recipient AS recipient,
        outcomes.action AS action, outcomes.status AS status,
        outcomes.diagnostic AS diagnostic, message_bounces.unreadable AS unreadable";
/// The joins every arm shares: the stored report, the organization it was
/// received in, and its recipient outcomes.
const BOUNCE_JOINS: &str = "JOIN messages AS reports ON reports.id=message_bounces.message_id
        JOIN mailboxes AS report_boxes ON report_boxes.id=reports.mailbox_id
        LEFT JOIN message_bounce_recipients AS outcomes
            ON outcomes.message_id=message_bounces.message_id";

/// A send or report counts when its instant lies in the window: at or after
/// `$3` and before `$4`, each RFC 3339 text or absent for no bound.
const IN_WINDOW: &str = "($3::text IS NULL OR {at}::timestamptz >= $3::timestamptz)
                 AND ($4::text IS NULL OR {at}::timestamptz < $4::timestamptz)";

impl Database {
    /// Every recorded send of `mailbox_id` claimed in the window, counted by
    /// kind, status and code.
    pub fn delivery_counts(
        &self,
        organization_id: &str,
        mailbox_id: Uuid,
        window: (Option<String>, Option<String>),
    ) -> Result<Vec<DeliveryCount>, AppError> {
        self.get_mailbox(organization_id, mailbox_id)?;
        let connection = self.lock()?;
        let within = |at: &str| IN_WINDOW.replace("{at}", at);
        let mut statement = connection.prepare(&format!(
            "SELECT 'reply' AS kind, reply_attempts.status AS status,
                    reply_attempts.error_code AS error_code, COUNT(*) AS count
             FROM reply_attempts JOIN messages ON messages.id=reply_attempts.message_id
             WHERE messages.mailbox_id=$1 AND {}
                 AND EXISTS (SELECT FROM mailboxes WHERE mailboxes.id=$1 AND mailboxes.organization_id=$2)
             GROUP BY reply_attempts.status, reply_attempts.error_code
             UNION ALL
             SELECT 'outbound', status, error_code, COUNT(*)
             FROM outbound_messages WHERE mailbox_id=$1 AND {}
             GROUP BY status, error_code
             UNION ALL
             SELECT 'unsubscribe', unsubscribe_attempts.status, unsubscribe_attempts.error_code,
                    COUNT(*)
             FROM unsubscribe_attempts JOIN messages ON messages.id=unsubscribe_attempts.message_id
             WHERE messages.mailbox_id=$1 AND {}
             GROUP BY unsubscribe_attempts.status, unsubscribe_attempts.error_code",
            within("reply_attempts.created_at"),
            within("outbound_messages.created_at"),
            within("unsubscribe_attempts.created_at"),
        ))?;
        let rows = statement.query_map(
            params![mailbox_id.to_string(), organization_id, window.0, window.1],
            count_from_row,
        )?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Every stored delivery status notification about one of the mailbox's
    /// replies, originated messages or unsubscribe mails, matched by the
    /// Message-ID the send went out with, newest report first.
    pub fn mailbox_bounces(
        &self,
        organization_id: &str,
        mailbox_id: Uuid,
        window: (Option<String>, Option<String>),
    ) -> Result<Vec<StoredBounce>, AppError> {
        self.get_mailbox(organization_id, mailbox_id)?;
        let connection = self.lock()?;
        let within = IN_WINDOW.replace("{at}", "reports.received_at");
        let mut statement = connection.prepare(&format!(
            "SELECT 'reply' AS kind, reply_attempts.id AS send_id, {BOUNCE_COLUMNS}
             FROM message_bounces {BOUNCE_JOINS}
             JOIN reply_attempts
                 ON reply_attempts.provider_message_id=message_bounces.original_message_id
             JOIN messages AS answered ON answered.id=reply_attempts.message_id
             WHERE answered.mailbox_id=$1 AND report_boxes.organization_id=$2 AND {within}
             UNION ALL
             SELECT 'outbound', outbound_messages.id, {BOUNCE_COLUMNS}
             FROM message_bounces {BOUNCE_JOINS}
             JOIN outbound_messages
                 ON outbound_messages.provider_message_id=message_bounces.original_message_id
             WHERE outbound_messages.mailbox_id=$1 AND report_boxes.organization_id=$2 AND {within}
             UNION ALL
             SELECT 'unsubscribe', unsubscribe_attempts.id, {BOUNCE_COLUMNS}
             FROM message_bounces {BOUNCE_JOINS}
             JOIN unsubscribe_attempts
                 ON unsubscribe_attempts.provider_message_id=message_bounces.original_message_id
             JOIN messages AS left_from ON left_from.id=unsubscribe_attempts.message_id
             WHERE left_from.mailbox_id=$1 AND report_boxes.organization_id=$2 AND {within}
             ORDER BY received_at DESC"
        ))?;
        let rows = statement.query_map(
            params![mailbox_id.to_string(), organization_id, window.0, window.1],
            bounce_from_row,
        )?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}

fn bounce_from_row(row: &Row<'_>) -> sql::Result<StoredBounce> {
    Ok(StoredBounce {
        kind: row.get("kind")?,
        send_id: row.get("send_id")?,
        report_message_id: row.get("report_message_id")?,
        received_at: row.get("received_at")?,
        recipient: row.get("recipient")?,
        action: row.get("action")?,
        status: row.get("status")?,
        diagnostic: row.get("diagnostic")?,
        unreadable: row.get("unreadable")?,
    })
}

fn count_from_row(row: &Row<'_>) -> sql::Result<DeliveryCount> {
    let count = row.get::<_, i64>("count")?;
    Ok((
        row.get("kind")?,
        row.get::<_, String>("status")?
            .parse::<DeliveryStatus>()
            .map_err(|error| {
                sql::Error::Conversion(format!("column status holds no delivery status: {error}"))
            })?,
        row.get("error_code")?,
        u64::try_from(count).map_err(|error| {
            sql::Error::Conversion(format!("count {count} is not a count: {error}"))
        })?,
    ))
}
