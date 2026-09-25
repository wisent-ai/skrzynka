//! Mailbox routes: the mailboxes Skarbiec declares, the declaration itself,
//! local mail removal, and sync.

use super::parse_uuid;
use crate::{
    auth::{AuthContext, OrganizationRole},
    error::AppError,
    models::DeclareMailboxRequest,
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
    Ok(Json(json!(
        state.list_mailboxes(&auth.organization_id).await?
    )))
}

pub(super) async fn declare_mailbox(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Json(request): Json<DeclareMailboxRequest>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Admin)?;
    Ok(Json(json!(
        state
            .declare_mailbox(&auth.organization_id, &request.skarbiec_item_id)
            .await?
    )))
}

pub(super) async fn undeclare_mailbox(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Admin)?;
    Ok(Json(json!(
        state
            .undeclare_mailbox(&auth.organization_id, parse_uuid(&id)?)
            .await?
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
