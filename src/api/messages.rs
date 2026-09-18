//! Message routes: stored messages, replies and outbound mail.

use super::parse_uuid;
use crate::{
    auth::{AuthContext, OrganizationRole},
    error::AppError,
    models::{CreateOutboundRequest, CreateReplyRequest},
    service::AppState,
};
use axum::{
    extract::{Extension, Path, Query, State},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
pub(super) struct MessageQuery {
    mailbox_id: Option<String>,
    limit: Option<u32>,
    offset: Option<u32>,
}

pub(super) async fn list_messages(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Query(query): Query<MessageQuery>,
) -> Result<Json<Value>, AppError> {
    let mailbox_id = query.mailbox_id.as_deref().map(parse_uuid).transpose()?;
    Ok(Json(json!(state.list_messages(
        &auth.organization_id,
        mailbox_id,
        query.limit.unwrap_or(100),
        query.offset.unwrap_or(0),
    )?)))
}

pub(super) async fn get_message(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(
        state.get_message(&auth.organization_id, parse_uuid(&id)?)?
    )))
}

pub(super) async fn list_replies(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(
        state.list_replies(&auth.organization_id, parse_uuid(&id)?,)?
    )))
}

pub(super) async fn create_reply(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
    Json(request): Json<CreateReplyRequest>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(
        state
            .reply(&auth.organization_id, parse_uuid(&id)?, request)
            .await?
    )))
}

#[derive(Deserialize)]
pub(super) struct OutboundQuery {
    mailbox_id: Option<String>,
    limit: Option<u32>,
    offset: Option<u32>,
}

pub(super) async fn list_outbound(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Query(query): Query<OutboundQuery>,
) -> Result<Json<Value>, AppError> {
    let mailbox_id = query.mailbox_id.as_deref().map(parse_uuid).transpose()?;
    Ok(Json(json!(state.list_outbound(
        &auth.organization_id,
        mailbox_id,
        query.limit.unwrap_or(100),
        query.offset.unwrap_or(0),
    )?)))
}

pub(super) async fn list_mailbox_outbound(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
    Query(query): Query<OutboundQuery>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(state.list_outbound(
        &auth.organization_id,
        Some(parse_uuid(&id)?),
        query.limit.unwrap_or(100),
        query.offset.unwrap_or(0),
    )?)))
}

pub(super) async fn get_outbound(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(
        state.get_outbound(&auth.organization_id, parse_uuid(&id)?)?
    )))
}

pub(super) async fn create_outbound(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
    Json(request): Json<CreateOutboundRequest>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(
        state
            .send_outbound(&auth.organization_id, parse_uuid(&id)?, request)
            .await?
    )))
}
