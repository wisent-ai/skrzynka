//! `skrzynka message draft`: write a message over time, keep it in Skrzynka,
//! and send it when it is ready.

use super::{
    arguments::DraftCommand,
    message::{read_body_file, required_key},
    output::NewFile,
    print_json,
    sending::read_attachments,
    templates::values,
};
use crate::{
    error::AppError,
    mail::DraftChange,
    service::{draft_table, AppState, DraftFilter, DraftSendRequest},
};
use std::path::PathBuf;
use uuid::Uuid;

/// The fields a draft is written with; each one given replaces what the
/// draft holds.
#[derive(clap::Args)]
pub(super) struct DraftFields {
    /// Recipient address; repeatable. Given at all, it replaces every recipient.
    #[arg(long = "to")]
    to: Vec<String>,
    /// Copied address; repeatable. Given at all, it replaces every copied address.
    #[arg(long = "cc")]
    cc: Vec<String>,
    /// Blind copy, sent to and never named in the headers; repeatable. Given at all, it replaces every one.
    #[arg(long = "bcc")]
    bcc: Vec<String>,
    /// Subject line; without it, a --template's subject when it names one.
    #[arg(long)]
    subject: Option<String>,
    /// UTF-8 file holding the body.
    #[arg(long, conflicts_with = "template")]
    body_file: Option<PathBuf>,
    /// A template whose text becomes the body, as `message template list` names it.
    #[arg(long)]
    template: Option<String>,
    /// A value for the template's `{{KEY}}` placeholder, as KEY=VALUE; repeatable.
    #[arg(long = "set", requires = "template")]
    set: Vec<String>,
    /// File to carry, as TYPE=PATH (such as application/pdf=./quote.pdf); repeatable.
    /// Given at all, it replaces every file the draft holds.
    #[arg(long = "attach", conflicts_with = "no_attachments")]
    attach: Vec<String>,
    /// Remove every file the draft holds.
    #[arg(long)]
    no_attachments: bool,
}

/// What `message draft save` starts.
#[derive(clap::Args)]
pub(super) struct SaveArgs {
    /// Mailbox the draft will be sent from, by ID or address.
    #[arg(long)]
    mailbox: String,
    /// With --template, one draft per row of this UTF-8 CSV table instead: its `to` column addresses each,
    /// every other column fills the placeholder it names; nothing is kept unless every row is.
    #[arg(long, requires = "template", conflicts_with_all = ["to", "cc", "bcc", "subject", "set", "attach", "no_attachments"])]
    merge: Option<PathBuf>,
    #[command(flatten)]
    fields: DraftFields,
}

/// What `message draft update` changes.
#[derive(clap::Args)]
pub(super) struct UpdateArgs {
    /// Draft ID, as `skrzynka message draft list` prints it.
    id: Uuid,
    #[command(flatten)]
    fields: DraftFields,
}

/// What `message draft delete` removes and `message draft unschedule` takes
/// off its schedule: one draft, or every draft mentioning a text, such as a
/// mail merge made by mistake or a staggered one to hold back.
#[derive(clap::Args)]
pub(super) struct ChosenDrafts {
    /// Draft ID, as `skrzynka message draft list` prints it.
    #[arg(required_unless_present = "contains", conflicts_with = "contains")]
    id: Option<Uuid>,
    /// Every draft whose To, Cc, Bcc, subject or body contains this, case aside, instead.
    #[arg(long)]
    contains: Option<String>,
    /// With --contains, only this mailbox's drafts, by ID or address.
    #[arg(long, requires = "contains")]
    mailbox: Option<String>,
}

/// One draft, named by its ID.
#[derive(clap::Args)]
pub(super) struct DraftRef {
    /// Draft ID, as `skrzynka message draft list` prints it.
    id: Uuid,
}

/// One originated message, named by its ID.
#[derive(clap::Args)]
pub(super) struct OutboundRef {
    /// Outbound message ID, as `skrzynka message outbound` prints it.
    outbound_id: Uuid,
}

/// What `message draft copy` copies, and the mailbox the copy is written from.
#[derive(clap::Args)]
pub(super) struct CopyArgs {
    /// Draft ID, as `skrzynka message draft list` prints it.
    id: Uuid,
    /// Write the copy from this mailbox, by ID or address; the draft's own when absent.
    #[arg(long)]
    mailbox: Option<String>,
}

/// What `message draft send` sends: one draft, or every draft mentioning a text.
#[derive(clap::Args)]
pub(super) struct SendArgs {
    #[command(flatten)]
    chosen: ChosenDrafts,
    /// Key that makes a repeated send the same send; required, never drawn.
    #[arg(long)]
    idempotency_key: Option<String>,
    /// Send even to an address a stored bounce report says delivery to failed for good.
    #[arg(long)]
    allow_bounced: bool,
    /// Expect an answer by this RFC 3339 instant or span from now (+3d) once it is sent.
    #[arg(long)]
    follow_up: Option<String>,
}

/// What `message draft schedule` sets.
#[derive(clap::Args)]
pub(super) struct ScheduleArgs {
    /// Draft ID, as `skrzynka message draft list` prints it.
    #[arg(required_unless_present = "contains", conflicts_with = "contains")]
    id: Option<Uuid>,
    /// Schedule every draft mentioning this text and not scheduled yet instead, such as a mail merge's.
    #[arg(long)]
    contains: Option<String>,
    /// With --contains, only this mailbox's drafts, by ID or address.
    #[arg(long, requires = "contains")]
    mailbox: Option<String>,
    /// With --contains, send each next draft this much after the one before (5m, 1h, 1d), earliest written first.
    #[arg(long, requires = "contains")]
    every: Option<String>,
    /// RFC 3339 instant to send at, such as 2025-01-31T09:00:00+01:00; it must lie ahead.
    #[arg(long)]
    at: String,
    /// Key the scheduled send is claimed by; with --contains, each draft's is this key, a dash and its ID.
    #[arg(long)]
    idempotency_key: Option<String>,
}

/// What `message draft list` keeps: one mailbox's drafts, the scheduled
/// ones, or those mentioning a text.
#[derive(clap::Args)]
pub(super) struct ListArgs {
    /// Only this mailbox's drafts, by ID or address.
    #[arg(long)]
    mailbox: Option<String>,
    /// Only drafts a running service will send at their instant, soonest first.
    #[arg(long)]
    scheduled: bool,
    /// Only drafts whose To, Cc, Bcc, subject or body contains this, case aside.
    #[arg(long)]
    contains: Option<String>,
    /// Only drafts addressed to an address `message bounced` lists, which a send refuses.
    #[arg(long)]
    bounced: bool,
    /// Only drafts addressed to a recipient `message suppressed` covers, which a send and a schedule refuse.
    #[arg(long)]
    suppressed: bool,
    /// Only drafts last changed at or after an RFC 3339 instant, YYYY-MM-DD day or span back such as -7d.
    #[arg(long, allow_hyphen_values = true)]
    since: Option<String>,
    /// Only drafts last changed before this: the ones left untouched, such as --until -7d.
    #[arg(long, allow_hyphen_values = true)]
    until: Option<String>,
    /// Only drafts carrying at least one file.
    #[arg(long)]
    with_attachments: bool,
    /// Only scheduled drafts going out before this: +1d (what goes out within a day), an RFC 3339 instant or a YYYY-MM-DD day.
    #[arg(long, allow_hyphen_values = true)]
    send_by: Option<String>,
    /// Write the drafts listed as a CSV table (RFC 4180), one row each without the body, to this new file.
    #[arg(long)]
    csv: Option<PathBuf>,
}

impl DraftFields {
    /// The change these fields make; a flag not given changes nothing.
    fn change(self, state: &AppState, organization: &str) -> Result<DraftChange, AppError> {
        let (templated, body) = match (self.body_file, self.template) {
            (Some(path), _) => (None, Some(read_body_file(&path, "DRAFT_FILE_INVALID")?)),
            (None, Some(name)) => {
                let filled = state.render_template(organization, &name, &values(&self.set)?, None)?;
                (filled.subject, Some(filled.body))
            }
            (None, None) => (None, None),
        };
        Ok(DraftChange {
            to: (!self.to.is_empty()).then_some(self.to),
            cc: (!self.cc.is_empty()).then_some(self.cc),
            bcc: (!self.bcc.is_empty()).then_some(self.bcc),
            subject: self.subject.or(templated),
            body,
            attachments: match (self.attach.is_empty(), self.no_attachments) {
                (false, _) => Some(read_attachments(&self.attach)?),
                (true, true) => Some(Vec::new()),
                (true, false) => None,
            },
        })
    }
}

pub(super) async fn run_draft(
    state: &AppState,
    organization: &str,
    command: DraftCommand,
) -> Result<(), AppError> {
    match command {
        DraftCommand::Save(SaveArgs { mailbox, merge: Some(path), fields }) => {
            let refused = |message: String| AppError::invalid("DRAFT_MERGE_INVALID", format!("{message}; nothing was drafted"));
            let Some(template) = fields.template else {
                return Err(refused("--merge fills a template; name it with --template".to_string()));
            };
            let table = std::fs::read_to_string(&path)
                .map_err(|error| refused(format!("the table {} could not be read as UTF-8 text: {error}", path.display())))?;
            print_json(&state.merge_drafts(organization, &mailbox, &template, &table)?)
        }
        DraftCommand::Save(SaveArgs { mailbox, merge: None, fields }) => {
            print_json(&state.create_draft(organization, &mailbox, fields.change(state, organization)?)?)
        }
        DraftCommand::Again(OutboundRef { outbound_id }) => {
            print_json(&state.draft_from_outbound(organization, outbound_id)?)
        }
        DraftCommand::Copy(CopyArgs { id, mailbox }) => {
            print_json(&state.copy_draft(organization, id, mailbox.as_deref())?)
        }
        DraftCommand::Update(UpdateArgs { id, fields }) => {
            print_json(&state.update_draft(organization, id, fields.change(state, organization)?)?)
        }
        DraftCommand::List(ListArgs { mailbox, scheduled, contains, bounced, suppressed, since, until, with_attachments, send_by, csv }) => {
            let mailbox_id = super::reading::chosen_mailbox(state, organization, mailbox)?;
            let filter =
                DraftFilter { mailbox_id, scheduled, contains, bounced, suppressed, since, until, with_attachments, send_by };
            let Some(path) = csv else {
                return print_json(&state.list_drafts(organization, &filter)?);
            };
            let file = NewFile { path: &path, exists: "DRAFT_OUTPUT_EXISTS", invalid: "DRAFT_OUTPUT_INVALID" };
            file.refuse_occupied()?;
            let drafts = state.list_drafts(organization, &filter)?;
            let table = draft_table(&drafts);
            file.write(table.as_bytes())?;
            print_json(&serde_json::json!({ "path": path, "drafts": drafts.len(), "size_bytes": table.len() }))
        }
        DraftCommand::Show(DraftRef { id }) => print_json(&state.get_draft(organization, id)?),
        DraftCommand::Delete(ChosenDrafts { id: Some(id), .. }) => {
            state.delete_draft(organization, id)?;
            print_json(&serde_json::json!({ "deleted": id }))
        }
        DraftCommand::Delete(ChosenDrafts { id: None, contains, mailbox }) => {
            let mailbox_id = super::reading::chosen_mailbox(state, organization, mailbox)?;
            print_json(&state.delete_drafts(organization, &DraftFilter { mailbox_id, contains, ..DraftFilter::default() })?)
        }
        DraftCommand::Send(SendArgs { chosen: ChosenDrafts { id: Some(id), .. }, idempotency_key, allow_bounced, follow_up }) => {
            let request = DraftSendRequest { idempotency_key: required_key(idempotency_key, "message draft send")?, allow_bounced, follow_up };
            print_json(&state.send_draft(organization, id, request).await?)
        }
        DraftCommand::Send(SendArgs { chosen: ChosenDrafts { id: None, contains, mailbox }, idempotency_key, allow_bounced, follow_up }) => {
            let key = required_key(idempotency_key, "message draft send")?;
            let mailbox_id = super::reading::chosen_mailbox(state, organization, mailbox)?;
            let filter = DraftFilter { mailbox_id, contains, ..DraftFilter::default() };
            print_json(&state.send_drafts(organization, &filter, &key, allow_bounced, follow_up.as_deref()).await?)
        }
        DraftCommand::Schedule(ScheduleArgs { id, contains, mailbox, every, at, idempotency_key }) => {
            let key = required_key(idempotency_key, "message draft schedule")?;
            if let Some(id) = id {
                return print_json(&state.schedule_draft(organization, id, &at, &key)?);
            }
            let mailbox_id = super::reading::chosen_mailbox(state, organization, mailbox)?;
            let filter = DraftFilter { mailbox_id, contains, ..DraftFilter::default() };
            print_json(&state.schedule_drafts(organization, &filter, &at, every.as_deref(), &key)?)
        }
        DraftCommand::Unschedule(ChosenDrafts { id: Some(id), .. }) => print_json(&state.unschedule_draft(organization, id)?),
        DraftCommand::Unschedule(ChosenDrafts { id: None, contains, mailbox }) => {
            let mailbox_id = super::reading::chosen_mailbox(state, organization, mailbox)?;
            print_json(&state.unschedule_drafts(organization, &DraftFilter { mailbox_id, contains, ..DraftFilter::default() })?)
        }
    }
}
