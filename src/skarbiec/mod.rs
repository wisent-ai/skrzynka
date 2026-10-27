//! Skarbiec, the vault every credential lives in: the resolver type, its constants and the
//! item-shape helpers; the sub-modules extend it with the CLI transport, item reads and
//! writes, mailbox resolution and Google token minting.

use crate::{error::AppError, models::MAX_HOST_LENGTH};
use chrono::Utc;
use reqwest::Client;
use serde_json::Value;
use std::{collections::HashMap, path::PathBuf, sync::Arc};
use tokio::sync::Mutex;

mod cli;
mod google;
mod items;
mod local;
mod mailboxes;

pub use mailboxes::MAILBOX_TAG;

/// The Google desktop OAuth client, asked of Skarbiec by the role its item
/// plays (`skarbiec get role:<role>` reads the one live item tagged
/// `stado:role:<role>`), so no item id is written here.
const GOOGLE_OAUTH_CLIENT_ITEM_ID: &str = "role:skrzynka-google-oauth-desktop";
/// The delegated-mail service account, asked of Skarbiec by its role.
const GOOGLE_SERVICE_ACCOUNT_ITEM_ID: &str = "role:skrzynka-google-service-account";
const GOOGLE_TOKEN_URI: &str = "https://oauth2.googleapis.com/token";
pub const GMAIL_DELEGATION_SCOPE: &str = "https://mail.google.com/";
pub const GOOGLE_ADMIN_DELEGATION_URL: &str =
    "https://admin.google.com/ac/owl/domainwidedelegation";

pub enum ResolvedCredentials {
    Password {
        username: String,
        password: String,
    },
    OAuth2 {
        username: String,
        access_token: String,
    },
}

#[derive(Clone)]
struct CachedAccessToken {
    value: String,
    expires_at: chrono::DateTime<Utc>,
    credential_fingerprint: sha2::digest::Output<sha2::Sha256>,
}

#[derive(Clone)]
pub(crate) struct GoogleOAuthClient {
    pub client_id: String,
    pub client_secret: String,
    pub auth_uri: String,
    pub token_uri: String,
}

/// The delegated-mail service account read from Skarbiec. `client_id` is the
/// numeric OAuth2 client the Workspace admin must grant; `client_email` is the
/// JWT issuer.
#[derive(Clone)]
pub(crate) struct GoogleServiceAccount {
    pub client_email: String,
    pub client_id: String,
    pub private_key: String,
    pub private_key_id: Option<String>,
    pub token_uri: String,
}

#[derive(Clone)]
pub struct SkarbiecResolver {
    binary: PathBuf,
    /// `SKRZYNKA_CREDENTIALS_FILE`, which answers in Skarbiec's place on a
    /// machine without it; `None` asks the Skarbiec CLI at `binary`.
    local: Option<PathBuf>,
    client: Client,
    token_cache: Arc<Mutex<HashMap<String, CachedAccessToken>>>,
}

impl SkarbiecResolver {
    pub fn new(binary: impl Into<PathBuf>) -> Self {
        Self {
            binary: binary.into(),
            local: local::configured(),
            client: Client::new(),
            token_cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Whether Skarbiec answers `skarbiec version`. A refusal carries the exit
    /// status and what Skarbiec wrote, and a command that never ran carries
    /// why, so a missing binary, a refusal and a crash are told apart.
    pub async fn availability(&self) -> Result<(), AppError> {
        let output = self.output(&["version"]).await?;
        if output.status.success() {
            return Ok(());
        }
        Err(AppError::dependency(
            "SKARBIEC_UNAVAILABLE",
            format!(
                "skarbiec version exited with {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            ),
            true,
        ))
    }
}

pub(super) fn required_text(value: Option<&Value>, name: &str) -> Result<String, AppError> {
    let value = value
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| invalid_item(format!("item field {name} is required and must be text")))?;
    Ok(value.to_string())
}

pub(super) fn optional_text(value: Option<&Value>) -> Option<String> {
    value
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// The port an item field states, as a JSON number or its decimal text. A
/// value that is no port, zero included (nothing listens there), is refused
/// with the value and why it is none.
pub(super) fn port_field(value: &Value, name: &str) -> Result<u16, AppError> {
    let port = match value {
        Value::Number(number) => {
            let whole = number.as_u64().ok_or_else(|| {
                profile_error(format!(
                    "Skarbiec item {name} {number} is not a whole non-negative number"
                ))
            })?;
            u16::try_from(whole).map_err(|cause| {
                profile_error(format!(
                    "Skarbiec item {name} {whole} is not a port: {cause}"
                ))
            })?
        }
        Value::String(text) => text.trim().parse::<u16>().map_err(|cause| {
            profile_error(format!(
                "Skarbiec item {name} '{text}' is not a port: {cause}"
            ))
        })?,
        other => {
            return Err(profile_error(format!(
                "Skarbiec item {name} must be a number or its decimal text, not {other}"
            )));
        }
    };
    std::num::NonZeroU16::new(port)
        .map(std::num::NonZeroU16::get)
        .ok_or_else(|| {
            profile_error(format!(
                "Skarbiec item {name} states port zero, which no service listens on"
            ))
        })
}

/// Whether a mailbox item's stored reference names `coordinate`: the role
/// coordinate itself, or the id of the item that plays that role now, which
/// is what a mailbox authorized before references named roles recorded.
pub(super) fn reference_names(
    reference: Option<&str>,
    coordinate: &str,
    items: &[crate::models::SkarbiecItemMetadata],
) -> bool {
    let Some(reference) = reference else {
        return false;
    };
    if reference == coordinate {
        return true;
    }
    items
        .iter()
        .any(|item| item.id == reference && item.matches_coordinate(coordinate))
}

pub(super) fn validate_item_id(item_id: &str) -> Result<(), AppError> {
    if item_id.is_empty() || item_id.chars().any(char::is_whitespace) {
        return Err(profile_error(
            "skarbiec_item_id must contain non-whitespace characters only",
        ));
    }
    Ok(())
}

pub(super) fn validate_hostname(value: &str, field: &str) -> Result<(), AppError> {
    if value.is_empty()
        || value.len() > MAX_HOST_LENGTH
        || value.contains("://")
        || value.chars().any(char::is_whitespace)
    {
        return Err(profile_error(format!(
            "{field} must be a hostname without a URL scheme"
        )));
    }
    Ok(())
}

pub(super) fn invalid_item(message: impl Into<String>) -> AppError {
    AppError::dependency("SKARBIEC_ITEM_INVALID", message, false)
}

pub(super) fn profile_error(message: impl Into<String>) -> AppError {
    AppError::invalid("MAILBOX_PROFILE_INVALID", message)
}
