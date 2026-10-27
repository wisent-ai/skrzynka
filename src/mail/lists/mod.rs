//! A mailing list's own way out: the addresses its List-Unsubscribe header
//! (RFC 2369) names, which of them take a one-click request (RFC 8058), and
//! the request itself, made over HTTPS or as mail from the receiving mailbox.

use super::{deliver, dependency_error};
use crate::{error::AppError, models::Mailbox, skarbiec::ResolvedCredentials};
use lettre::{
    message::{header::ContentType, Mailbox as LettreMailbox},
    Message as OutgoingMessage,
};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use uuid::Uuid;

mod attempt;
pub use attempt::{
    Subscription, Subscriptions, UnsubscribeAttempt, UnsubscribeMethod, UnsubscribeRequest,
};

/// The List-Unsubscribe-Post value that makes an HTTPS address one-click.
const ONE_CLICK: &str = "List-Unsubscribe=One-Click";
/// RFC 8058 allows one-click only over HTTPS.
const ONE_CLICK_SCHEME: &str = "https";
/// RFC 6068: an address to mail.
const MAILTO_SCHEME: &str = "mailto";

/// The mailing-list headers a message states, as it writes them: its List-Id
/// (RFC 2919), List-Unsubscribe (RFC 2369) and List-Unsubscribe-Post
/// (RFC 8058). Each is absent when the message states none.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailingList {
    pub list_id: Option<String>,
    pub unsubscribe: Option<String>,
    pub unsubscribe_post: Option<String>,
}

impl MailingList {
    /// The list headers of a parsed message.
    pub(in crate::mail) fn of(headers: &[mailparse::MailHeader<'_>]) -> Self {
        use mailparse::MailHeaderMap;
        Self {
            list_id: headers.get_first_value("List-Id"),
            unsubscribe: headers.get_first_value("List-Unsubscribe"),
            unsubscribe_post: headers.get_first_value("List-Unsubscribe-Post"),
        }
    }

    /// Every way out this list names; a message that names none is refused.
    pub fn targets(&self) -> Result<Vec<UnsubscribeTarget>, String> {
        match self.unsubscribe.as_deref() {
            Some(header) => unsubscribe_targets(header, self.unsubscribe_post.as_deref()),
            None => Err("the message states no List-Unsubscribe header".to_string()),
        }
    }
}

/// One way a list can be left, as its message names it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum UnsubscribeTarget {
    /// An HTTPS address the list asks to receive `List-Unsubscribe=One-Click` at.
    OneClick { url: String },
    /// An address to mail, with the subject and body the list asks for.
    Mailto {
        address: String,
        subject: Option<String>,
        body: Option<String>,
    },
    /// Any other address: a page a person opens; Skrzynka opens none.
    Page { url: String },
}

/// Every way out the List-Unsubscribe `header` names, in its order. `post` is
/// the message's List-Unsubscribe-Post header: only with
/// `List-Unsubscribe=One-Click` is an HTTPS address a one-click address. An
/// entry that is not a URI in angle brackets is refused with the entry and why.
pub fn unsubscribe_targets(
    header: &str,
    post: Option<&str>,
) -> Result<Vec<UnsubscribeTarget>, String> {
    let one_click = post.is_some_and(|value| value.trim().eq_ignore_ascii_case(ONE_CLICK));
    let mut targets = Vec::new();
    let mut rest = header;
    while let Some(open) = rest.find('<') {
        let after = &rest[open..];
        let close = after
            .find('>')
            .ok_or_else(|| format!("{after:?} opens an address that is never closed"))?;
        let uri = after[..close].trim_start_matches('<').trim();
        targets.push(target(uri, one_click)?);
        rest = &after[close..];
    }
    if targets.is_empty() {
        return Err(format!("{header:?} names no address in angle brackets"));
    }
    Ok(targets)
}

fn target(uri: &str, one_click: bool) -> Result<UnsubscribeTarget, String> {
    let url = reqwest::Url::parse(uri).map_err(|error| format!("{uri:?} is not a URI: {error}"))?;
    if url.scheme() == MAILTO_SCHEME {
        let mut subject = None;
        let mut body = None;
        for (key, value) in url.query_pairs() {
            if key.eq_ignore_ascii_case("subject") {
                subject = Some(value.into_owned());
            } else if key.eq_ignore_ascii_case("body") {
                body = Some(value.into_owned());
            }
        }
        return Ok(UnsubscribeTarget::Mailto {
            address: url.path().to_string(),
            subject,
            body,
        });
    }
    if one_click && url.scheme() == ONE_CLICK_SCHEME {
        return Ok(UnsubscribeTarget::OneClick {
            url: url.to_string(),
        });
    }
    Ok(UnsubscribeTarget::Page {
        url: url.to_string(),
    })
}

/// RFC 8058's request: a POST of `List-Unsubscribe=One-Click` to `url`. Any
/// 2xx answer is the list accepting it; another status, or no answer, is
/// refused with what was observed.
pub async fn post_one_click(client: &reqwest::Client, url: &str) -> Result<u16, AppError> {
    let response = stado_wait::request(
        client
            .post(url)
            .header(
                reqwest::header::CONTENT_TYPE,
                "application/x-www-form-urlencoded",
            )
            .body(ONE_CLICK),
    )
    .await
    .map_err(|error| {
        dependency_error(
            "UNSUBSCRIBE_UNREACHABLE",
            format!("one-click unsubscribe POST to {url} got no answer: {error}"),
            true,
        )
    })?;
    let status = response.status();
    if !status.is_success() {
        return Err(dependency_error(
            "UNSUBSCRIBE_REFUSED",
            format!("one-click unsubscribe POST to {url} was answered HTTP {status}"),
            status.is_server_error(),
        ));
    }
    Ok(status.as_u16())
}

/// Mail the list's unsubscribe `address` from `mailbox`, with the subject and
/// body the list's own address asks for; a list that names neither gets a
/// message with neither. Returns the Message-ID the mail went out with.
pub fn send_unsubscribe_mail(
    mailbox: &Mailbox,
    credentials: &ResolvedCredentials,
    address: &str,
    subject: Option<&str>,
    body: Option<&str>,
) -> Result<String, AppError> {
    let from_address = mailbox.email.parse().map_err(|cause| {
        AppError::invalid(
            "MAILBOX_PROFILE_INVALID",
            format!(
                "mailbox sending address {:?} is invalid: {cause}",
                mailbox.email
            ),
        )
    })?;
    let to = LettreMailbox::from_str(address).map_err(|cause| {
        AppError::invalid(
            "UNSUBSCRIBE_ADDRESS_INVALID",
            format!("the list's unsubscribe address {address:?} is not a valid address: {cause}"),
        )
    })?;
    let message_id = format!("<{}@skrzynka.local>", Uuid::new_v4());
    let mut builder = OutgoingMessage::builder()
        .from(LettreMailbox::new(
            Some(mailbox.display_name.clone()),
            from_address,
        ))
        .to(to)
        .message_id(Some(message_id.clone()))
        .header(ContentType::TEXT_PLAIN);
    if let Some(subject) = subject {
        builder = builder.subject(subject);
    }
    let text = match body {
        Some(text) => text.to_string(),
        None => String::new(),
    };
    let outgoing = builder.body(text).map_err(|cause| {
        AppError::invalid(
            "UNSUBSCRIBE_MESSAGE_INVALID",
            format!("the unsubscribe mail to {address} could not be encoded: {cause}"),
        )
    })?;
    deliver(mailbox, credentials, &outgoing)?;
    Ok(message_id)
}
