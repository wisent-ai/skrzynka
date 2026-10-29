//! Message action routes: what an operator asks Skrzynka to change about a
//! stored message at its provider, and the record of those requests.

use super::parse_uuid;
use crate::{
    auth::{AuthContext, OrganizationRole},
    error::AppError,
    mail::{MarkMessageRequest, MoveMessageRequest, ProviderAction},
    service::AppState,
};
use axum::{
    extract::{Extension, Path, State},
    Json,
};
use serde_json::{json, Value};

/// `POST /v1/messages/:id/mark` with `{"as": "seen" | "unseen" | "flagged" | "unflagged"}`.
pub(super) async fn mark_message(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
    Json(request): Json<MarkMessageRequest>,
) -> Result<Json<Value>, AppError> {
    act(state, auth, &id, request.mark_as.action()).await
}

/// `POST /v1/messages/:id/move` with `{"folder": "<provider folder>"}` or
/// `{"to": "archive" | "trash" | "junk" | "all"}`.
pub(super) async fn move_message(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
    Json(request): Json<MoveMessageRequest>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(
        state
            .move_message(
                &auth.organization_id,
                parse_uuid(&id)?,
                request.folder,
                request.to
            )
            .await?
    )))
}

/// `POST /v1/messages/:id/delete`: delete the message at its provider.
pub(super) async fn delete_message(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    act(state, auth, &id, ProviderAction::Delete).await
}

/// `GET /v1/messages/:id/actions`: every action asked of the message, newest first.
pub(super) async fn list_message_actions(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(state.list_message_actions(
        &auth.organization_id,
        parse_uuid(&id)?
    )?)))
}

/// Changing mail at the provider is a member's act, as sending is.
async fn act(
    state: AppState,
    auth: AuthContext,
    id: &str,
    action: ProviderAction,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(
        state
            .act_on_message(&auth.organization_id, parse_uuid(id)?, action)
            .await?
    )))
}

/// `POST /v1/views/:name/mark` with `{"as": "seen" | "unseen" | "flagged" | "unflagged"}`: mark every
/// message the saved view lists; answers `{view, actions, refused}`.
pub(super) async fn mark_view(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(name): Path<String>,
    Json(request): Json<MarkMessageRequest>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(
        state
            .act_on_view(&auth.organization_id, &name, request.mark_as.action())
            .await?
    )))
}

/// `POST /v1/views/:name/move` with `{"folder": "<provider folder>"}` or
/// `{"to": "archive" | "trash" | "junk" | "all"}`.
pub(super) async fn move_view(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(name): Path<String>,
    Json(request): Json<MoveMessageRequest>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(
        state
            .move_view(&auth.organization_id, &name, request.folder, request.to)
            .await?
    )))
}

/// `POST /v1/messages/:id/thread/mark` with `{"as": …}`: mark every message
/// of the conversation; answers `{thread, actions, refused}`.
pub(super) async fn mark_thread(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
    Json(request): Json<MarkMessageRequest>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    let id = parse_uuid(&id)?;
    Ok(Json(json!(
        state
            .act_on_thread(&auth.organization_id, id, request.mark_as.action())
            .await?
    )))
}

/// `POST /v1/messages/:id/thread/move` with `{"folder": …}` or `{"to": …}`:
/// move every message of the conversation, such as to archive it.
pub(super) async fn move_thread(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
    Json(request): Json<MoveMessageRequest>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    let id = parse_uuid(&id)?;
    Ok(Json(json!(
        state
            .move_thread(&auth.organization_id, id, request.folder, request.to)
            .await?
    )))
}
