//! The message subcommands: reading mail, replying and sending.

use super::{arguments::MessageCommand, print_json};
use crate::{
    error::AppError,
    models::{CreateOutboundRequest, CreateReplyRequest},
    service::AppState,
};
use std::path::Path;

/// The key a provider-facing send is claimed by. A key drawn per invocation
/// would make a repeated command a second real message, so none is drawn:
/// the caller states the key, and the same key on a repeated command returns
/// the first attempt.
fn required_key(key: Option<String>, command: &str) -> Result<String, AppError> {
    match key {
        Some(key) => Ok(key),
        None => Err(AppError::invalid(
            "IDEMPOTENCY_KEY_REQUIRED",
            format!(
                "{command} needs --idempotency-key: the same key on a repeated command returns the first attempt instead of sending again"
            ),
        )),
    }
}

pub(super) async fn run_message(
    state: AppState,
    organization: &str,
    command: MessageCommand,
) -> Result<(), AppError> {
    match command {
        // An address of a declared inbound domain is read from the receiving
        // feed (`skrzynka mailbox declare --inbound-domain`): the feed is read
        // first, then that address's stored mail is listed.
        MessageCommand::List {
            mailbox: None,
            recipient: Some(recipient),
            sender,
            limit,
            offset,
        } if state.is_inbound_recipient(organization, &recipient)? => {
            let received = state
                .read_inbound(organization, Some(&recipient), sender.as_deref(), None)
                .await?;
            let page = received.into_iter().skip(offset as usize);
            print_json(&match limit {
                Some(limit) => page.take(limit as usize).collect::<Vec<_>>(),
                None => page.collect::<Vec<_>>(),
            })
        }
        MessageCommand::List {
            mailbox,
            recipient,
            sender,
            limit,
            offset,
        } => print_json(&state.list_messages(
            organization,
            mailbox,
            recipient.as_deref(),
            sender.as_deref(),
            limit,
            Some(offset),
        )?),
        MessageCommand::Show { id } => print_json(&state.get_message(organization, id)?),
        MessageCommand::Reply {
            id,
            body_file,
            idempotency_key,
        } => {
            let idempotency_key = required_key(idempotency_key, "message reply")?;
            let body = read_body_file(&body_file, "REPLY_FILE_INVALID")?;
            let request = CreateReplyRequest {
                idempotency_key,
                body,
            };
            print_json(&state.reply(organization, id, request).await?)
        }
        MessageCommand::Send {
            mailbox,
            to,
            cc,
            subject,
            body_file,
            idempotency_key,
        } => {
            let idempotency_key = required_key(idempotency_key, "message send")?;
            let mailbox = state.resolve_mailbox(organization, &mailbox)?;
            let body = read_body_file(&body_file, "OUTBOUND_FILE_INVALID")?;
            let request = CreateOutboundRequest {
                idempotency_key,
                to,
                cc,
                subject,
                body,
            };
            print_json(
                &state
                    .send_outbound(organization, mailbox.id, request)
                    .await?,
            )
        }
        MessageCommand::Outbound {
            mailbox,
            limit,
            offset,
        } => {
            let mailbox_id = match mailbox {
                Some(selector) => Some(state.resolve_mailbox(organization, &selector)?.id),
                None => None,
            };
            print_json(&state.list_outbound(organization, mailbox_id, limit, offset)?)
        }
    }
}

pub(super) fn read_body_file(path: &Path, code: &'static str) -> Result<String, AppError> {
    let metadata = std::fs::metadata(path).map_err(|error| {
        AppError::invalid(
            code,
            format!(
                "message body file {} could not be read: {error}",
                path.display()
            ),
        )
    })?;
    if !metadata.is_file() {
        return Err(AppError::invalid(
            code,
            "message body file must be a regular file",
        ));
    }
    // A permission or I/O failure is not a UTF-8 failure; only the decoding
    // error is reported as one.
    std::fs::read_to_string(path).map_err(|error| match error.kind() {
        std::io::ErrorKind::InvalidData => AppError::invalid(
            code,
            format!(
                "message body file {} must contain valid UTF-8 text",
                path.display()
            ),
        ),
        _ => AppError::invalid(
            code,
            format!(
                "message body file {} could not be read: {error}",
                path.display()
            ),
        ),
    })
}
