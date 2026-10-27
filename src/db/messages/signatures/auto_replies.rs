//! A mailbox's automatic reply and the responder's decisions, beside the
//! signature as the other text a mailbox answers with
//! (docs/replies/auto-reply.md).

use crate::db::sql::{params, OptionalExtension, Row};
use crate::db::Database;
use crate::error::AppError;
use crate::mail::{AutoReply, AutoReplyDecision};
use chrono::Utc;
use stado_database::notifications::{self, Subscription};
use uuid::Uuid;

/// One received message the responder has not decided on yet.
pub struct AutoReplyCandidate {
    pub message_id: Uuid,
    pub sender: String,
    pub reply_to: Option<String>,
}

fn auto_reply_row(row: &Row<'_>) -> crate::db::sql::Result<AutoReply> {
    Ok(AutoReply {
        mailbox_id: uuid(row.get("mailbox_id")?)?,
        body: row.get("body")?,
        starts_at: row.get("starts_at")?,
        ends_at: row.get("ends_at")?,
        resend_after_seconds: row.get("resend_after_seconds")?,
        updated_at: row.get("updated_at")?,
    })
}

fn uuid(value: String) -> crate::db::sql::Result<Uuid> {
    Uuid::parse_str(&value)
        .map_err(|error| crate::db::sql::Error::Conversion(format!("{value:?} is not a UUID: {error}")))
}

impl Database {
    /// The mailbox's automatic reply, absent when it answers nothing.
    pub fn auto_reply(&self, organization_id: &str, mailbox_id: Uuid) -> Result<Option<AutoReply>, AppError> {
        self.get_mailbox(organization_id, mailbox_id)?;
        self.auto_reply_internal(mailbox_id)
    }

    pub fn auto_reply_internal(&self, mailbox_id: Uuid) -> Result<Option<AutoReply>, AppError> {
        self.lock()?
            .query_row(
                "SELECT mailbox_id, body, starts_at, ends_at, resend_after_seconds, updated_at
                 FROM mailbox_auto_replies WHERE mailbox_id=$1",
                params![mailbox_id.to_string()],
                auto_reply_row,
            )
            .optional()
            .map_err(Into::into)
    }

    /// Store the automatic reply (body, start, end, seconds between answers to
    /// one sender) or remove it when absent, and answer what is stored.
    pub fn set_auto_reply(
        &self,
        organization_id: &str,
        mailbox_id: Uuid,
        reply: Option<(&str, &str, Option<&str>, i64)>,
    ) -> Result<Option<AutoReply>, AppError> {
        self.get_mailbox(organization_id, mailbox_id)?;
        let id = mailbox_id.to_string();
        match reply {
            Some((body, starts_at, ends_at, resend_after_seconds)) => {
                self.lock()?.execute(
                    "INSERT INTO mailbox_auto_replies
                        (mailbox_id, body, starts_at, ends_at, resend_after_seconds, updated_at)
                     VALUES ($1, $2, $3, $4, $5, $6)
                     ON CONFLICT (mailbox_id) DO UPDATE
                     SET body=EXCLUDED.body, starts_at=EXCLUDED.starts_at, ends_at=EXCLUDED.ends_at,
                         resend_after_seconds=EXCLUDED.resend_after_seconds, updated_at=EXCLUDED.updated_at",
                    params![id, body, starts_at, ends_at, resend_after_seconds, Utc::now().to_rfc3339()],
                )?;
            }
            None => {
                self.lock()?
                    .execute("DELETE FROM mailbox_auto_replies WHERE mailbox_id=$1", params![id])?;
            }
        }
        self.auto_reply(organization_id, mailbox_id)
    }

    /// The mailbox's messages received inside the reply's window that the
    /// responder has not decided on, oldest first.
    pub fn auto_reply_candidates(&self, reply: &AutoReply) -> Result<Vec<AutoReplyCandidate>, AppError> {
        let client = self.lock()?;
        let mut statement = client.prepare(
            "SELECT messages.id AS id, messages.sender AS sender, messages.reply_to AS reply_to
             FROM messages
             LEFT JOIN auto_reply_decisions ON auto_reply_decisions.message_id=messages.id
             WHERE messages.mailbox_id=$1 AND auto_reply_decisions.message_id IS NULL
               AND messages.received_at::timestamptz >= $2::timestamptz
               AND ($3::text IS NULL OR messages.received_at::timestamptz < $3::timestamptz)
             ORDER BY messages.received_at, messages.id",
        )?;
        let rows = statement.query_map(
            params![reply.mailbox_id.to_string(), reply.starts_at, reply.ends_at],
            |row| {
                Ok(AutoReplyCandidate {
                    message_id: uuid(row.get("id")?)?,
                    sender: row.get("sender")?,
                    reply_to: row.get("reply_to")?,
                })
            },
        )?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Claim the decision on one message before anything is sent; false when
    /// another pass already claimed it.
    pub fn claim_auto_reply(&self, mailbox_id: Uuid, message_id: Uuid, sender: &str) -> Result<bool, AppError> {
        let claimed = self
            .lock()?
            .query_row(
                "INSERT INTO auto_reply_decisions (message_id, mailbox_id, sender, decided_at)
                 VALUES ($1, $2, $3, $4) ON CONFLICT (message_id) DO NOTHING
                 RETURNING message_id",
                params![message_id.to_string(), mailbox_id.to_string(), sender, Utc::now().to_rfc3339()],
                |row| row.get::<_, String>("message_id"),
            )
            .optional()?;
        Ok(claimed.is_some())
    }

    /// When the responder last answered `sender` from the mailbox at or after
    /// `since`, if it did.
    pub fn answered_since(&self, mailbox_id: Uuid, sender: &str, since: &str) -> Result<Option<String>, AppError> {
        self.lock()?
            .query_row(
                "SELECT decided_at FROM auto_reply_decisions
                 WHERE mailbox_id=$1 AND sender=$2 AND reply_attempt_id IS NOT NULL
                   AND decided_at::timestamptz >= $3::timestamptz
                 ORDER BY decided_at DESC LIMIT 1",
                params![mailbox_id.to_string(), sender, since],
                |row| row.get::<_, String>("decided_at"),
            )
            .optional()
            .map_err(Into::into)
    }

    /// Record the decision: the reply attempt sent, or why none was.
    pub fn record_auto_reply(
        &self,
        message_id: Uuid,
        reply_attempt_id: Option<Uuid>,
        skipped: Option<&str>,
    ) -> Result<(), AppError> {
        self.lock()?.execute(
            "UPDATE auto_reply_decisions SET reply_attempt_id=$2, skipped=$3, decided_at=$4 WHERE message_id=$1",
            params![
                message_id.to_string(),
                reply_attempt_id.map(|id| id.to_string()),
                skipped,
                Utc::now().to_rfc3339()
            ],
        )?;
        Ok(())
    }

    /// Every decision the responder made for the mailbox, newest first.
    pub fn auto_reply_decisions(
        &self,
        organization_id: &str,
        mailbox_id: Uuid,
    ) -> Result<Vec<AutoReplyDecision>, AppError> {
        self.get_mailbox(organization_id, mailbox_id)?;
        let client = self.lock()?;
        let mut statement = client.prepare(
            "SELECT message_id, sender, reply_attempt_id, skipped, decided_at
             FROM auto_reply_decisions WHERE mailbox_id=$1
             ORDER BY decided_at DESC, message_id",
        )?;
        let rows = statement.query_map(params![mailbox_id.to_string()], |row| {
            Ok(AutoReplyDecision {
                message_id: uuid(row.get("message_id")?)?,
                sender: row.get("sender")?,
                reply_attempt_id: match row.get::<_, Option<String>>("reply_attempt_id")? {
                    Some(id) => Some(uuid(id)?),
                    None => None,
                },
                skipped: row.get("skipped")?,
                decided_at: row.get("decided_at")?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Every mailbox's automatic reply, by mailbox.
    pub fn auto_replies(&self) -> Result<Vec<AutoReply>, AppError> {
        let client = self.lock()?;
        let mut statement = client.prepare(
            "SELECT mailbox_id, body, starts_at, ends_at, resend_after_seconds, updated_at
             FROM mailbox_auto_replies ORDER BY mailbox_id",
        )?;
        let rows = statement.query_map(params![], auto_reply_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// Subscribe to announcements that mail was received or an automatic
    /// reply changed (`skrzynka_announce_received()` in `inbound.sql`).
    pub fn subscribe_received(&self) -> Result<Subscription, AppError> {
        self.lock()?
            .run(|connection| async move {
                notifications::subscribe(&connection, RECEIVED_CHANNEL.to_string()).await
            })?
            .map_err(received_unavailable)
    }

    /// Wait on `subscription` for the next announcement and hand it back; an
    /// ended subscription answers the cause it ended on.
    pub fn next_received(&self, subscription: Subscription) -> Result<Subscription, AppError> {
        let (subscription, heard) = self.lock()?.run(move |_connection| async move {
            let mut subscription = subscription;
            let heard = subscription.next().await;
            (subscription, heard)
        })?;
        heard.map(|_| subscription).map_err(received_unavailable)
    }
}

/// The channel `skrzynka_announce_received()` notifies.
const RECEIVED_CHANNEL: &str = "skrzynka_received";

fn received_unavailable(error: stado_database::Error) -> AppError {
    AppError::new(
        axum::http::StatusCode::SERVICE_UNAVAILABLE,
        "RECEIVED_ANNOUNCEMENTS_UNAVAILABLE",
        format!("received-mail announcements on the fleet database channel {RECEIVED_CHANNEL}: {error}"),
        true,
    )
}
