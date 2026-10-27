//! Routes that read or act on one message as part of the operator's own
//! handling: its conversation, forwarding it, and snoozing it.

use super::parse_uuid;
use crate::{
    auth::{AuthContext, OrganizationRole},
    error::AppError,
    service::{AppState, ForwardRequest},
};
use axum::{
    extract::{Extension, Path, State},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

/// `GET /v1/messages/:id/thread`: the conversation the message belongs to.
pub(in crate::api) async fn get_thread(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(
        state.message_thread(&auth.organization_id, parse_uuid(&id)?)?
    )))
}

/// `POST /v1/messages/:id/forward`: send the message on, the original
/// attached, from the mailbox that received it.
pub(in crate::api) async fn forward_message(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
    Json(request): Json<ForwardRequest>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(
        state
            .forward_message(&auth.organization_id, parse_uuid(&id)?, request)
            .await?
    )))
}

/// `POST /v1/messages/:id/snooze` with `{until}`.
#[derive(Deserialize)]
pub(in crate::api) struct Snooze {
    until: String,
}

pub(in crate::api) async fn snooze_message(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
    Json(request): Json<Snooze>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(state.snooze_message(
        &auth.organization_id,
        parse_uuid(&id)?,
        &request.until
    )?)))
}

/// `DELETE /v1/messages/:id/snooze`: show the message in listings again now.
pub(in crate::api) async fn wake_message(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(
        state.wake_message(&auth.organization_id, parse_uuid(&id)?)?
    )))
}

/// `POST /v1/messages/:id/mute`: leave the message's conversation, and later
/// mail of it, out of listings.
pub(in crate::api) async fn mute_conversation(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(
        state.mute_conversation(&auth.organization_id, parse_uuid(&id)?)?
    )))
}

/// `DELETE /v1/messages/:id/mute`: show the conversation in listings again.
pub(in crate::api) async fn unmute_conversation(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(
        state.unmute_conversation(&auth.organization_id, parse_uuid(&id)?)?
    )))
}

/// `POST /v1/views/:name/snooze` with `{until}`: snooze every message the
/// saved view lists; answers `{view, until, messages}`.
pub(in crate::api) async fn snooze_view(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(name): Path<String>,
    Json(request): Json<Snooze>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(state.snooze_view(
        &auth.organization_id,
        &name,
        &request.until
    )?)))
}

/// `DELETE /v1/views/:name/snooze`: show every snoozed message the saved view
/// keeps again now; answers `{view, messages}`.
pub(in crate::api) async fn wake_view(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(name): Path<String>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(state.wake_view(&auth.organization_id, &name)?)))
}

/// `POST /v1/views/:name/mute` mutes the conversation of every message the
/// saved view keeps and `DELETE` unmutes them; both answer `{view,
/// conversations, refused}`.
pub(in crate::api) async fn mute_view(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(name): Path<String>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(state.mute_view(&auth.organization_id, &name, true)?)))
}

pub(in crate::api) async fn unmute_view(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(name): Path<String>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(state.mute_view(&auth.organization_id, &name, false)?)))
}

/// `POST /v1/messages/:id/thread/snooze` with `{until}`: snooze every
/// message of the conversation; `DELETE` wakes its snoozed ones. Both answer
/// `{thread, until, messages}`.
pub(in crate::api) async fn snooze_thread(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
    Json(request): Json<Snooze>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(state.snooze_thread(&auth.organization_id, parse_uuid(&id)?, &request.until)?)))
}

pub(in crate::api) async fn wake_thread(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(state.wake_thread(&auth.organization_id, parse_uuid(&id)?)?)))
}

/// `PUT /v1/messages/:id/note` with `{note}`; `null` removes it.
#[derive(Deserialize)]
pub(in crate::api) struct Note {
    note: Option<String>,
    /// Add `note` to the message's note on a line of its own instead of replacing it.
    #[serde(default)]
    append: bool,
}

pub(in crate::api) async fn set_message_note(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
    Json(request): Json<Note>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    let id = parse_uuid(&id)?;
    Ok(Json(match (request.append, request.note.as_deref()) {
        (true, Some(text)) => json!(state.append_message_note(&auth.organization_id, id, text)?),
        (_, note) => json!(state.set_message_note(&auth.organization_id, id, note)?),
    }))
}
