//! The text sent to every message of a view at once, such as a forward or a
//! reply to each: one body for all, or a template filled for each message
//! with the values given and that message's sender, sender's name and
//! subject.

use super::super::AppState;
use crate::error::AppError;
use std::collections::BTreeMap;
use uuid::Uuid;

/// What each message of a view is sent: the same body, or a template filled
/// for that message.
pub(in crate::service) enum PerMessage<'a> {
    Body(&'a str),
    Template { name: &'a str, values: &'a BTreeMap<String, String> },
}

impl<'a> PerMessage<'a> {
    /// The text a request names: exactly one of `body` and `template`, else
    /// `code` saying so, before anything is sent.
    pub(in crate::service) fn chosen(
        body: &'a Option<String>,
        template: &'a Option<String>,
        values: &'a BTreeMap<String, String>,
        code: &'static str,
    ) -> Result<Self, AppError> {
        match (body, template) {
            (Some(body), None) => Ok(Self::Body(body)),
            (None, Some(name)) => Ok(Self::Template { name, values }),
            _ => Err(AppError::invalid(code, "name the text sent for each message, as a body or a template, and only one of them")),
        }
    }

    /// The text for stored message `message_id`; a template's own refusal
    /// (a placeholder nothing fills) is that message's.
    pub(in crate::service) fn for_message(&self, state: &AppState, organization_id: &str, message_id: Uuid) -> Result<String, AppError> {
        match self {
            Self::Body(body) => Ok(body.to_string()),
            Self::Template { name, values } => Ok(state.render_template(organization_id, name, values, Some(message_id))?.body),
        }
    }
}
