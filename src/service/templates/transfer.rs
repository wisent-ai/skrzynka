//! Templates carried between installations: every template, or those
//! holding a text, written as one JSON document, and such a document read
//! back, every template checked before any is kept.

use super::{super::AppState, checked_template};
use crate::{error::AppError, mail::MessageTemplate};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// One template as an export writes it and an import reads it: its name,
/// the subject a new message from it carries when it names one, and its text.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateFile {
    pub name: String,
    #[serde(default)]
    pub subject: Option<String>,
    pub body: String,
}

/// What `message template import` did: every template kept, in file order;
/// one already of that name was replaced, as `template save` replaces it.
#[derive(Debug, Serialize)]
pub struct ImportedTemplates {
    pub templates: Vec<MessageTemplate>,
}

impl AppState {
    /// The templates `templates` keeps for `filter`, as the JSON document
    /// `import_templates` reads: `[{name, subject, body}]` in name order.
    pub fn template_document(
        &self,
        organization_id: &str,
        filter: &super::TemplateFilter,
    ) -> Result<String, AppError> {
        let files: Vec<TemplateFile> = self
            .templates(organization_id, filter)?
            .into_iter()
            .map(|template| TemplateFile {
                name: template.name,
                subject: template.subject,
                body: template.body,
            })
            .collect();
        serde_json::to_string_pretty(&files).map_err(|error| {
            AppError::internal(format!(
                "the templates could not be written as JSON: {error}"
            ))
        })
    }

    /// Keep every template a document `template_document` wrote holds. The
    /// whole document is read and every template checked as `template save`
    /// checks one before any is kept, so a document that is not such a list,
    /// a template refused, a name given twice or a document with none is
    /// `TEMPLATE_INVALID` and nothing changes.
    pub fn import_templates(
        &self,
        organization_id: &str,
        text: &str,
    ) -> Result<ImportedTemplates, AppError> {
        let files: Vec<TemplateFile> = serde_json::from_str(text).map_err(|error| {
            AppError::invalid(
                "TEMPLATE_INVALID",
                format!("the import is not a list of {{name, subject, body}} templates ({error}); nothing was imported"),
            )
        })?;
        if files.is_empty() {
            return Err(AppError::invalid(
                "TEMPLATE_INVALID",
                "the import holds no template; export some with message template export",
            ));
        }
        let mut seen = BTreeSet::new();
        for file in &files {
            let (name, _) = checked_template(&file.name, file.subject.as_deref(), &file.body)
                .map_err(|refusal| {
                    AppError::invalid(
                        "TEMPLATE_INVALID",
                        format!("{}; nothing was imported", refusal.message),
                    )
                })?;
            if !seen.insert(name.clone()) {
                return Err(AppError::invalid(
                    "TEMPLATE_INVALID",
                    format!("the import names the template {name:?} twice; nothing was imported"),
                ));
            }
        }
        let mut templates = Vec::new();
        for file in files {
            templates.push(self.save_template(
                organization_id,
                &file.name,
                file.subject.as_deref(),
                &file.body,
            )?);
        }
        Ok(ImportedTemplates { templates })
    }
}
