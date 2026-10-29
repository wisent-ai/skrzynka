//! What Skrzynka changes about one INBOX message at its provider: whether it
//! is seen or flagged, which folder holds it, and whether the provider keeps it. Each
//! action runs in one IMAP session that first reads the provider's copy by
//! UID and proves it is the message Skrzynka stored, then acts, then reads
//! the provider's state again to observe the effect.

use super::{dependency_error, incoming::open_inbox, read_message};
use crate::{
    error::AppError,
    models::{Mailbox, Message},
    skarbiec::ResolvedCredentials,
};
use axum::http::StatusCode;
use imap::types::Flag;

mod model;
pub use model::{
    ActionStatus, MarkMessageRequest, MarkState, MessageAction, MoveMessageRequest, ProviderAction,
    SpecialUse,
};

type Session = imap::Session<imap::Connection>;

/// Apply `action` to the provider's copy of `stored` and observe its effect.
pub fn act_on_message(
    mailbox: &Mailbox,
    credentials: &ResolvedCredentials,
    stored: &Message,
    action: &ProviderAction,
) -> Result<(), AppError> {
    let mut session = open_inbox(mailbox, credentials)?;
    let outcome = act(&mut session, mailbox, stored, action);
    // The action's outcome is decided; a LOGOUT the server does not answer
    // cleanly changes none of that and is said, not dropped.
    if let Err(error) = session.logout() {
        tracing::warn!(%error, host = %mailbox.imap_host, "IMAP LOGOUT after a message action failed");
    }
    outcome
}

fn act(
    session: &mut Session,
    mailbox: &Mailbox,
    stored: &Message,
    action: &ProviderAction,
) -> Result<(), AppError> {
    let uid = stored.external_uid;
    let Some(source) =
        fetch(session, mailbox, uid, "(UID BODY.PEEK[])")?.and_then(|fetched| fetched.body)
    else {
        return Err(AppError::new(
            StatusCode::GONE,
            "MESSAGE_SOURCE_GONE",
            format!(
                "INBOX of {} at {}:{} no longer holds UID {uid}, where message {} was imported from: the provider deleted, archived or moved it, so there is nothing to {}",
                mailbox.email, mailbox.imap_host, mailbox.imap_port, stored.id, action.verb()
            ),
            false,
        ));
    };
    let (provider, _) = read_message(uid, &source)?;
    if !provider.is_stored_as(stored) {
        return Err(AppError::conflict(
            "MESSAGE_SOURCE_CHANGED",
            format!(
                "UID {uid} in INBOX of {} at {}:{} now holds a different message than message {} (stored Message-ID {:?}, provider Message-ID {:?}); nothing was changed at the provider",
                mailbox.email,
                mailbox.imap_host,
                mailbox.imap_port,
                stored.id,
                stored.message_id,
                provider.message_id
            ),
        ));
    }
    match action {
        ProviderAction::Seen => set_flag(session, mailbox, uid, MarkedFlag::Seen, true),
        ProviderAction::Unseen => set_flag(session, mailbox, uid, MarkedFlag::Seen, false),
        ProviderAction::Flagged => set_flag(session, mailbox, uid, MarkedFlag::Flagged, true),
        ProviderAction::Unflagged => set_flag(session, mailbox, uid, MarkedFlag::Flagged, false),
        ProviderAction::Move { folder } => move_to(session, mailbox, uid, folder),
        ProviderAction::Delete => delete(session, mailbox, uid),
    }
}

/// What one UID FETCH answered for the requested UID: its body when asked
/// for, and the flags it carries.
struct Fetched {
    body: Option<Vec<u8>>,
    flags: Vec<MarkedFlag>,
}

/// A system flag `message mark` sets or clears.
#[derive(Clone, Copy, PartialEq, Eq)]
enum MarkedFlag {
    Seen,
    Flagged,
}

impl MarkedFlag {
    /// The flag as IMAP writes it.
    fn imap(self) -> &'static str {
        match self {
            Self::Seen => "\\Seen",
            Self::Flagged => "\\Flagged",
        }
    }

    /// The state the message is in with and without the flag.
    fn state(self, carried: bool) -> &'static str {
        match (self, carried) {
            (Self::Seen, true) => "seen",
            (Self::Seen, false) => "unseen",
            (Self::Flagged, true) => "flagged",
            (Self::Flagged, false) => "unflagged",
        }
    }
}

fn fetch(
    session: &mut Session,
    mailbox: &Mailbox,
    uid: u32,
    query: &str,
) -> Result<Option<Fetched>, AppError> {
    let fetches = session
        .uid_fetch(uid.to_string(), query)
        .map_err(|error| command_failed(mailbox, &format!("UID FETCH {uid} {query}"), &error))?;
    Ok(fetches
        .iter()
        .find(|fetch| fetch.uid == Some(uid))
        .map(|fetch| Fetched {
            body: fetch.body().map(<[u8]>::to_vec),
            flags: fetch
                .flags()
                .iter()
                .filter_map(|flag| match flag {
                    Flag::Seen => Some(MarkedFlag::Seen),
                    Flag::Flagged => Some(MarkedFlag::Flagged),
                    _ => None,
                })
                .collect(),
        }))
}

/// Set or clear one flag, then read the flags again to observe the effect.
fn set_flag(
    session: &mut Session,
    mailbox: &Mailbox,
    uid: u32,
    flag: MarkedFlag,
    on: bool,
) -> Result<(), AppError> {
    let query = format!(
        "{}FLAGS.SILENT ({})",
        if on { "+" } else { "-" },
        flag.imap()
    );
    session
        .uid_store(uid.to_string(), &query)
        .map_err(|error| command_failed(mailbox, &format!("UID STORE {uid} {query}"), &error))?;
    match fetch(session, mailbox, uid, "(UID FLAGS)")? {
        Some(fetched) if fetched.flags.contains(&flag) == on => Ok(()),
        Some(_) => Err(unconfirmed(
            mailbox,
            uid,
            format!(
                "the provider accepted UID STORE {query} but still reports the message {}",
                flag.state(!on)
            ),
        )),
        None => Err(unconfirmed(
            mailbox,
            uid,
            "the provider accepted the flag change, then no longer listed the UID in INBOX"
                .to_string(),
        )),
    }
}

fn move_to(
    session: &mut Session,
    mailbox: &Mailbox,
    uid: u32,
    folder: &str,
) -> Result<(), AppError> {
    require_capability(session, mailbox, "MOVE", "RFC 6851")?;
    session
        .uid_mv(uid.to_string(), folder)
        .map_err(|error| command_failed(mailbox, &format!("UID MOVE {uid} {folder:?}"), &error))?;
    gone_from_inbox(session, mailbox, uid, &format!("moved to {folder:?}"))
}

/// Mark the message deleted and expunge that one UID. A server without
/// UIDPLUS can only expunge every message marked deleted in INBOX, including
/// ones other clients marked, so the action is refused there instead.
fn delete(session: &mut Session, mailbox: &Mailbox, uid: u32) -> Result<(), AppError> {
    require_capability(session, mailbox, "UIDPLUS", "RFC 4315")?;
    let query = "+FLAGS.SILENT (\\Deleted)";
    session
        .uid_store(uid.to_string(), query)
        .map_err(|error| command_failed(mailbox, &format!("UID STORE {uid} {query}"), &error))?;
    session
        .uid_expunge(uid.to_string())
        .map_err(|error| command_failed(mailbox, &format!("UID EXPUNGE {uid}"), &error))?;
    gone_from_inbox(session, mailbox, uid, "deleted")
}

fn gone_from_inbox(
    session: &mut Session,
    mailbox: &Mailbox,
    uid: u32,
    effect: &str,
) -> Result<(), AppError> {
    match fetch(session, mailbox, uid, "(UID FLAGS)")? {
        None => Ok(()),
        Some(_) => Err(unconfirmed(
            mailbox,
            uid,
            format!("the provider accepted the message being {effect}, yet INBOX still lists it"),
        )),
    }
}

fn require_capability(
    session: &mut Session,
    mailbox: &Mailbox,
    capability: &str,
    standard: &str,
) -> Result<(), AppError> {
    let capabilities = session
        .capabilities()
        .map_err(|error| command_failed(mailbox, "CAPABILITY", &error))?;
    if capabilities.has_str(capability) {
        return Ok(());
    }
    Err(AppError::dependency(
        "IMAP_CAPABILITY_MISSING",
        format!(
            "the IMAP server at {}:{} does not announce {capability} ({standard}), which this action needs; nothing was changed at the provider",
            mailbox.imap_host, mailbox.imap_port
        ),
        false,
    ))
}

/// A command the provider refused or the connection lost. A refusal (`NO`,
/// `BAD`) carries the server's own words and changed nothing; a lost
/// connection leaves the effect unknown, and the next attempt's read of the
/// provider's copy tells which.
fn command_failed(mailbox: &Mailbox, command: &str, error: &imap::Error) -> AppError {
    let lost = matches!(error, imap::Error::Io(_) | imap::Error::ConnectionLost);
    let code = if lost {
        "IMAP_ACTION_UNCERTAIN"
    } else {
        "IMAP_ACTION_REFUSED"
    };
    dependency_error(
        code,
        format!(
            "IMAP {command} for {} at {}:{} failed: {error:?}",
            mailbox.email, mailbox.imap_host, mailbox.imap_port
        ),
        lost,
    )
}

fn unconfirmed(mailbox: &Mailbox, uid: u32, observed: String) -> AppError {
    dependency_error(
        "IMAP_ACTION_UNCONFIRMED",
        format!(
            "UID {uid} in INBOX of {} at {}:{}: {observed}",
            mailbox.email, mailbox.imap_host, mailbox.imap_port
        ),
        false,
    )
}
