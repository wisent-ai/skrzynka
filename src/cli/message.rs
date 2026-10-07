//! The message subcommands: reading mail, replying and sending.

use super::{arguments::MessageCommand, print_json};
use crate::{
    error::AppError,
    models::{CreateOutboundRequest, CreateReplyRequest},
    service::AppState,
};
use std::path::Path;
use uuid::Uuid;

pub(super) async fn run_message(
    state: AppState,
    organization: &str,
    command: MessageCommand,
) -> Result<(), AppError> {
    match command {
        MessageCommand::List {
            mailbox,
            limit,
            offset,
        } => print_json(&state.list_messages(organization, mailbox, limit, offset)?),
        MessageCommand::Show { id } => print_json(&state.get_message(organization, id)?),
        MessageCommand::Reply {
            id,
            body_file,
            idempotency_key,
        } => {
            let body = read_body_file(&body_file, "REPLY_FILE_INVALID")?;
            let request = CreateReplyRequest {
                idempotency_key: idempotency_key.unwrap_or_else(|| Uuid::new_v4().to_string()),
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
            let mailbox = state.resolve_mailbox(organization, &mailbox)?;
            let body = read_body_file(&body_file, "OUTBOUND_FILE_INVALID")?;
            let request = CreateOutboundRequest {
                idempotency_key: idempotency_key.unwrap_or_else(|| Uuid::new_v4().to_string()),
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
    let metadata = std::fs::metadata(path)
        .map_err(|_| AppError::invalid(code, "message body file could not be read"))?;
    if !metadata.is_file() {
        return Err(AppError::invalid(
            code,
            "message body file must be a regular file",
        ));
    }
    std::fs::read_to_string(path)
        .map_err(|_| AppError::invalid(code, "message body file must contain valid UTF-8 text"))
}
