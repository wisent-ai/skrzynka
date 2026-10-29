//! `/v1/mailboxes/:id/auto-reply`: a mailbox's automatic reply, and the
//! responder's decisions on received mail (docs/replies/auto-reply.md).

use super::parse_uuid;
use crate::{
    auth::{AuthContext, OrganizationRole},
    error::AppError,
    service::{AppState, AutoReplyRequest},
};
use axum::{
    extract::{Extension, Path, State},
    Json,
};
use serde_json::{json, Value};

pub(super) async fn mailbox_auto_reply(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(state.mailbox_auto_reply(
        &auth.organization_id,
        parse_uuid(&id)?
    )?)))
}

/// `PUT`: `{body, every, from?, until?}` sets the reply; `null` removes it.
pub(super) async fn set_mailbox_auto_reply(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
    Json(request): Json<Option<AutoReplyRequest>>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Admin)?;
    Ok(Json(json!(state.set_mailbox_auto_reply(
        &auth.organization_id,
        parse_uuid(&id)?,
        request
    )?)))
}

/// `GET /v1/mailboxes/:id/auto-reply/decisions`: every decision, newest first.
pub(super) async fn auto_reply_decisions(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(state.auto_reply_decisions(
        &auth.organization_id,
        parse_uuid(&id)?
    )?)))
}

/// `POST /v1/auto-replies/answer`: answer every message of the
/// organization's mailboxes the responder has not decided on, now, as a
/// running service does on each import.
pub(super) async fn answer_automatically(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(
        state
            .answer_automatically(Some(&auth.organization_id))
            .await?
    )))
}
