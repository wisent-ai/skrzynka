//! `skrzynka message template`: texts kept to reuse in replies and new
//! messages. `message reply --template` and `message send --template` fill
//! their body with one, `--set KEY=VALUE` filling its `{{KEY}}` placeholders;
//! a new message also takes the template's subject when it names none.

use super::{arguments::TemplateCommand, message::read_body_file, output::NewFile, print_json};
use crate::{
    error::AppError,
    service::{AppState, TemplateFilter},
};
use std::collections::BTreeMap;
use std::path::PathBuf;
use uuid::Uuid;

/// What `message template save` stores.
#[derive(clap::Args)]
pub(super) struct SaveArgs {
    /// The template's name, unique within the organization.
    name: String,
    /// File holding the template's text.
    #[arg(long)]
    file: PathBuf,
    /// The subject a new message from the template carries when the send names none.
    #[arg(long)]
    subject: Option<String>,
}

/// One template, named.
#[derive(clap::Args)]
pub(super) struct NameArg {
    /// The template's name, as `skrzynka message template list` prints it.
    name: String,
}

/// What `message template list` and `template export` keep.
#[derive(clap::Args)]
pub(super) struct ListArgs {
    /// Only the templates whose name, subject or text contains this, case aside.
    #[arg(long)]
    contains: Option<String>,
    /// Only the templates whose subject or text names this {{KEY}} placeholder, case aside.
    #[arg(long)]
    placeholder: Option<String>,
}

impl ListArgs {
    fn filter(self) -> TemplateFilter {
        TemplateFilter { contains: self.contains, placeholder: self.placeholder }
    }
}

/// A template and the name it, or its copy, is to carry.
#[derive(clap::Args)]
pub(super) struct RenameArgs {
    /// The template's name, as `skrzynka message template list` prints it.
    name: String,
    /// The name it is to carry; no other template may have it.
    #[arg(long)]
    to: String,
}

/// A template and the values its placeholders are filled with.
#[derive(clap::Args)]
pub(super) struct RenderArgs {
    /// The template's name, as `skrzynka message template list` prints it.
    name: String,
    /// A value for the template's `{{KEY}}` placeholder, as KEY=VALUE; one per placeholder.
    #[arg(long = "set")]
    sets: Vec<String>,
    /// A stored message whose sender, sender's name and subject fill `{{sender}}`, `{{sender_name}}` and `{{subject}}` where no --set does.
    #[arg(long)]
    message: Option<Uuid>,
}

/// What `message template export` writes and which templates.
#[derive(clap::Args)]
pub(super) struct ExportArgs {
    /// New file to write the templates to, as JSON `[{name, subject, body}]`; an existing path is never written over.
    #[arg(long)]
    output: PathBuf,
    #[command(flatten)]
    listing: ListArgs,
}

/// What `message template import` reads.
#[derive(clap::Args)]
pub(super) struct ImportArgs {
    /// A file `message template export` wrote: JSON `[{name, subject, body}]`.
    #[arg(long)]
    input: PathBuf,
}

pub(super) fn run_template(
    state: &AppState,
    organization: &str,
    command: TemplateCommand,
) -> Result<(), AppError> {
    match command {
        TemplateCommand::Save(SaveArgs { name, file, subject }) => {
            let body = read_body_file(&file, "TEMPLATE_FILE_INVALID")?;
            print_json(&state.save_template(organization, &name, subject.as_deref(), &body)?)
        }
        TemplateCommand::List(listing) => print_json(&state.templates(organization, &listing.filter())?),
        TemplateCommand::Show(NameArg { name }) => print_json(&state.template(organization, &name)?),
        TemplateCommand::Render(RenderArgs { name, sets, message }) => {
            print_json(&state.render_template(organization, &name, &values(&sets)?, message)?)
        }
        TemplateCommand::Rename(RenameArgs { name, to }) => {
            print_json(&state.rename_template(organization, &name, &to)?)
        }
        TemplateCommand::Copy(RenameArgs { name, to }) => {
            print_json(&state.copy_template(organization, &name, &to)?)
        }
        TemplateCommand::Delete(NameArg { name }) => {
            print_json(&state.delete_template(organization, &name)?)
        }
        TemplateCommand::Export(ExportArgs { output, listing }) => {
            let file = NewFile { path: &output, exists: "TEMPLATE_OUTPUT_EXISTS", invalid: "TEMPLATE_OUTPUT_INVALID" };
            file.refuse_occupied()?;
            let filter = listing.filter();
            let templates = state.templates(organization, &filter)?.len();
            let document = state.template_document(organization, &filter)?;
            file.write(document.as_bytes())?;
            print_json(&serde_json::json!({ "path": output, "templates": templates, "size_bytes": document.len() }))
        }
        TemplateCommand::Import(ImportArgs { input }) => {
            print_json(&state.import_templates(organization, &read_body_file(&input, "TEMPLATE_INVALID")?)?)
        }
    }
}

/// The text a send takes: a body, and the subject a template names.
pub(super) struct Written {
    /// The template's subject, its placeholders filled; absent for a file
    /// or a template without one.
    pub(super) subject: Option<String>,
    pub(super) body: String,
}

/// The text a send takes: the file's, or the named template's with its
/// placeholders filled from `sets` (`--set KEY=VALUE`) and, for a reply or a
/// forward, from the stored `message` it answers.
pub(super) fn written(
    state: &AppState,
    organization: &str,
    file: Option<&PathBuf>,
    template: Option<&str>,
    sets: &[String],
    message: Option<Uuid>,
    code: &'static str,
) -> Result<Written, AppError> {
    match (file, template) {
        (Some(file), None) if sets.is_empty() => Ok(Written { subject: None, body: read_body_file(file, code)? }),
        (None, Some(name)) => {
            let filled = state.render_template(organization, name, &values(sets)?, message)?;
            Ok(Written { subject: filled.subject, body: filled.body })
        }
        _ => Err(AppError::invalid(
            code,
            "name the body with exactly one of --body-file and --template; --set fills a template only",
        )),
    }
}

/// The `--set KEY=VALUE` values by key. One without `=` or a key, or a key
/// given twice, is `TEMPLATE_VALUE_INVALID`.
pub(super) fn values(sets: &[String]) -> Result<BTreeMap<String, String>, AppError> {
    let mut values = BTreeMap::new();
    for set in sets {
        let Some((key, value)) = set.split_once('=').filter(|(key, _)| !key.trim().is_empty()) else {
            return Err(AppError::invalid(
                "TEMPLATE_VALUE_INVALID",
                format!("--set {set:?} is not KEY=VALUE; name the placeholder's key before ="),
            ));
        };
        if values.insert(key.trim().to_string(), value.to_string()).is_some() {
            return Err(AppError::invalid(
                "TEMPLATE_VALUE_INVALID",
                format!("--set gives {:?} twice; give each placeholder one value", key.trim()),
            ));
        }
    }
    Ok(values)
}
