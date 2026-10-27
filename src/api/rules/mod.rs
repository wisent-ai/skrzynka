//! Tag rule routes: rules that tag mail as every import writes it.

use super::parse_uuid;
use crate::{
    auth::{AuthContext, OrganizationRole},
    error::AppError,
    service::{AppState, NewTagRule},
};
use axum::{
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

/// `GET /v1/tag-rules?contains=TEXT&paused=true|false&mailbox_id=UUID`: every
/// rule, or those holding the text, only the paused or the tagging ones, or
/// those tagging one mailbox's mail.
#[derive(Deserialize)]
pub(in crate::api) struct RulesQuery {
    contains: Option<String>,
    paused: Option<bool>,
    mailbox_id: Option<String>,
}

pub(in crate::api) async fn list_rules(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Query(query): Query<RulesQuery>,
) -> Result<Json<Value>, AppError> {
    let mailbox_id = query.mailbox_id.as_deref().map(parse_uuid).transpose()?;
    Ok(Json(json!(state.tag_rules(&auth.organization_id, query.contains.as_deref(), query.paused, mailbox_id)?)))
}

/// `GET /v1/tag-rules/export` with the listing's parameters: the rules it
/// keeps as the JSON document `message rule export` writes, offered as
/// `tag-rules.json`.
pub(in crate::api) async fn export_rules(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Query(query): Query<RulesQuery>,
) -> Result<axum::response::Response, AppError> {
    use axum::{
        http::header::{CONTENT_DISPOSITION, CONTENT_TYPE},
        response::IntoResponse,
    };
    let mailbox_id = query.mailbox_id.as_deref().map(parse_uuid).transpose()?;
    let document = state.tag_rule_document(&auth.organization_id, query.contains.as_deref(), query.paused, mailbox_id)?;
    Ok((
        [
            (CONTENT_TYPE, "application/json; charset=utf-8"),
            (CONTENT_DISPOSITION, "attachment; filename=\"tag-rules.json\""),
        ],
        document,
    )
        .into_response())
}

/// `POST /v1/tag-rules/import` with `{text}`: add every rule of a document
/// an export wrote, as `message rule import` does.
#[derive(Deserialize)]
pub(in crate::api) struct ImportRules {
    text: String,
}

pub(in crate::api) async fn import_rules(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Json(request): Json<ImportRules>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(state.import_tag_rules(&auth.organization_id, &request.text)?)))
}

/// `POST /v1/tag-rules` with `{tag, sender, list, contains, recipient, attachment, mailbox}`.
pub(in crate::api) async fn add_rule(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Json(rule): Json<NewTagRule>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(state.add_tag_rule(&auth.organization_id, rule)?)))
}

/// `PUT /v1/tag-rules/:id` with the same body: the rule's tag, conditions
/// and mailbox replaced, its ID kept.
pub(in crate::api) async fn replace_rule(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
    Json(rule): Json<NewTagRule>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    let id = parse_uuid(&id)?;
    Ok(Json(json!(state.replace_tag_rule(&auth.organization_id, id, rule)?)))
}

/// `GET /v1/tag-rules/:id`: one rule, as `message rule show` prints it.
pub(in crate::api) async fn show_rule(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(state.tag_rule(&auth.organization_id, parse_uuid(&id)?)?)))
}

/// `GET /v1/tag-rules/:id/matches`: the stored messages the rule matches now,
/// newest first, as `message rule matches` prints them; nothing is tagged.
pub(in crate::api) async fn rule_matches(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(state.tag_rule_matches(&auth.organization_id, parse_uuid(&id)?)?)))
}

/// `POST /v1/tag-rules/:id/copy` with `{tag, mailbox}` (`mailbox` optional).
#[derive(Deserialize)]
pub(in crate::api) struct CopyRule {
    tag: String,
    #[serde(default)]
    mailbox: Option<String>,
}

/// A new rule with the rule's conditions and `tag`, as `message rule copy` makes it.
pub(in crate::api) async fn copy_rule(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
    Json(request): Json<CopyRule>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    let id = parse_uuid(&id)?;
    Ok(Json(json!(state.copy_tag_rule(&auth.organization_id, id, &request.tag, request.mailbox.as_deref())?)))
}

pub(in crate::api) async fn remove_rule(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    state.remove_tag_rule(&auth.organization_id, parse_uuid(&id)?)?;
    Ok(StatusCode::NO_CONTENT)
}

/// `POST /v1/tag-rules/apply`: tag the mail already stored.
pub(in crate::api) async fn apply_rules(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(state.apply_tag_rules(&auth.organization_id, None)?)))
}

/// `POST /v1/tag-rules/:id/apply`: tag the mail already stored by one rule.
pub(in crate::api) async fn apply_rule(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    let id = parse_uuid(&id)?;
    Ok(Json(json!(state.apply_tag_rules(&auth.organization_id, Some(id))?)))
}

/// `POST /v1/tag-rules/:id/pause` pauses the rule and `DELETE` resumes it;
/// both answer the rule with `paused_at`.
pub(in crate::api) async fn pause_rule(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(state.pause_tag_rule(&auth.organization_id, parse_uuid(&id)?, true)?)))
}

pub(in crate::api) async fn resume_rule(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(state.pause_tag_rule(&auth.organization_id, parse_uuid(&id)?, false)?)))
}
