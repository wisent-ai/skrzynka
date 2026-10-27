//! `skrzynka message reply|send|outbound`: replying to a stored message, or
//! to every message of a saved view, originating mail from a mailbox, and
//! reading back what went out.

use super::{
    message::{read_body_file, required_key},
    print_json,
    templates::{values, written},
};
use crate::{
    error::AppError,
    mail::{DraftChange, OutgoingAttachment},
    models::{CreateOutboundRequest, CreateReplyRequest},
    service::{AppState, ReplyViewRequest},
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use std::path::{Path, PathBuf};
use uuid::Uuid;

mod outbound;
mod replies;
pub(super) use outbound::{outbound, OutboundArgs};
pub(super) use replies::{replies, RepliesArgs};

/// What `message reply` sends.
#[derive(clap::Args)]
pub(super) struct ReplyArgs {
    /// Message ID being answered.
    #[arg(required_unless_present = "view", conflicts_with = "view")]
    id: Option<Uuid>,
    /// Answer every message this saved view lists instead, each claimed by <KEY>-<message id>.
    #[arg(long)]
    view: Option<String>,
    /// File holding the reply body.
    #[arg(long, required_unless_present = "template", conflicts_with = "template")]
    body_file: Option<PathBuf>,
    /// A template whose text is the reply body, as `message template list` names it.
    #[arg(long)]
    template: Option<String>,
    /// A value for the template's `{{KEY}}` placeholder, as KEY=VALUE; repeatable.
    #[arg(long = "set", requires = "template")]
    set: Vec<String>,
    /// A file to attach, as TYPE=PATH (application/pdf=./quote.pdf); repeatable.
    #[arg(long = "attach")]
    attach: Vec<String>,
    /// Key that makes a repeated reply the same reply; required, never drawn.
    #[arg(long)]
    idempotency_key: Option<String>,
    /// Answer every To and Cc address of the message too, the mailbox's own aside.
    #[arg(long)]
    all: bool,
    /// Quote the message's text below the reply, each line after `> `.
    #[arg(long)]
    quote: bool,
    /// Another address to copy openly; repeatable, or comma-separated.
    #[arg(long = "cc")]
    cc: Vec<String>,
    /// A blind copy: sent to, never named in the headers; repeatable.
    #[arg(long = "bcc")]
    bcc: Vec<String>,
}

/// What `message send` sends.
#[derive(clap::Args)]
pub(super) struct SendArgs {
    /// Mailbox the message is sent from.
    #[arg(long)]
    mailbox: String,
    /// Recipient address; repeatable.
    #[arg(long = "to", required = true)]
    to: Vec<String>,
    /// Copied address; repeatable.
    #[arg(long = "cc")]
    cc: Vec<String>,
    /// Blind copy: sent to, never named in the headers; repeatable.
    #[arg(long = "bcc")]
    bcc: Vec<String>,
    /// Subject line; without it, the subject the --template names.
    #[arg(long, required_unless_present = "template")]
    subject: Option<String>,
    /// File holding the message body.
    #[arg(long, required_unless_present = "template", conflicts_with = "template")]
    body_file: Option<PathBuf>,
    /// A template whose text is the message body, as `message template list` names it.
    #[arg(long)]
    template: Option<String>,
    /// A value for the template's `{{KEY}}` placeholder, as KEY=VALUE; repeatable.
    #[arg(long = "set", requires = "template")]
    set: Vec<String>,
    /// A file to attach, as TYPE=PATH (application/pdf=./quote.pdf); repeatable.
    #[arg(long = "attach")]
    attach: Vec<String>,
    /// Key that makes a repeated send the same send; required, never drawn.
    #[arg(long)]
    idempotency_key: Option<String>,
    /// Send even to an address a stored bounce report says delivery to failed for good.
    #[arg(long)]
    allow_bounced: bool,
    /// Expect an answer by this RFC 3339 instant or span from now (+3d) once it is sent.
    #[arg(long)]
    follow_up: Option<String>,
    /// Send it later: keep it as a draft that a running `skrzynka serve` sends at this RFC 3339 instant.
    #[arg(long, conflicts_with = "allow_bounced", conflicts_with = "follow_up")]
    at: Option<String>,
}

/// One `--attach TYPE=PATH`: the file read now, named by its own file name,
/// with the type the caller stated. Nothing guesses a type from the name.
fn read_attachment(spec: &str) -> Result<OutgoingAttachment, AppError> {
    let invalid = |message: String| AppError::invalid("OUTBOUND_ATTACHMENT_INVALID", message);
    let Some((content_type, path)) = spec.split_once('=') else {
        return Err(invalid(format!(
            "--attach {spec:?} names no type: write it as TYPE=PATH, such as application/pdf=./quote.pdf"
        )));
    };
    let path = Path::new(path);
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| invalid(format!("--attach {spec:?} names no file at its end")))?;
    let bytes = std::fs::read(path).map_err(|cause| {
        invalid(format!("attachment file {} could not be read: {cause}", path.display()))
    })?;
    Ok(OutgoingAttachment {
        filename: filename.to_string(),
        content_type: content_type.to_string(),
        content_base64: STANDARD.encode(bytes),
    })
}

pub(super) fn read_attachments(specs: &[String]) -> Result<Vec<OutgoingAttachment>, AppError> {
    specs.iter().map(|spec| read_attachment(spec)).collect()
}

pub(super) async fn reply(
    state: &AppState,
    organization: &str,
    args: ReplyArgs,
) -> Result<(), AppError> {
    let idempotency_key = required_key(args.idempotency_key, "message reply")?;
    let attachments = read_attachments(&args.attach)?;
    if let Some(view) = args.view {
        let request = ReplyViewRequest {
            idempotency_key,
            body: args.body_file.as_deref().map(|path| read_body_file(path, "REPLY_FILE_INVALID")).transpose()?,
            template: args.template,
            values: values(&args.set)?,
            attachments,
            reply_all: args.all,
            quote: args.quote,
            cc: args.cc,
            bcc: args.bcc,
        };
        return print_json(&state.reply_view(organization, &view, request).await?);
    }
    let Some(id) = args.id else {
        return Err(AppError::invalid("REPLY_TARGET_INVALID", "name the message to answer, or a view with --view"));
    };
    let request = CreateReplyRequest {
        idempotency_key,
        body: written(state, organization, args.body_file.as_ref(), args.template.as_deref(), &args.set, Some(id), "REPLY_FILE_INVALID")?
            .body,
        attachments,
        reply_all: args.all,
        quote: args.quote,
        cc: args.cc,
        bcc: args.bcc,
        automatic: false,
    };
    print_json(&state.reply(organization, id, request).await?)
}

pub(super) async fn send(
    state: &AppState,
    organization: &str,
    args: SendArgs,
) -> Result<(), AppError> {
    let idempotency_key = required_key(args.idempotency_key, "message send")?;
    let mailbox = state.resolve_mailbox(organization, &args.mailbox)?;
    let text = written(
        state,
        organization,
        args.body_file.as_ref(),
        args.template.as_deref(),
        &args.set,
        None,
        "OUTBOUND_FILE_INVALID",
    )?;
    // A subject given wins; otherwise the template's, filled as its text is.
    let subject = match (args.subject, text.subject) {
        (Some(given), _) => given,
        (None, Some(templated)) => templated,
        (None, None) => {
            return Err(AppError::invalid(
                "OUTBOUND_SUBJECT_INVALID",
                format!(
                    "template {:?} names no subject; give --subject, or save the template with --subject",
                    args.template
                ),
            ))
        }
    };
    let attachments = read_attachments(&args.attach)?;
    // Later: a draft scheduled under the key, as `draft save` and `draft schedule` would make it.
    if let Some(at) = args.at {
        let change = DraftChange {
            to: Some(args.to),
            cc: Some(args.cc),
            bcc: Some(args.bcc),
            subject: Some(subject),
            body: Some(text.body),
            attachments: Some(attachments),
        };
        let id = mailbox.id.to_string();
        return print_json(&state.schedule_new_draft(organization, &id, change, &at, &idempotency_key)?);
    }
    let request = CreateOutboundRequest {
        idempotency_key,
        to: args.to,
        cc: args.cc,
        bcc: args.bcc,
        subject,
        body: text.body,
        attachments,
        allow_bounced: args.allow_bounced,
        follow_up: args.follow_up,
    };
    print_json(
        &state
            .send_outbound(organization, mailbox.id, request)
            .await?,
    )
}

