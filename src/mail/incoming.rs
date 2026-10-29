use super::password::password_login_refusal;
use super::{dependency_error, normalize_message};
use crate::{
    error::AppError,
    models::{Mailbox, NewMessage},
    skarbiec::ResolvedCredentials,
};
use std::{collections::BTreeMap, num::NonZeroU32};

/// Gmail's own IMAP endpoint: the only host the Gmail connection paths speak
/// for, and the boundary that decides whether a refusal is Google's.
pub const GMAIL_IMAP_HOST: &str = "imap.gmail.com";

/// What one pass read from the provider besides the messages it handed to
/// `commit`: what it could not import and why, and the cursor it reached.
#[derive(Debug)]
pub struct FetchedMessages {
    pub skipped: usize,
    pub rejected_by_reason: BTreeMap<String, usize>,
    pub last_uid: u32,
}

struct OAuth2Authenticator<'a> {
    username: &'a str,
    access_token: &'a str,
}

impl imap::Authenticator for OAuth2Authenticator<'_> {
    type Response = String;

    fn process(&self, _: &[u8]) -> Self::Response {
        format!(
            "user={}\u{1}auth=Bearer {}\u{1}\u{1}",
            self.username, self.access_token
        )
    }
}

/// Block until the provider reports new mail in INBOX (IMAP IDLE, RFC 2177).
/// The connection re-issues IDLE on the RFC's own refresh boundary by itself;
/// no interval is chosen here. Returns when an `EXISTS` arrives, or with the
/// error that ended the connection.
pub fn wait_for_new_mail(
    mailbox: &Mailbox,
    credentials: &ResolvedCredentials,
) -> Result<(), AppError> {
    let mut session = open_inbox(mailbox, credentials)?;
    session
        .idle()
        .wait_while(|response| !matches!(response, imap::types::UnsolicitedResponse::Exists(_)))
        .map_err(|error| {
            dependency_error(
                "IMAP_IDLE_FAILED",
                format!(
                    "IMAP IDLE on INBOX for {} at {}:{} after UID {} ended: {error:?}",
                    mailbox.email, mailbox.imap_host, mailbox.imap_port, mailbox.last_uid
                ),
                true,
            )
        })?;
    Ok(())
}

/// Connect over TLS, authenticate with the mailbox's credentials and select INBOX.
pub(super) fn open_inbox(
    mailbox: &Mailbox,
    credentials: &ResolvedCredentials,
) -> Result<imap::Session<imap::Connection>, AppError> {
    let client = imap::ClientBuilder::new(mailbox.imap_host.as_str(), mailbox.imap_port)
        .mode(imap::ConnectionMode::Tls)
        .tls_kind(imap::TlsKind::Native)
        .connect()
        .map_err(|error| {
            dependency_error(
                "IMAP_UNAVAILABLE",
                format!(
                    "IMAP TLS connect to {}:{} failed: {error:?}",
                    mailbox.imap_host, mailbox.imap_port
                ),
                true,
            )
        })?;
    let mut session = match credentials {
        ResolvedCredentials::Password { username, password } => {
            client.login(username, password).map_err(|(error, _)| {
                password_login_refusal(error, &mailbox.imap_host, password).into_error(
                    &mailbox.organization_id,
                    &mailbox.email,
                    &mailbox.skarbiec_item_id,
                )
            })?
        }
        ResolvedCredentials::OAuth2 {
            username,
            access_token,
        } => client
            .authenticate(
                "XOAUTH2",
                &OAuth2Authenticator {
                    username,
                    access_token,
                },
            )
            .map_err(|(error, _)| {
                let detail = format!("{error:?}");
                let detail = if access_token.is_empty() {
                    detail
                } else {
                    detail.replace(access_token, "[redacted]")
                };
                dependency_error(
                    "IMAP_AUTHENTICATION_FAILED",
                    format!(
                        "IMAP XOAUTH2 at {}:{} failed: {detail}",
                        mailbox.imap_host, mailbox.imap_port
                    ),
                    matches!(&error, imap::Error::Io(_) | imap::Error::ConnectionLost),
                )
            })?,
    };
    session.select("INBOX").map_err(|error| {
        dependency_error(
            "IMAP_INBOX_UNAVAILABLE",
            format!(
                "IMAP SELECT INBOX at {}:{} failed: {error:?}",
                mailbox.imap_host, mailbox.imap_port
            ),
            true,
        )
    })?;
    Ok(session)
}

/// Read every message the provider holds past the mailbox cursor, oldest UID
/// first, and hand each to `commit` with the cursor it reaches. A pass imports
/// everything there is, one message at a time: no batch size decides how much
/// mail arrives, only one message is held at once, and a failure part way
/// keeps what was committed before it, so the next pass continues from there.
/// A UID that yields no importable message is committed as an empty slice, so
/// the cursor still passes it.
pub fn fetch_messages(
    mailbox: &Mailbox,
    credentials: &ResolvedCredentials,
    mut commit: impl FnMut(&[NewMessage], u32) -> Result<(), AppError>,
) -> Result<FetchedMessages, AppError> {
    let mut session = open_inbox(mailbox, credentials)?;

    // No cursor yet reads every message; `last:*` holds the cursor itself, and
    // the newest message when nothing is newer (RFC 3501, 6.4.8), both of
    // which the filter below drops.
    let query = match NonZeroU32::new(mailbox.last_uid) {
        None => "ALL".to_string(),
        Some(last) => format!("UID {last}:*"),
    };
    let mut uids = session
        .uid_search(&query)
        .map_err(|error| {
            dependency_error(
                "IMAP_SEARCH_FAILED",
                format!(
                    "IMAP UID SEARCH {query} at {}:{} failed: {error:?}",
                    mailbox.imap_host, mailbox.imap_port
                ),
                true,
            )
        })?
        .into_iter()
        .filter(|uid| *uid > mailbox.last_uid)
        .collect::<Vec<_>>();
    // SEARCH returns a HashSet. Advance through UIDs in order, so a pass that
    // stops part way never leaves an older unread row behind its cursor.
    uids.sort_unstable();

    // The UIDs each reason kept out; the counts the caller reports are read
    // from them.
    let mut rejected: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    let mut last_uid = mailbox.last_uid;
    for requested_uid in uids {
        let fetches = session
            .uid_fetch(requested_uid.to_string(), "(UID BODY.PEEK[])")
            .map_err(|error| {
                dependency_error(
                    "IMAP_FETCH_FAILED",
                    format!(
                        "IMAP fetch at UID {requested_uid} on {}:{} failed: {error:?}",
                        mailbox.imap_host, mailbox.imap_port
                    ),
                    true,
                )
            })?;
        let Some(fetch) = fetches
            .iter()
            .find(|fetch| fetch.uid == Some(requested_uid))
        else {
            if !fetches.is_empty() {
                let observed = fetches.iter().map(|fetch| fetch.uid).collect::<Vec<_>>();
                return Err(dependency_error(
                    "IMAP_FETCH_UID_MISMATCH",
                    format!("IMAP fetch at UID {requested_uid} on {}:{} returned rows with UIDs {observed:?}, none matching the requested message; cursor not advanced", mailbox.imap_host, mailbox.imap_port),
                    true,
                ));
            }
            rejected
                .entry("provider_row_missing".to_string())
                .or_default()
                .push(requested_uid);
            last_uid = last_uid.max(requested_uid);
            commit(&[], last_uid)?;
            continue;
        };
        let uid = requested_uid;
        last_uid = requested_uid;
        let Some(body) = fetch.body() else {
            rejected
                .entry("message_body_missing".to_string())
                .or_default()
                .push(uid);
            commit(&[], last_uid)?;
            continue;
        };
        match normalize_message(uid, body) {
            Ok(message) => commit(std::slice::from_ref(&message), last_uid)?,
            Err(error) => {
                rejected
                    .entry(error.code.to_ascii_lowercase())
                    .or_default()
                    .push(uid);
                commit(&[], last_uid)?;
            }
        }
    }
    // Every fetched message is committed; a LOGOUT the server does not answer
    // cleanly changes none of that and is said, not dropped.
    if let Err(error) = session.logout() {
        tracing::warn!(%error, host = %mailbox.imap_host, "IMAP LOGOUT after a fetch failed");
    }
    let rejected_by_reason: BTreeMap<String, usize> = rejected
        .into_iter()
        .map(|(reason, uids)| (reason, uids.len()))
        .collect();
    Ok(FetchedMessages {
        skipped: rejected_by_reason.values().sum(),
        rejected_by_reason,
        last_uid,
    })
}

/// The provider's own copy of one INBOX message, read by UID with
/// `BODY.PEEK[]` so it is not marked seen; `None` when INBOX no longer holds
/// that UID (the message was deleted, archived or moved at the provider).
pub fn fetch_message_source(
    mailbox: &Mailbox,
    credentials: &ResolvedCredentials,
    uid: u32,
) -> Result<Option<Vec<u8>>, AppError> {
    let mut session = open_inbox(mailbox, credentials)?;
    let source = {
        let fetches = session
            .uid_fetch(uid.to_string(), "(UID BODY.PEEK[])")
            .map_err(|error| {
                dependency_error(
                    "IMAP_FETCH_FAILED",
                    format!(
                        "IMAP fetch at UID {uid} on {}:{} failed: {error:?}",
                        mailbox.imap_host, mailbox.imap_port
                    ),
                    true,
                )
            })?;
        match fetches.iter().find(|fetch| fetch.uid == Some(uid)) {
            None => None,
            Some(fetch) => match fetch.body() {
                Some(body) => Some(body.to_vec()),
                None => {
                    return Err(dependency_error(
                        "IMAP_FETCH_FAILED",
                        format!(
                            "IMAP fetch at UID {uid} on {}:{} answered the message \
                             without its body",
                            mailbox.imap_host, mailbox.imap_port
                        ),
                        true,
                    ));
                }
            },
        }
    };
    // The message is read; a LOGOUT the server does not answer cleanly
    // changes none of that and is said, not dropped.
    if let Err(error) = session.logout() {
        tracing::warn!(%error, host = %mailbox.imap_host, "IMAP LOGOUT after reading one message failed");
    }
    Ok(source)
}
