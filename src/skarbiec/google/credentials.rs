//! The Google OAuth client and service account Skrzynka reads from Skarbiec.

use serde_json::Value;

use super::super::{
    invalid_item, optional_text, required_text, GoogleOAuthClient, GoogleServiceAccount,
    SkarbiecResolver, GOOGLE_OAUTH_CLIENT_ITEM_ID, GOOGLE_SERVICE_ACCOUNT_ITEM_ID,
    GOOGLE_TOKEN_URI,
};
use crate::error::AppError;

/// The authorization endpoint of Google's installed-application OAuth flow.
const GOOGLE_AUTH_URI: &str = "https://accounts.google.com/o/oauth2/auth";

/// An endpoint a credential may state only as the canonical one: a stated
/// other endpoint is refused, and one it does not state is the canonical one.
fn canonical_endpoint(
    stated: Option<String>,
    canonical: &str,
    refusal: &str,
) -> Result<String, AppError> {
    match stated {
        Some(uri) if uri != canonical => Err(invalid_item(refusal)),
        _ => Ok(canonical.to_string()),
    }
}

impl SkarbiecResolver {
    /// The JSON document a Google credential item carries in `fields.value`,
    /// either as text or wrapped as `{"value": text}`.
    async fn google_document(&self, item_id: &str, label: &str) -> Result<Value, AppError> {
        let payload = self.get_item(item_id).await?;
        let wrapped = payload
            .pointer("/fields/value")
            .ok_or_else(|| invalid_item(format!("{label} item has no fields.value")))?;
        let raw = wrapped
            .get("value")
            .and_then(Value::as_str)
            .or_else(|| wrapped.as_str())
            .ok_or_else(|| invalid_item(format!("{label} value is not text")))?;
        serde_json::from_str(raw)
            .map_err(|error| invalid_item(format!("{label} value is invalid JSON: {error}")))
    }

    pub(crate) async fn google_oauth_client(&self) -> Result<GoogleOAuthClient, AppError> {
        let document = self
            .google_document(GOOGLE_OAUTH_CLIENT_ITEM_ID, "Google OAuth client")
            .await?;
        let profile = document
            .get("installed")
            .and_then(Value::as_object)
            .ok_or_else(|| {
                invalid_item("Google OAuth client is not a desktop application client")
            })?;
        let refusal = "Google OAuth client endpoints are not canonical";
        let auth_uri = canonical_endpoint(
            optional_text(profile.get("auth_uri")),
            GOOGLE_AUTH_URI,
            refusal,
        )?;
        let token_uri = canonical_endpoint(
            optional_text(profile.get("token_uri")),
            GOOGLE_TOKEN_URI,
            refusal,
        )?;
        Ok(GoogleOAuthClient {
            client_id: required_text(profile.get("client_id"), "client_id")?,
            client_secret: required_text(profile.get("client_secret"), "client_secret")?,
            auth_uri,
            token_uri,
        })
    }

    pub(crate) async fn google_service_account(&self) -> Result<GoogleServiceAccount, AppError> {
        let document = self
            .google_document(GOOGLE_SERVICE_ACCOUNT_ITEM_ID, "Google service account")
            .await?;
        if document.get("type").and_then(Value::as_str) != Some("service_account") {
            return Err(invalid_item(
                "Google service account JSON is not a service_account key",
            ));
        }
        let token_uri = canonical_endpoint(
            document
                .get("token_uri")
                .and_then(Value::as_str)
                .map(str::to_string),
            GOOGLE_TOKEN_URI,
            "Google service account token endpoint is not canonical",
        )?;
        let fields = document
            .as_object()
            .ok_or_else(|| invalid_item("Google service account JSON is not an object"))?;
        Ok(GoogleServiceAccount {
            client_email: required_text(fields.get("client_email"), "client_email")?,
            client_id: required_text(fields.get("client_id"), "client_id")?,
            private_key: required_text(fields.get("private_key"), "private_key")?,
            private_key_id: optional_text(fields.get("private_key_id")),
            token_uri,
        })
    }
}
