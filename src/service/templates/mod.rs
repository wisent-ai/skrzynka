//! Message templates: texts kept to reuse in replies and new messages. A
//! template is only text; using one fills a body, and the send is held to
//! every rule a send is.

use super::AppState;
use crate::{
    error::AppError,
    mail::{placeholders, MessageTemplate},
};
use axum::http::StatusCode;
use std::collections::BTreeMap;
use uuid::Uuid;

mod filter;
mod per_message;
mod transfer;
pub use filter::TemplateFilter;
pub(in crate::service) use per_message::PerMessage;

/// The display name of a From entry such as `"Ada Lovelace" <ada@example.com>`,
/// unquoted; none when the entry names only an address.
fn display_name(sender: &str) -> Option<String> {
    let (name, _) = sender.split_once('<')?;
    let name = name.trim().trim_matches('"').trim();
    (!name.is_empty()).then(|| name.to_string())
}

impl AppState {
    /// One template by name; an unknown name is refused naming the ones there are.
    pub fn template(&self, organization_id: &str, name: &str) -> Result<MessageTemplate, AppError> {
        let name = template_name(name)?;
        match self.database.template(organization_id, &name)? {
            Some(template) => Ok(template),
            None => Err(self.unknown(organization_id, &name)?),
        }
    }

    /// The template with every `{{key}}` placeholder of its subject and text
    /// filled with the value given for its key: what a send takes. With
    /// `message`, a stored message answered or forwarded, the placeholders
    /// `sender`, `sender_name` and `subject` the template names and no value
    /// gives are filled from that message, when it states them. A key
    /// without a value is `TEMPLATE_VALUE_MISSING`, a value for no
    /// placeholder `TEMPLATE_VALUE_UNUSED`.
    pub fn render_template(
        &self,
        organization_id: &str,
        name: &str,
        values: &BTreeMap<String, String>,
        message: Option<Uuid>,
    ) -> Result<MessageTemplate, AppError> {
        let template = self.template(organization_id, name)?;
        let mut values = values.clone();
        if let Some(id) = message {
            let message = self.get_message(organization_id, id)?;
            let address = super::messages::entry_address(&message.sender).to_string();
            let stated = [
                ("sender", Some(address)),
                ("sender_name", display_name(&message.sender)),
                ("subject", message.subject.clone()),
            ];
            for (key, value) in stated {
                if let Some(value) =
                    value.filter(|_| template.placeholders.iter().any(|named| named == key))
                {
                    values.entry(key.to_string()).or_insert(value);
                }
            }
        }
        let values = &values;
        let (subject, body) = match template.subject.as_deref() {
            Some(written) => {
                let [subject, body] =
                    placeholders::fill(&template.name, [written, &template.body], values)?;
                (Some(subject), body)
            }
            None => {
                let [body] = placeholders::fill(&template.name, [&template.body], values)?;
                (None, body)
            }
        };
        Ok(MessageTemplate {
            subject,
            body,
            ..template
        })
    }

    /// Store the template, replacing the subject and text of one of the
    /// same name. A subject is stored trimmed; one with no visible text, or
    /// holding a line break, is `TEMPLATE_INVALID`.
    pub fn save_template(
        &self,
        organization_id: &str,
        name: &str,
        subject: Option<&str>,
        body: &str,
    ) -> Result<MessageTemplate, AppError> {
        let (name, subject) = checked_template(name, subject, body)?;
        self.database
            .save_template(organization_id, &name, subject, body)
    }

    /// Give a template another name, keeping its text. The new name is held to
    /// the rules a saved one is; a name another template carries is
    /// `TEMPLATE_EXISTS`.
    pub fn rename_template(
        &self,
        organization_id: &str,
        name: &str,
        to: &str,
    ) -> Result<MessageTemplate, AppError> {
        let from = template_name(name)?;
        let to = template_name(to)?;
        if from == to {
            return self.template(organization_id, &from);
        }
        self.refuse_taken(organization_id, &to)?;
        match self.database.rename_template(organization_id, &from, &to)? {
            Some(template) => Ok(template),
            None => Err(self.unknown(organization_id, &from)?),
        }
    }

    /// Keep a template's text under another name too, to change one without
    /// the other. The new name is held to the rules a saved one is; a name
    /// another template carries, the template's own included, is
    /// `TEMPLATE_EXISTS`, and an unknown template `TEMPLATE_NOT_FOUND`.
    pub fn copy_template(
        &self,
        organization_id: &str,
        name: &str,
        to: &str,
    ) -> Result<MessageTemplate, AppError> {
        let from = template_name(name)?;
        let to = template_name(to)?;
        let source = self.template(organization_id, &from)?;
        self.refuse_taken(organization_id, &to)?;
        self.database.save_template(
            organization_id,
            &to,
            source.subject.as_deref(),
            &source.body,
        )
    }

    /// `TEMPLATE_EXISTS` when a template carries `name` already.
    fn refuse_taken(&self, organization_id: &str, name: &str) -> Result<(), AppError> {
        match self.database.template(organization_id, name)? {
            Some(other) => Err(AppError::conflict(
                "TEMPLATE_EXISTS",
                format!(
                    "a template {:?} exists already; rename or delete it first, or choose another name",
                    other.name
                ),
            )),
            None => Ok(()),
        }
    }

    /// Delete the template and answer what it held.
    pub fn delete_template(
        &self,
        organization_id: &str,
        name: &str,
    ) -> Result<MessageTemplate, AppError> {
        let name = template_name(name)?;
        match self.database.delete_template(organization_id, &name)? {
            Some(template) => Ok(template),
            None => Err(self.unknown(organization_id, &name)?),
        }
    }

    fn unknown(&self, organization_id: &str, name: &str) -> Result<AppError, AppError> {
        let names: Vec<String> = self
            .database
            .templates(organization_id)?
            .into_iter()
            .map(|template| template.name)
            .collect();
        let known = match names.is_empty() {
            true => "there are none yet".to_string(),
            false => format!("there are: {}", names.join(", ")),
        };
        Ok(AppError::new(
            StatusCode::NOT_FOUND,
            "TEMPLATE_NOT_FOUND",
            format!("there is no template {name:?}; {known}"),
            false,
        ))
    }
}

/// A template as it will be kept: its name as stored and its subject
/// trimmed. A text with nothing visible, or a subject that is blank or holds
/// a line break, is `TEMPLATE_INVALID`, as is a name `template_name` refuses.
fn checked_template<'a>(
    name: &str,
    subject: Option<&'a str>,
    body: &str,
) -> Result<(String, Option<&'a str>), AppError> {
    let name = template_name(name)?;
    if body.trim().is_empty() {
        return Err(AppError::invalid(
            "TEMPLATE_INVALID",
            format!("template {name:?} holds no visible text; write its text"),
        ));
    }
    let subject = subject.map(str::trim);
    if let Some(written) = subject {
        if written.is_empty() || written.contains(['\r', '\n']) {
            return Err(AppError::invalid(
                "TEMPLATE_INVALID",
                format!("template {name:?} has the subject {written:?}; a subject is one line of visible text, or leave it out"),
            ));
        }
    }
    Ok((name, subject))
}

/// A template's name as stored: trimmed, not empty, without a control
/// character or a `/`, which would not survive a URL path.
fn template_name(name: &str) -> Result<String, AppError> {
    let name = name.trim();
    if name.is_empty() || name.contains('/') || name.chars().any(char::is_control) {
        return Err(AppError::invalid(
            "TEMPLATE_INVALID",
            format!("{name:?} is not a template name; name it with visible text, without a / or a control character"),
        ));
    }
    Ok(name.to_string())
}
