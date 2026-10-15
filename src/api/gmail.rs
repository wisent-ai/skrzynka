//! Gmail routes: OAuth start, callback, status and settled status; delegated and app-password connection; disconnection.

use super::parse_uuid;
use crate::{
    auth::{AuthContext, OrganizationRole},
    error::AppError,
    gmail::{GmailOAuthCallback, StartGmailOAuthRequest},
    service::AppState,
};
use axum::{
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    response::Html,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

pub(super) async fn start_gmail_oauth(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Json(request): Json<StartGmailOAuthRequest>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Admin)?;
    Ok(Json(json!(
        state
            .start_gmail_oauth(&auth.organization_id, request)
            .await?
    )))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct GmailConnectionQuery {
    email: Option<String>,
}

pub(super) async fn gmail_connection_readiness_handler(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Query(query): Query<GmailConnectionQuery>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(
        state
            .gmail_connection_readiness(&auth.organization_id, query.email.as_deref())
            .await?
    )))
}

#[derive(Deserialize)]
pub(super) struct DelegateGmailRequest {
    email: String,
    display_name: Option<String>,
}

pub(super) async fn connect_gmail_delegated(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Json(request): Json<DelegateGmailRequest>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Admin)?;
    Ok(Json(json!(
        state
            .connect_gmail_delegated(&auth.organization_id, &request.email, request.display_name)
            .await?
    )))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DisconnectGmailRequest {
    email: String,
}

/// The API side of `skrzynka account disconnect`.
pub(super) async fn disconnect_gmail(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Json(request): Json<DisconnectGmailRequest>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Admin)?;
    Ok(Json(state.disconnect_gmail(&request.email).await?))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ConnectGmailAppPasswordRequest {
    skarbiec_item_id: String,
    display_name: Option<String>,
}

pub(super) async fn connect_gmail_app_password(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Json(request): Json<ConnectGmailAppPasswordRequest>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Admin)?;
    Ok(Json(json!(
        state
            .connect_gmail_app_password_item(
                &auth.organization_id,
                &request.skarbiec_item_id,
                request.display_name,
            )
            .await?
    )))
}

/// The page Google's redirect lands on. A refused callback names the core's
/// own code and sentence, the same refusal Skrzynka Desktop then reads from
/// the flow.
pub(super) async fn gmail_oauth_callback(
    State(state): State<AppState>,
    Query(callback): Query<GmailOAuthCallback>,
) -> (StatusCode, Html<String>) {
    match state.complete_gmail_oauth_callback(callback).await {
        Ok(_) => (
            StatusCode::OK,
            Html(String::from("<!doctype html><meta charset=utf-8><title>Gmail connected</title><p>Gmail is connected to Skrzynka. You can close this window and return to Skrzynka Desktop.</p>")),
        ),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Html(format!(
                "<!doctype html><meta charset=utf-8><title>Gmail connection failed</title><p>Gmail could not be connected: {}</p><p>Skrzynka Desktop shows the same refusal.</p>",
                escape_html(&error.to_string())
            )),
        ),
    }
}

/// Text placed inside the callback page's markup, with the characters HTML
/// would read as markup written as references.
fn escape_html(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            other => escaped.push(other),
        }
    }
    escaped
}

pub(super) async fn gmail_oauth_status(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(flow_id): Path<String>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(
        state
            .gmail_oauth_status(&auth.organization_id, parse_uuid(&flow_id)?)
            .await?
    )))
}

/// The flow's status once Google's callback has completed or failed it. The
/// answer waits for the callback, so a client reads the outcome once instead
/// of asking again on a clock; a client that stops waiting closes the
/// connection, which drops this wait. A flow the core no longer holds is the
/// same not-found refusal the status read gives.
pub(super) async fn gmail_oauth_settled(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(flow_id): Path<String>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(
        state
            .gmail_oauth_settled(&auth.organization_id, parse_uuid(&flow_id)?)
            .await?
    )))
}
