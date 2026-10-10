use crate::error::AppError;
use chrono::{Duration as ChronoDuration, Utc};
use serde_json::Value;

pub(super) fn refresh_refusal(status: reqwest::StatusCode, payload: &Value) -> AppError {
    let code = payload.get("error").and_then(Value::as_str);
    let description = payload.get("error_description").and_then(Value::as_str);
    let observed = format!("Google refresh-token request was refused with HTTP {status}; error={code:?}; error_description={description:?}");
    if code == Some("invalid_grant") {
        AppError::dependency(
            "GMAIL_AUTHORIZATION_EXPIRED",
            format!("{observed}; reconnect the profile"),
            false,
        )
    } else {
        AppError::dependency(
            "GMAIL_TOKEN_REFRESH_UNAVAILABLE",
            observed,
            status.is_server_error() || status == reqwest::StatusCode::TOO_MANY_REQUESTS,
        )
    }
}

pub(super) fn token_expiry(
    payload: &Value,
    requested_at: chrono::DateTime<Utc>,
) -> Result<Option<chrono::DateTime<Utc>>, AppError> {
    let Some(value) = payload.get("expires_in") else {
        return Ok(None);
    };
    let seconds = value.as_i64().ok_or_else(|| {
        AppError::dependency(
            "GMAIL_TOKEN_RESPONSE_INVALID",
            format!("Google expires_in must be an integer number of seconds, received {value}"),
            false,
        )
    })?;
    ChronoDuration::try_seconds(seconds)
        .and_then(|duration| requested_at.checked_add_signed(duration))
        .map(Some)
        .ok_or_else(|| {
            AppError::dependency(
                "GMAIL_TOKEN_RESPONSE_INVALID",
                format!("Google expires_in {seconds} cannot be represented as a token expiry"),
                false,
            )
        })
}
