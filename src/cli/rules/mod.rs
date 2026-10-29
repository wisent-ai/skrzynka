//! `skrzynka message rule`: the operator's rules that tag mail as it
//! arrives, and tagging the mail already stored by them.

use super::{arguments::RuleCommand, message::read_body_file, output::NewFile, print_json};
use crate::{
    error::AppError,
    service::{AppState, NewTagRule},
};
use uuid::Uuid;

/// What `message rule add` stores, and what `message rule update` replaces
/// a rule's tag, conditions and mailbox with.
#[derive(clap::Args)]
pub(super) struct AddArgs {
    /// The tag a matching message gets.
    #[arg(long)]
    tag: String,
    /// Text the From header must contain, case aside.
    #[arg(long)]
    sender: Option<String>,
    /// Text the List-Id header must contain, case aside.
    #[arg(long)]
    list: Option<String>,
    /// Text the subject or stored text must contain, case aside.
    #[arg(long)]
    contains: Option<String>,
    /// Text the To or Cc header must contain, case aside: mail sent to one address or alias.
    #[arg(long)]
    recipient: Option<String>,
    /// Text the recorded name of one of the message's files must contain, case aside.
    #[arg(long)]
    attachment: Option<String>,
    /// Tag only this mailbox's mail, by ID or address; every mailbox's when absent.
    #[arg(long)]
    mailbox: Option<String>,
}

impl AddArgs {
    fn rule(self) -> NewTagRule {
        let AddArgs {
            tag,
            sender,
            list,
            contains,
            recipient,
            attachment,
            mailbox,
        } = self;
        NewTagRule {
            tag,
            sender,
            list,
            contains,
            recipient,
            attachment,
            mailbox,
        }
    }
}

/// `message rule update`: the rule and everything it is to be from now on.
#[derive(clap::Args)]
pub(super) struct UpdateArgs {
    /// Rule ID, as `skrzynka message rule list` prints it.
    id: Uuid,
    #[command(flatten)]
    rule: AddArgs,
}

/// One rule, named by its ID.
#[derive(clap::Args)]
pub(super) struct RuleRef {
    /// Rule ID, as `skrzynka message rule list` prints it.
    id: Uuid,
}

/// What `message rule copy` makes: a new rule with a rule's conditions.
#[derive(clap::Args)]
pub(super) struct CopyArgs {
    /// Rule ID, as `skrzynka message rule list` prints it.
    id: Uuid,
    /// The tag the new rule gives.
    #[arg(long)]
    tag: String,
    /// Keep the new rule to this mailbox, by ID or address; the copied rule's mailbox when left out.
    #[arg(long)]
    mailbox: Option<String>,
}

/// Which rules `message rule list` prints.
#[derive(clap::Args)]
pub(super) struct ListArgs {
    /// Only the rules whose tag or a condition holds this text, case aside.
    #[arg(long)]
    contains: Option<String>,
    /// Only the paused rules.
    #[arg(long, conflicts_with = "active")]
    paused: bool,
    /// Only the rules tagging imported mail now.
    #[arg(long)]
    active: bool,
    /// Only the rules that tag this mailbox's mail, by ID or address: those kept to it and those of every mailbox.
    #[arg(long)]
    mailbox: Option<String>,
}

/// What `message rule apply` tags by: every rule, or one.
#[derive(clap::Args)]
pub(super) struct ApplyArgs {
    /// Apply only this rule, by its ID as `skrzynka message rule list` prints it.
    #[arg(long)]
    rule: Option<Uuid>,
}

/// What `message rule export` writes: the rules `rule list` keeps for the
/// same flags.
#[derive(clap::Args)]
pub(super) struct ExportArgs {
    /// New file to write the rules to, as JSON `[{tag, sender, list, contains, recipient, attachment, mailbox}]`; an existing path is never written over.
    #[arg(long)]
    output: std::path::PathBuf,
    #[command(flatten)]
    listing: ListArgs,
}

/// What `message rule import` reads.
#[derive(clap::Args)]
pub(super) struct ImportArgs {
    /// A file `message rule export` wrote.
    #[arg(long)]
    input: std::path::PathBuf,
}

/// The paused state and mailbox `rule list` and `rule export` keep.
fn listing_filters(
    state: &AppState,
    organization: &str,
    listing: &ListArgs,
) -> Result<(Option<bool>, Option<Uuid>), AppError> {
    let wanted = match (listing.paused, listing.active) {
        (true, _) => Some(true),
        (_, true) => Some(false),
        _ => None,
    };
    Ok((
        wanted,
        super::reading::chosen_mailbox(state, organization, listing.mailbox.clone())?,
    ))
}

pub(super) fn run_rule(
    state: &AppState,
    organization: &str,
    command: RuleCommand,
) -> Result<(), AppError> {
    match command {
        RuleCommand::Add(rule) => print_json(&state.add_tag_rule(organization, rule.rule())?),
        RuleCommand::Matches(RuleRef { id }) => {
            print_json(&state.tag_rule_matches(organization, id)?)
        }
        RuleCommand::Update(UpdateArgs { id, rule }) => {
            print_json(&state.replace_tag_rule(organization, id, rule.rule())?)
        }
        RuleCommand::List(listing) => {
            let (wanted, mailbox_id) = listing_filters(state, organization, &listing)?;
            print_json(&state.tag_rules(
                organization,
                listing.contains.as_deref(),
                wanted,
                mailbox_id,
            )?)
        }
        RuleCommand::Export(ExportArgs { output, listing }) => {
            let file = NewFile {
                path: &output,
                exists: "TAG_RULE_OUTPUT_EXISTS",
                invalid: "TAG_RULE_OUTPUT_INVALID",
            };
            file.refuse_occupied()?;
            let (wanted, mailbox_id) = listing_filters(state, organization, &listing)?;
            let contains = listing.contains.as_deref();
            let rules = state
                .tag_rules(organization, contains, wanted, mailbox_id)?
                .len();
            let document = state.tag_rule_document(organization, contains, wanted, mailbox_id)?;
            file.write(document.as_bytes())?;
            print_json(
                &serde_json::json!({ "path": output, "rules": rules, "size_bytes": document.len() }),
            )
        }
        RuleCommand::Import(ImportArgs { input }) => print_json(
            &state.import_tag_rules(organization, &read_body_file(&input, "TAG_RULE_INVALID")?)?,
        ),
        RuleCommand::Show(RuleRef { id }) => print_json(&state.tag_rule(organization, id)?),
        RuleCommand::Copy(CopyArgs { id, tag, mailbox }) => {
            print_json(&state.copy_tag_rule(organization, id, &tag, mailbox.as_deref())?)
        }
        RuleCommand::Pause(RuleRef { id }) => {
            print_json(&state.pause_tag_rule(organization, id, true)?)
        }
        RuleCommand::Resume(RuleRef { id }) => {
            print_json(&state.pause_tag_rule(organization, id, false)?)
        }
        RuleCommand::Remove(RuleRef { id }) => {
            state.remove_tag_rule(organization, id)?;
            print_json(&serde_json::json!({ "removed": id }))
        }
        RuleCommand::Apply(ApplyArgs { rule }) => {
            print_json(&state.apply_tag_rules(organization, rule)?)
        }
    }
}
