//! Gmail routes: OAuth start, callback and status; delegated and app-password connection.

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

pub(super) async fn gmail_oauth_callback(
    State(state): State<AppState>,
    Query(callback): Query<GmailOAuthCallback>,
) -> (StatusCode, Html<&'static str>) {
    match state.complete_gmail_oauth_callback(callback).await {
        Ok(_) => (
            StatusCode::OK,
            Html("<!doctype html><meta charset=utf-8><title>Gmail connected</title><p>Gmail is connected to Skrzynka. You can close this window and return to Skrzynka Desktop.</p>"),
        ),
        Err(_) => (
            StatusCode::BAD_REQUEST,
            Html("<!doctype html><meta charset=utf-8><title>Gmail connection failed</title><p>Gmail could not be connected. Return to Skrzynka Desktop for the exact error.</p>"),
        ),
    }
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
