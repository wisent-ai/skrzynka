//! Inbound domain routes: declare, list, read the feed, remove, and the mail
//! one address received.

use super::parse_uuid;
use crate::{
    auth::{AuthContext, OrganizationRole},
    error::AppError,
    service::AppState,
};
use axum::{
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
pub(super) struct DeclareRequest {
    domain: String,
}

pub(super) async fn declare_inbound_domain(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Json(request): Json<DeclareRequest>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Admin)?;
    Ok(Json(json!(state.declare_inbound_domain(
        &auth.organization_id,
        &request.domain
    )?)))
}

pub(super) async fn list_inbound_domains(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(
        state.list_inbound_domains(&auth.organization_id)?
    )))
}

pub(super) async fn sync_inbound_domain(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(
        state
            .sync_inbound_domain(&auth.organization_id, parse_uuid(&id)?)
            .await?
    )))
}

#[derive(Deserialize)]
pub(super) struct RemoveQuery {
    confirm: Option<bool>,
}

pub(super) async fn remove_inbound_domain(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
    Query(query): Query<RemoveQuery>,
) -> Result<StatusCode, AppError> {
    auth.require_role(OrganizationRole::Admin)?;
    state.remove_inbound_domain(
        &auth.organization_id,
        parse_uuid(&id)?,
        query.confirm == Some(true),
    )?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub(super) struct ReadQuery {
    recipient: Option<String>,
    sender: Option<String>,
    limit: Option<u32>,
}

pub(super) async fn read_inbound(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Query(query): Query<ReadQuery>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(
        state
            .read_inbound(
                &auth.organization_id,
                query.recipient.as_deref(),
                query.sender.as_deref(),
                query.limit,
            )
            .await?
    )))
}
