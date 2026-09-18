//! Skarbiec, the vault every credential lives in: the resolver type, its constants and the
//! item-shape helpers; the sub-modules extend it with the CLI transport, item reads and
//! writes, mailbox resolution and Google token minting.

use crate::{error::AppError, models::{MAX_HOST_LENGTH, MAX_ITEM_ID_LENGTH}};
use chrono::Utc;
use reqwest::Client;
use serde_json::Value;
use std::{collections::HashMap, path::PathBuf, sync::Arc};
use tokio::sync::Mutex;

mod cli;
mod google;
mod items;
mod mailboxes;

const MAX_SKARBIEC_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
/// A mailbox is polled every minute unless its profile says otherwise.
const DEFAULT_POLL_INTERVAL_SECONDS: u64 = 60;
/// A cached token is reused only while it has more than a minute left.
const TOKEN_EXPIRY_MARGIN_SECONDS: i64 = 60;
/// The Skarbiec CLI answers within seconds; longer than this is a stuck vault.
const SKARBIEC_COMMAND_TIMEOUT_SECONDS: u64 = 15;
const GOOGLE_OAUTH_CLIENT_ITEM_ID: &str = "skrzynka-google-oauth-desktop";
const GOOGLE_SERVICE_ACCOUNT_ITEM_ID: &str = "skrzynka-google-service-account";
const GOOGLE_TOKEN_URI: &str = "https://oauth2.googleapis.com/token";
const GMAIL_DELEGATION_SCOPE: &str = "https://mail.google.com/";
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
    client: Client,
    token_cache: Arc<Mutex<HashMap<String, CachedAccessToken>>>,
}

impl SkarbiecResolver {
    pub fn new(binary: impl Into<PathBuf>) -> Self {
        Self {
            binary: binary.into(),
            client: Client::new(),
            token_cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn is_available(&self) -> bool {
        self.output(&["version"])
            .await
            .map(|output| output.status.success())
            .unwrap_or(false)
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

pub(super) fn optional_port(value: Option<&Value>) -> Option<u16> {
    value.and_then(|value| {
        value
            .as_u64()
            .and_then(|number| u16::try_from(number).ok())
            .or_else(|| value.as_str().and_then(|text| text.parse::<u16>().ok()))
    })
}

pub(super) fn looks_like_google_profile(item_id: &str, email: &str, payload: &Value) -> bool {
    let id = item_id.to_ascii_lowercase();
    let domain = email
        .rsplit_once('@')
        .map(|(_, domain)| domain.to_ascii_lowercase());
    let context_mentions_google = payload
        .get("context")
        .and_then(|value| serde_json::to_string(value).ok())
        .is_some_and(|value| value.to_ascii_lowercase().contains("google"));
    domain.as_deref() == Some("gmail.com")
        || id.contains("google")
        || id.contains("gmail")
        || context_mentions_google
}

pub(super) fn profile_preference(item_id: &str) -> u8 {
    let id = item_id.to_ascii_lowercase();
    u8::from(id.contains("gmail")) * 2 + u8::from(id.contains("google"))
}

pub(super) fn validate_item_id(item_id: &str) -> Result<(), AppError> {
    if item_id.is_empty()
        || item_id.len() > MAX_ITEM_ID_LENGTH
        || item_id.chars().any(char::is_whitespace)
    {
        return Err(profile_error(
            "skarbiec_item_id must contain 1 to 256 non-whitespace characters",
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
