//! Saved view routes: listing filters kept under a name.
//! `GET /v1/messages?view=NAME` lists what one keeps.

use crate::{
    auth::{AuthContext, OrganizationRole},
    error::AppError,
    service::{AppState, SavedFilters},
};
use axum::{
    extract::{Extension, Path, Query, State},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

/// `GET /v1/views?count=true&contains=`: each view, or those whose name
/// holds the text, with how many messages it lists now when `count` is asked.
#[derive(Deserialize)]
pub(in crate::api) struct ViewsQuery {
    #[serde(default)]
    count: bool,
    contains: Option<String>,
}

pub(in crate::api) async fn list_views(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Query(query): Query<ViewsQuery>,
) -> Result<Json<Value>, AppError> {
    let contains = query.contains.as_deref();
    Ok(Json(match query.count {
        true => json!(state.counted_views(&auth.organization_id, contains)?),
        false => json!(state.views(&auth.organization_id, contains)?),
    }))
}

/// `GET /v1/view-document?contains=`: every view, or those whose name holds
/// the text, as the JSON document `message view export` writes, offered as
/// `views.json`.
pub(in crate::api) async fn export_views(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Query(query): Query<ViewsQuery>,
) -> Result<axum::response::Response, AppError> {
    use axum::{
        http::header::{CONTENT_DISPOSITION, CONTENT_TYPE},
        response::IntoResponse,
    };
    let document = state.view_document(&auth.organization_id, query.contains.as_deref())?;
    Ok((
        [
            (CONTENT_TYPE, "application/json; charset=utf-8"),
            (CONTENT_DISPOSITION, "attachment; filename=\"views.json\""),
        ],
        document,
    )
        .into_response())
}

/// `POST /v1/view-document` with `{text}`: keep every view of a document an
/// export wrote, as `message view import` does.
#[derive(Deserialize)]
pub(in crate::api) struct ImportViews {
    text: String,
}

pub(in crate::api) async fn import_views(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Json(request): Json<ImportViews>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(state.import_views(&auth.organization_id, &request.text)?)))
}

/// `GET /v1/views/:name`: the name URL-encoded.
pub(in crate::api) async fn get_view(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(name): Path<String>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(state.view(&auth.organization_id, &name)?)))
}

/// `PUT /v1/views/:name` with the filters as `GET /v1/messages` names them
/// (`mailbox_id`, `recipient`, `sender`, `text`, `since`, `until`, `tag`,
/// `list`, `with_attachments`, `bounces`, `include_blocked`, `include_snoozed`).
pub(in crate::api) async fn save_view(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(name): Path<String>,
    Json(filters): Json<SavedFilters>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(
        state.save_view(&auth.organization_id, &name, &filters)?
    )))
}

pub(in crate::api) async fn delete_view(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(name): Path<String>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(state.delete_view(&auth.organization_id, &name)?)))
}

/// `POST /v1/views/:name/rename` with `{"to": "<NAME>"}`.
#[derive(Deserialize)]
pub(in crate::api) struct RenameView {
    to: String,
}

pub(in crate::api) async fn rename_view(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(name): Path<String>,
    Json(request): Json<RenameView>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(
        state.rename_view(&auth.organization_id, &name, &request.to)?
    )))
}

/// `POST /v1/views/:name/copy` with `{"to": "<NAME>"}`: the copy kept under
/// `to`, the view itself unchanged.
pub(in crate::api) async fn copy_view(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(name): Path<String>,
    Json(request): Json<RenameView>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(state.copy_view(&auth.organization_id, &name, &request.to)?)))
}

/// `POST /v1/views/:name/forward` with `{to, cc, bcc, body | template,
/// values, attachments, allow_bounced, follow_up, idempotency_key}`: every
/// message the view lists forwarded, each claimed by `<key>-<message id>`,
/// as `message forward --view` does; role `member`.
pub(in crate::api) async fn forward_view(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(name): Path<String>,
    Json(request): Json<crate::service::ForwardViewRequest>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(state.forward_view(&auth.organization_id, &name, request).await?)))
}

/// `POST /v1/views/:name/replies` with `{body | template, values,
/// attachments, reply_all, quote, cc, bcc, idempotency_key}`: every message
/// the view lists answered, each claimed by `<key>-<message id>`, as
/// `message reply --view` does; role `member`.
pub(in crate::api) async fn reply_view(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(name): Path<String>,
    Json(request): Json<crate::service::ReplyViewRequest>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(state.reply_view(&auth.organization_id, &name, request).await?)))
}
