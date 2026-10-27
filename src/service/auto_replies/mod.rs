//! A mailbox's automatic reply (docs/replies/auto-reply.md): the text it
//! answers received mail with between two instants, at most once per sender
//! within a span, as an RFC 3834 auto-reply. The responder of a running
//! service answers each message once, as soon as an import writes it; it
//! never answers mail from the mailbox's own address, one sender twice within
//! the span, or mail whose headers say a machine sent it. Every decision is
//! recorded with the reply attempt sent or why none was.

use super::snoozes::{instant, span_from_now};
use super::AppState;
use crate::{
    error::AppError,
    mail::{self, AutoReply, AutoReplyDecision},
    models::CreateReplyRequest,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use uuid::Uuid;

/// `PUT /v1/mailboxes/:id/auto-reply`: `body`, `every` (a span such as `7d`
/// between answers to one sender), and `from`/`until` (RFC 3339 or a span
/// from now such as `+2h`); `from` absent is now, `until` absent is open.
#[derive(Debug, Deserialize)]
pub struct AutoReplyRequest {
    pub body: String,
    pub every: String,
    #[serde(default)]
    pub from: Option<String>,
    #[serde(default)]
    pub until: Option<String>,
}

/// What `mailbox responder` answers.
#[derive(Debug, Serialize)]
pub struct MailboxAutoReply {
    pub mailbox_id: Uuid,
    pub auto_reply: Option<AutoReply>,
}

const INVALID: &str = "AUTO_REPLY_INVALID";

/// An instant written as RFC 3339 or as a span from now (`+2h`), stored in
/// the fixed width snoozes use.
fn when(written: &str, field: &str) -> Result<DateTime<Utc>, AppError> {
    let written = written.trim();
    match written.strip_prefix('+') {
        Some(span) => Utc::now()
            .checked_add_signed(span_from_now(span, '+', INVALID)?)
            .ok_or_else(|| AppError::invalid(INVALID, format!("{field} +{span} reaches past the last instant Skrzynka can store"))),
        None => DateTime::parse_from_rfc3339(written)
            .map(|at| at.with_timezone(&Utc))
            .map_err(|error| {
                AppError::invalid(
                    INVALID,
                    format!("{field} {written:?} is neither an RFC 3339 instant ({error}) nor a span from now such as +2h"),
                )
            }),
    }
}

/// The address a reply to the message goes to, bare and in lower case.
fn reply_address(sender: &str, reply_to: Option<&str>) -> String {
    let target = match reply_to {
        Some(reply_to) => reply_to,
        None => sender,
    };
    match lettre::message::Mailbox::from_str(target) {
        Ok(mailbox) => mailbox.email.to_string().to_lowercase(),
        Err(_unreadable) => target.trim().to_lowercase(),
    }
}

impl AppState {
    pub fn mailbox_auto_reply(&self, organization_id: &str, mailbox_id: Uuid) -> Result<MailboxAutoReply, AppError> {
        Ok(MailboxAutoReply {
            mailbox_id,
            auto_reply: self.database.auto_reply(organization_id, mailbox_id)?,
        })
    }

    /// Set the automatic reply, or remove it with `None`. A body with no
    /// visible text, a span between answers that is not ahead of now, and an
    /// end not after the start are refused before anything is stored.
    pub fn set_mailbox_auto_reply(
        &self,
        organization_id: &str,
        mailbox_id: Uuid,
        request: Option<AutoReplyRequest>,
    ) -> Result<MailboxAutoReply, AppError> {
        let Some(request) = request else {
            return Ok(MailboxAutoReply {
                mailbox_id,
                auto_reply: self.database.set_auto_reply(organization_id, mailbox_id, None)?,
            });
        };
        let body = request.body.trim_end();
        if body.trim().is_empty() {
            return Err(AppError::invalid(INVALID, "the automatic reply holds no visible text; write one, or remove the automatic reply"));
        }
        let every = span_from_now(request.every.trim().trim_start_matches('+'), '+', INVALID)?;
        if every <= chrono::TimeDelta::zero() {
            return Err(AppError::invalid(INVALID, format!("every {} is no span: answering one sender again needs a time that passes", request.every.trim())));
        }
        let starts = match request.from.as_deref() {
            Some(from) => when(from, "from")?,
            None => Utc::now(),
        };
        let ends = match request.until.as_deref() {
            Some(until) => Some(when(until, "until")?),
            None => None,
        };
        if let Some(ends) = ends {
            if ends <= starts {
                return Err(AppError::invalid(INVALID, format!("until {} is not after from {}; the reply would answer nothing", instant(ends), instant(starts))));
            }
        }
        let ends = ends.map(instant);
        let stored = self.database.set_auto_reply(
            organization_id,
            mailbox_id,
            Some((body, &instant(starts), ends.as_deref(), every.num_seconds())),
        )?;
        Ok(MailboxAutoReply { mailbox_id, auto_reply: stored })
    }

    pub fn auto_reply_decisions(&self, organization_id: &str, mailbox_id: Uuid) -> Result<Vec<AutoReplyDecision>, AppError> {
        self.database.auto_reply_decisions(organization_id, mailbox_id)
    }

    /// Answer every message the responder has not decided on, of every
    /// mailbox with an automatic reply (of one organization when named);
    /// answers the decisions it made.
    pub async fn answer_automatically(&self, organization_id: Option<&str>) -> Result<Vec<AutoReplyDecision>, AppError> {
        let mut decided = Vec::new();
        for reply in self.database.auto_replies()? {
            let mailbox = self.database.get_mailbox_internal(reply.mailbox_id)?;
            if organization_id.is_some_and(|organization| organization != mailbox.organization_id) {
                continue;
            }
            for candidate in self.database.auto_reply_candidates(&reply)? {
                let address = reply_address(&candidate.sender, candidate.reply_to.as_deref());
                if !self.database.claim_auto_reply(mailbox.id, candidate.message_id, &address)? {
                    continue;
                }
                let skipped = self
                    .why_not_answer(&mailbox.organization_id, &mailbox.email, &reply, candidate.message_id, &address)
                    .await?;
                let attempt = match skipped {
                    Some(_) => None,
                    None => Some(
                        self.reply(
                            &mailbox.organization_id,
                            candidate.message_id,
                            CreateReplyRequest {
                                idempotency_key: format!("auto-reply:{}", candidate.message_id),
                                body: reply.body.clone(),
                                attachments: Vec::new(),
                                reply_all: false,
                                quote: false,
                                cc: Vec::new(),
                                bcc: Vec::new(),
                                automatic: true,
                            },
                        )
                        .await,
                    ),
                };
                let (attempt_id, why) = match (attempt, skipped) {
                    (Some(Ok(attempt)), _) => (Some(attempt.id), None),
                    (Some(Err(error)), _) => (None, Some(format!("the reply was refused: {}: {}", error.code, error.message))),
                    (None, why) => (None, why),
                };
                self.database.record_auto_reply(candidate.message_id, attempt_id, why.as_deref())?;
                decided.push(AutoReplyDecision {
                    message_id: candidate.message_id,
                    sender: address,
                    reply_attempt_id: attempt_id,
                    skipped: why,
                    decided_at: Utc::now().to_rfc3339(),
                });
            }
        }
        Ok(decided)
    }

    /// Why the responder answers nothing to the message, or `None` to answer.
    async fn why_not_answer(
        &self,
        organization_id: &str,
        own: &str,
        reply: &AutoReply,
        message_id: Uuid,
        address: &str,
    ) -> Result<Option<String>, AppError> {
        if address == own.trim().to_lowercase() {
            return Ok(Some("it was sent from this mailbox's own address".to_string()));
        }
        let span = chrono::TimeDelta::try_seconds(reply.resend_after_seconds).ok_or_else(|| {
            AppError::internal(format!("the stored span of {} seconds between answers is not a span", reply.resend_after_seconds))
        })?;
        let since = Utc::now().checked_sub_signed(span).ok_or_else(|| {
            AppError::internal(format!("{} seconds back from now is before the first instant Skrzynka can store", reply.resend_after_seconds))
        })?;
        if let Some(answered) = self.database.answered_since(reply.mailbox_id, address, &instant(since))? {
            return Ok(Some(format!(
                "{address} was answered automatically at {answered}, within the {} seconds between answers",
                reply.resend_after_seconds
            )));
        }
        Ok(match self.message_source(organization_id, message_id).await {
            Err(error) => Some(format!("its headers could not be read from the provider: {}: {}", error.code, error.message)),
            Ok(source) => match mail::machine_sent(&source) {
                Err(why) => Some(why),
                Ok(Some(why)) => Some(format!("a machine sent it: {why}")),
                Ok(None) => None,
            },
        })
    }

    /// Answer received mail for as long as the service runs: once at start,
    /// and again whenever any process imports mail or changes an automatic
    /// reply. A pass that fails is logged with its cause and the next
    /// announcement runs the next one.
    pub(in crate::service) fn start_answering(&self) -> Result<(), AppError> {
        let subscription = self.database.subscribe_received()?;
        let state = self.clone();
        tokio::spawn(async move {
            let mut subscription = subscription;
            loop {
                if let Err(error) = state.answer_automatically(None).await {
                    tracing::error!(code = error.code, message = %error.message, "received mail could not be answered automatically");
                }
                let database = state.database.clone();
                let waited = tokio::task::spawn_blocking(move || database.next_received(subscription)).await;
                subscription = match waited {
                    Ok(Ok(subscription)) => subscription,
                    Ok(Err(ended)) => match state.database.subscribe_received() {
                        Ok(subscription) => {
                            tracing::warn!(code = ended.code, message = %ended.message, "received-mail announcements stopped; subscribed again");
                            subscription
                        }
                        Err(error) => {
                            tracing::error!(code = error.code, message = %error.message, "received-mail announcements cannot be received; mail is not answered automatically until this service starts again");
                            return;
                        }
                    },
                    Err(stopped) => {
                        tracing::error!(error = %stopped, "the automatic responder stopped; mail is not answered automatically until this service starts again");
                        return;
                    }
                };
            }
        });
        Ok(())
    }
}
