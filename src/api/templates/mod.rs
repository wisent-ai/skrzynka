//! Template routes: texts kept to reuse in replies and new messages.

use crate::{
    auth::{AuthContext, OrganizationRole},
    error::AppError,
    service::{AppState, TemplateFilter},
};
use axum::{
    extract::{Extension, Path, Query, State},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

/// `PUT /v1/templates/:name` with `{body, subject}`; `subject` optional.
#[derive(Deserialize)]
pub(in crate::api) struct SaveTemplate {
    body: String,
    #[serde(default)]
    subject: Option<String>,
}

/// `GET /v1/templates?contains=&placeholder=`, as `message template list
/// --contains --placeholder` takes them.
pub(in crate::api) async fn list_templates(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Query(filter): Query<TemplateFilter>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(state.templates(&auth.organization_id, &filter)?)))
}

/// `GET /v1/template-document?contains=&placeholder=`: the templates the
/// listing keeps for the same parameters, as the JSON document `message
/// template export` writes, offered as `templates.json`.
pub(in crate::api) async fn export_templates(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Query(filter): Query<TemplateFilter>,
) -> Result<axum::response::Response, AppError> {
    use axum::{
        http::header::{CONTENT_DISPOSITION, CONTENT_TYPE},
        response::IntoResponse,
    };
    let document = state.template_document(&auth.organization_id, &filter)?;
    Ok((
        [
            (CONTENT_TYPE, "application/json; charset=utf-8"),
            (CONTENT_DISPOSITION, "attachment; filename=\"templates.json\""),
        ],
        document,
    )
        .into_response())
}

/// `POST /v1/template-document` with `{text}`: keep every template of a
/// document an export wrote, as `message template import` does.
#[derive(Deserialize)]
pub(in crate::api) struct ImportTemplates {
    text: String,
}

pub(in crate::api) async fn import_templates(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Json(request): Json<ImportTemplates>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(state.import_templates(&auth.organization_id, &request.text)?)))
}

/// `GET /v1/templates/:name`: the name URL-encoded.
pub(in crate::api) async fn get_template(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(name): Path<String>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(state.template(&auth.organization_id, &name)?)))
}

pub(in crate::api) async fn save_template(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(name): Path<String>,
    Json(request): Json<SaveTemplate>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(
        state.save_template(&auth.organization_id, &name, request.subject.as_deref(), &request.body)?
    )))
}

pub(in crate::api) async fn delete_template(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(name): Path<String>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(
        state.delete_template(&auth.organization_id, &name)?
    )))
}

/// `POST /v1/templates/:name/rename` and `/copy` with `{"to": "<NAME>"}`.
#[derive(Deserialize)]
pub(in crate::api) struct RenameTemplate {
    to: String,
}

pub(in crate::api) async fn rename_template(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(name): Path<String>,
    Json(request): Json<RenameTemplate>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(
        state.rename_template(&auth.organization_id, &name, &request.to)?
    )))
}

pub(in crate::api) async fn copy_template(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(name): Path<String>,
    Json(request): Json<RenameTemplate>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(
        state.copy_template(&auth.organization_id, &name, &request.to)?
    )))
}

/// `POST /v1/templates/:name/render` with `{"values": [{"key", "value"}],
/// "message_id": UUID}`: the template with its `{{key}}` placeholders
/// filled, `sender`, `sender_name` and `subject` from the stored message
/// when it is named and no value gives them. Keys travel as values, never
/// as JSON member names, so no client renames them.
#[derive(Deserialize)]
pub(in crate::api) struct RenderTemplate {
    #[serde(default)]
    values: Vec<TemplateValue>,
    #[serde(default)]
    message_id: Option<String>,
}

#[derive(Deserialize)]
pub(in crate::api) struct TemplateValue {
    key: String,
    value: String,
}

pub(in crate::api) async fn render_template(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(name): Path<String>,
    Json(request): Json<RenderTemplate>,
) -> Result<Json<Value>, AppError> {
    let mut values = std::collections::BTreeMap::new();
    for TemplateValue { key, value } in request.values {
        if let Some(given) = values.insert(key.clone(), value) {
            return Err(AppError::invalid(
                "TEMPLATE_VALUE_INVALID",
                format!("values give {key:?} twice (one of them {given:?}); give each placeholder one value"),
            ));
        }
    }
    let message = request.message_id.as_deref().map(super::parse_uuid).transpose()?;
    Ok(Json(json!(
        state.render_template(&auth.organization_id, &name, &values, message)?
    )))
}
