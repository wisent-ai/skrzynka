//! Mailbox routes: list, create, import, read, update, delete and sync.

use super::parse_uuid;
use crate::{
    auth::{AuthContext, OrganizationRole},
    error::AppError,
    models::{CreateMailboxRequest, UpdateMailboxRequest},
    service::AppState,
};
use axum::{
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

pub(super) async fn list_mailboxes(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(state.list_mailboxes(&auth.organization_id)?)))
}

pub(super) async fn create_mailbox(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Json(request): Json<CreateMailboxRequest>,
) -> Result<(StatusCode, Json<Value>), AppError> {
    auth.require_role(OrganizationRole::Admin)?;
    let mailbox = state.create_mailbox(&auth.organization_id, request).await?;
    Ok((StatusCode::CREATED, Json(json!(mailbox))))
}

pub(super) async fn import_mailbox(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Json(request): Json<CreateMailboxRequest>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Admin)?;
    Ok(Json(json!(
        state.import_mailbox(&auth.organization_id, request).await?
    )))
}

pub(super) async fn get_mailbox(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(
        state.get_mailbox(&auth.organization_id, parse_uuid(&id)?)?
    )))
}

pub(super) async fn update_mailbox(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
    Json(request): Json<UpdateMailboxRequest>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Admin)?;
    Ok(Json(json!(state.update_mailbox(
        &auth.organization_id,
        parse_uuid(&id)?,
        request,
    )?)))
}

#[derive(Deserialize)]
pub(super) struct DeleteQuery {
    confirm: Option<bool>,
}

pub(super) async fn delete_mailbox(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
    Query(query): Query<DeleteQuery>,
) -> Result<StatusCode, AppError> {
    auth.require_role(OrganizationRole::Admin)?;
    if query.confirm != Some(true) {
        return Err(AppError::invalid(
            "CONFIRMATION_REQUIRED",
            "mailbox removal requires confirm=true",
        ));
    }
    state.delete_mailbox(&auth.organization_id, parse_uuid(&id)?)?;
    Ok(StatusCode::NO_CONTENT)
}

pub(super) async fn sync_mailbox(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(
        state
            .sync_mailbox(&auth.organization_id, parse_uuid(&id)?)
            .await?
    )))
}

pub(super) async fn sync_all(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(state.sync_all(&auth.organization_id).await?)))
}
