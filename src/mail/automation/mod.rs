//! Mail written by machines, as RFC 3834 describes it: the `Auto-Submitted`
//! header an automatic reply carries, and what a received message's own
//! headers say about whether a machine sent it, so a responder never answers
//! one (and two responders never answer each other).

use lettre::message::header::{Header, HeaderName, HeaderValue};
use mailparse::MailHeaderMap;
use serde::Serialize;
use std::error::Error;
use uuid::Uuid;

/// A mailbox's automatic reply (docs/replies/auto-reply.md): the text it
/// answers with, the window of received mail it answers, and how long one
/// sender waits before being answered again.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AutoReply {
    pub mailbox_id: Uuid,
    pub body: String,
    pub starts_at: String,
    pub ends_at: Option<String>,
    pub resend_after_seconds: i64,
    pub updated_at: String,
}

/// What the responder decided for one received message: the reply attempt
/// it sent (whose delivery state `message replies` reads), or why it sent
/// none.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AutoReplyDecision {
    pub message_id: Uuid,
    pub sender: String,
    pub reply_attempt_id: Option<Uuid>,
    pub skipped: Option<String>,
    pub decided_at: String,
}

/// The `Auto-Submitted` header (RFC 3834 section 5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoSubmitted(String);

impl AutoSubmitted {
    /// What an automatic reply states: `auto-replied`.
    pub fn replied() -> Self {
        Self("auto-replied".to_string())
    }
}

impl Header for AutoSubmitted {
    fn name() -> HeaderName {
        HeaderName::new_from_ascii_str("Auto-Submitted")
    }

    fn parse(value: &str) -> Result<Self, Box<dyn Error + Send + Sync>> {
        Ok(Self(value.trim().to_string()))
    }

    fn display(&self) -> HeaderValue {
        HeaderValue::new(Self::name(), self.0.clone())
    }
}

/// Why a machine, not a person, sent a message, as its headers say; `None`
/// when they say nothing of the kind. RFC 3834 section 2: a responder answers
/// only mail whose `Auto-Submitted` is absent or `no`. A `Precedence` header
/// of any value, a `List-Id`, or a delivery status notification
/// (`multipart/report`) marks mail a person did not write either.
pub fn machine_sent(source: &[u8]) -> Result<Option<String>, String> {
    let parsed = mailparse::parse_mail(source)
        .map_err(|error| format!("the message's headers do not read as RFC 5322: {error}"))?;
    let headers = parsed.get_headers();
    if let Some(value) = headers.get_first_value("Auto-Submitted") {
        if !value.trim().eq_ignore_ascii_case("no") {
            return Ok(Some(format!("it states Auto-Submitted: {}", value.trim())));
        }
    }
    if let Some(value) = headers.get_first_value("Precedence") {
        return Ok(Some(format!("it states Precedence: {}", value.trim())));
    }
    if let Some(value) = headers.get_first_value("List-Id") {
        return Ok(Some(format!(
            "it came through the mailing list {}",
            value.trim()
        )));
    }
    if parsed
        .ctype
        .mimetype
        .eq_ignore_ascii_case("multipart/report")
    {
        return Ok(Some(
            "it is a delivery report (multipart/report)".to_string(),
        ));
    }
    Ok(None)
}
