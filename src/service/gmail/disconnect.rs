//! `account disconnect`: the inverse of every Gmail connection path. The
//! credential item a path wrote into Skarbiec goes to Skarbiec's recoverable
//! trash; an OAuth grant is revoked at Google first, so a deleted item leaves
//! no live refresh token behind. A mailbox still declared on the item is
//! refused: `mailbox undeclare` comes first, and `mailbox remove` deletes
//! the mail kept for it.

use super::super::AppState;
use crate::{
    error::AppError,
    skarbiec::{SkarbiecResolver, MAILBOX_TAG},
};
use serde_json::{json, Value};

/// Google's OAuth 2.0 token revocation endpoint (RFC 7009).
const GOOGLE_REVOKE_URL: &str = "https://oauth2.googleapis.com/revoke";

impl AppState {
    pub async fn disconnect_gmail(&self, email: &str) -> Result<Value, AppError> {
        let candidates = [
            SkarbiecResolver::gmail_item_id(email),
            SkarbiecResolver::gmail_app_password_item_id(email)?,
        ];
        let held: Vec<_> = self
            .resolver
            .list_items()
            .await?
            .into_iter()
            .filter(|item| candidates.contains(&item.id))
            .collect();
        if held.is_empty() {
            return Err(AppError::invalid(
                "GMAIL_ACCOUNT_NOT_CONNECTED",
                format!("Skarbiec holds no Skrzynka credential for {email}; nothing to disconnect"),
            ));
        }
        if let Some(item) = held
            .iter()
            .find(|item| item.tags.iter().any(|tag| tag == MAILBOX_TAG))
        {
            return Err(AppError::conflict(
                "GMAIL_MAILBOX_DECLARED",
                format!(
                    "{email} is still a declared mailbox (Skarbiec item {}); run `skrzynka mailbox undeclare <ID>` with the ID `skrzynka mailbox list` prints, then disconnect",
                    item.id
                ),
            ));
        }
        let mut removed = Vec::new();
        for item in held {
            let payload = self.resolver.get_item(&item.id).await?;
            let method = payload
                .pointer("/fields/auth_method")
                .and_then(Value::as_str);
            let token = payload
                .pointer("/fields/refresh_token")
                .and_then(Value::as_str);
            let grant = match (method, token) {
                (Some("oauth2"), Some(token)) => revoke(token).await?,
                _ => "none",
            };
            self.resolver.delete_item(&item.id).await?;
            removed.push(json!({
                "item": item.id,
                "auth_method": method,
                "google_grant": grant,
            }));
        }
        Ok(json!({ "email": email, "removed": removed }))
    }
}

/// Revoke one refresh token. `already_invalid` is Google's `invalid_token`
/// answer: the grant was revoked or expired before. Any other answer keeps
/// the credential, so the token is never lost while it may still be live.
async fn revoke(token: &str) -> Result<&'static str, AppError> {
    let response = reqwest::Client::new()
        .post(GOOGLE_REVOKE_URL)
        .form(&[("token", token)])
        .send()
        .await
        .map_err(|error| {
            AppError::dependency(
                "GMAIL_REVOKE_UNAVAILABLE",
                format!("Google's revocation endpoint could not be reached ({error}); the credential was kept"),
                true,
            )
        })?;
    let status = response.status();
    if status.is_success() {
        return Ok("revoked");
    }
    let answer: Value = response.json().await.map_err(|error| {
        AppError::dependency(
            "GMAIL_REVOKE_FAILED",
            format!("Google refused the revocation with {status} and an unreadable body ({error}); the credential was kept"),
            status.is_server_error(),
        )
    })?;
    if answer.get("error").and_then(Value::as_str) == Some("invalid_token") {
        return Ok("already_invalid");
    }
    Err(AppError::dependency(
        "GMAIL_REVOKE_FAILED",
        format!("Google refused the revocation with {status}: {answer}; the credential was kept"),
        status.is_server_error(),
    ))
}
