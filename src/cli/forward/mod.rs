//! `skrzynka message forward`: send a stored message on, its original
//! attached, from the mailbox that received it; or every message a saved
//! view lists, each its own forward.

use super::{
    message::{read_body_file, required_key},
    print_json,
    sending::read_attachments,
    templates::{values, written},
};
use crate::{
    error::AppError,
    service::{AppState, ForwardRequest, ForwardViewRequest},
};
use std::path::PathBuf;
use uuid::Uuid;

/// What `message forward` sends.
#[derive(clap::Args)]
pub(super) struct ForwardArgs {
    /// Message ID, as `skrzynka message list` prints it.
    #[arg(required_unless_present = "view", conflicts_with = "view")]
    id: Option<Uuid>,
    /// Forward every message this saved view lists instead, each claimed by <KEY>-<message id>.
    #[arg(long)]
    view: Option<String>,
    /// Recipient address; repeatable.
    #[arg(long = "to", required = true)]
    to: Vec<String>,
    /// Copied address; repeatable.
    #[arg(long = "cc")]
    cc: Vec<String>,
    /// Blind copy: sent to, never named in the headers; repeatable.
    #[arg(long = "bcc")]
    bcc: Vec<String>,
    /// File holding the text sent above the attached original.
    #[arg(
        long,
        required_unless_present = "template",
        conflicts_with = "template"
    )]
    body_file: Option<PathBuf>,
    /// A template whose text is sent above the original, as `message template list` names it.
    #[arg(long)]
    template: Option<String>,
    /// A value for the template's `{{KEY}}` placeholder, as KEY=VALUE; repeatable.
    #[arg(long = "set", requires = "template")]
    set: Vec<String>,
    /// A file of your own sent after the original, as TYPE=PATH; repeatable.
    #[arg(long = "attach")]
    attach: Vec<String>,
    /// Key that makes a repeated forward the same send; required, never drawn.
    #[arg(long)]
    idempotency_key: Option<String>,
    /// Forward even to an address a stored bounce report says delivery to failed for good.
    #[arg(long)]
    allow_bounced: bool,
    /// Expect an answer by this RFC 3339 instant or span from now (+3d) once it is sent.
    #[arg(long)]
    follow_up: Option<String>,
}

pub(super) async fn forward(
    state: &AppState,
    organization: &str,
    args: ForwardArgs,
) -> Result<(), AppError> {
    let idempotency_key = required_key(args.idempotency_key, "message forward")?;
    let attachments = read_attachments(&args.attach)?;
    if let Some(view) = args.view {
        let request = ForwardViewRequest {
            idempotency_key,
            to: args.to,
            cc: args.cc,
            bcc: args.bcc,
            body: args
                .body_file
                .as_deref()
                .map(|path| read_body_file(path, "OUTBOUND_FILE_INVALID"))
                .transpose()?,
            template: args.template,
            values: values(&args.set)?,
            attachments,
            allow_bounced: args.allow_bounced,
            follow_up: args.follow_up,
        };
        return print_json(&state.forward_view(organization, &view, request).await?);
    }
    let Some(id) = args.id else {
        return Err(AppError::invalid(
            "FORWARD_TARGET_INVALID",
            "name the message to forward, or a view with --view",
        ));
    };
    let body = written(
        state,
        organization,
        args.body_file.as_ref(),
        args.template.as_deref(),
        &args.set,
        Some(id),
        "OUTBOUND_FILE_INVALID",
    )?
    .body;
    print_json(
        &state
            .forward_message(
                organization,
                id,
                ForwardRequest {
                    idempotency_key,
                    to: args.to,
                    cc: args.cc,
                    bcc: args.bcc,
                    body,
                    attachments,
                    allow_bounced: args.allow_bounced,
                    follow_up: args.follow_up,
                },
            )
            .await?,
    )
}
