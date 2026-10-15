//! The receiving feed: the mail a receiving provider accepted for an inbound
//! domain, read through wisent-integrations (`content/resend.receiving.list`
//! and `.get`), which holds the provider key. Skrzynka holds only its own
//! integration bearer and the service address, both in the Skarbiec item that
//! plays the role below.

use crate::{error::AppError, skarbiec::SkarbiecResolver};
use serde_json::{json, Value};

/// The Skarbiec item Skrzynka's wisent-integrations grant lives in, asked of
/// Skarbiec by its role: `token` is the bearer, `url` the service address.
const INTEGRATION_ROLE: &str = "role:skrzynka-integration-api";

/// One received message as the feed lists it. `subject` is absent when the
/// message states none; nothing is written in for it.
pub(super) struct Listed {
    pub id: String,
    pub sender: String,
    pub recipients: Vec<String>,
    pub subject: Option<String>,
    pub received_at: String,
}

pub(super) struct Feed {
    base: String,
    token: String,
    client: reqwest::Client,
}

impl Feed {
    /// The feed as Skrzynka's grant reaches it; refused, naming the role and
    /// the field, when the item is missing or incomplete.
    pub(super) async fn from_vault(resolver: &SkarbiecResolver) -> Result<Self, AppError> {
        let item = resolver.get_item(INTEGRATION_ROLE).await?;
        let field = |name: &str| -> Result<String, AppError> {
            item.pointer(&format!("/fields/{name}"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
                .ok_or_else(|| {
                    AppError::dependency(
                        "INBOUND_FEED_UNCONFIGURED",
                        format!(
                            "the Skarbiec item playing {INTEGRATION_ROLE} has no field '{name}'; it holds Skrzynka's wisent-integrations bearer (token) and the service address (url)"
                        ),
                        false,
                    )
                })
        };
        Ok(Self {
            base: field("url")?,
            token: field("token")?,
            client: reqwest::Client::new(),
        })
    }

    /// One page of the feed, newest first, after the provider id `after`;
    /// answers the page and whether an older page follows.
    pub(super) async fn page(&self, after: Option<&str>) -> Result<(Vec<Listed>, bool), AppError> {
        let mut request = json!({});
        if let Some(after) = after {
            request["after"] = json!(after);
        }
        let action = "resend.receiving.list";
        let answer = self.action(action, request).await?;
        let rows = answer
            .get("data")
            .and_then(Value::as_array)
            .ok_or_else(|| shape(action, "no data array", &answer))?;
        let listed = rows
            .iter()
            .map(|row| {
                listed(row).ok_or_else(|| {
                    shape(
                        action,
                        "a row without id, from, created_at or a readable to",
                        row,
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let more = answer
            .get("has_more")
            .and_then(Value::as_bool)
            .ok_or_else(|| shape(action, "no has_more", &answer))?;
        Ok((listed, more))
    }

    /// One received message's plain text; its HTML when it has no text part;
    /// nothing when it has neither.
    pub(super) async fn body(&self, id: &str) -> Result<String, AppError> {
        let answer = self
            .action("resend.receiving.get", json!({ "id": id }))
            .await?;
        let part = |name: &str| answer.get(name).and_then(Value::as_str).map(str::to_string);
        Ok(match (part("text"), part("html")) {
            (Some(text), _) => text,
            (None, Some(html)) => html,
            (None, None) => String::new(),
        })
    }

    async fn action(&self, action: &str, body: Value) -> Result<Value, AppError> {
        let endpoint = format!(
            "{}/api/integration/content/{action}",
            self.base.trim_end_matches('/')
        );
        let builder = self
            .client
            .post(&endpoint)
            .bearer_auth(&self.token)
            .json(&body);
        let response = stado_wait::request(builder).await.map_err(|error| {
            AppError::dependency(
                "INBOUND_FEED_UNAVAILABLE",
                format!("{endpoint} could not be reached: {error}"),
                true,
            )
        })?;
        let status = response.status();
        let text = response.text().await.map_err(|error| {
            AppError::dependency(
                "INBOUND_FEED_UNAVAILABLE",
                format!(
                    "{endpoint} answered HTTP {status} but its body could not be read: {error}"
                ),
                true,
            )
        })?;
        let envelope: Value = serde_json::from_str(&text).map_err(|error| {
            AppError::dependency(
                "INBOUND_FEED_REFUSED",
                format!("{endpoint} answered HTTP {status} with a body that is not JSON ({error}): {text}"),
                false,
            )
        })?;
        match (
            status.is_success(),
            envelope.get("ok"),
            envelope.get("result"),
        ) {
            (true, Some(Value::Bool(true)), Some(result)) => Ok(result.clone()),
            _ => Err(AppError::dependency(
                "INBOUND_FEED_REFUSED",
                format!("{endpoint} answered HTTP {status}: {envelope}"),
                status.is_server_error(),
            )),
        }
    }
}

/// A feed row, or nothing when it lacks a field the store needs or carries a
/// recipient that is no address: an unreadable recipient refuses the row
/// instead of being dropped from its list.
fn listed(row: &Value) -> Option<Listed> {
    let text = |name: &str| row.get(name).and_then(Value::as_str).map(str::to_string);
    let recipients = match row.get("to")? {
        Value::Array(entries) => entries.iter().map(address).collect::<Option<Vec<_>>>()?,
        single => vec![address(single)?],
    };
    Some(Listed {
        id: text("id")?,
        sender: text("from")?,
        recipients,
        subject: text("subject"),
        received_at: text("created_at")?,
    })
}

fn address(value: &Value) -> Option<String> {
    value
        .as_str()
        .or_else(|| value.get("email").and_then(Value::as_str))
        .map(str::to_string)
}

fn shape(action: &str, what: &str, seen: &Value) -> AppError {
    AppError::dependency(
        "INBOUND_FEED_REFUSED",
        format!("wisent-integrations {action} answered {what}: {seen}"),
        false,
    )
}
