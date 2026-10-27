//! Which templates a listing keeps: those holding a text, and those whose
//! subject or text names one `{{key}}` placeholder.

use super::super::{messages::filter_value, AppState};
use crate::{error::AppError, mail::MessageTemplate};
use serde::Deserialize;

/// What `message template list`, `template export` and `GET /v1/templates`
/// keep, as written; an absent one is no filter.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct TemplateFilter {
    /// Only the templates whose name, subject or text contains this, case aside.
    #[serde(default)]
    pub contains: Option<String>,
    /// Only the templates whose subject or text names this `{{key}}`
    /// placeholder, case aside.
    #[serde(default)]
    pub placeholder: Option<String>,
}

impl AppState {
    /// Every template of the organization, in name order, narrowed as
    /// `filter` says. A blank `contains` or `placeholder` is
    /// `MESSAGE_FILTER_INVALID`.
    pub fn templates(&self, organization_id: &str, filter: &TemplateFilter) -> Result<Vec<MessageTemplate>, AppError> {
        let contains = filter_value(filter.contains.as_deref(), "contains")?;
        let placeholder = filter_value(filter.placeholder.as_deref(), "placeholder")?;
        let mut templates = self.database.templates(organization_id)?;
        if let Some(part) = contains.as_deref() {
            templates.retain(|template| {
                [Some(template.name.as_str()), template.subject.as_deref(), Some(template.body.as_str())]
                    .into_iter()
                    .flatten()
                    .any(|text| text.to_lowercase().contains(part))
            });
        }
        if let Some(key) = placeholder.as_deref() {
            templates.retain(|template| template.placeholders.iter().any(|named| named.to_lowercase() == key));
        }
        Ok(templates)
    }
}
