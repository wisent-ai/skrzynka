//! Tag routes: the operator's own labels on stored messages.

use super::parse_uuid;
use crate::{
    auth::{AuthContext, OrganizationRole},
    error::AppError,
    service::{AppState, RetagRequest, TagFilter},
};
use axum::{
    extract::{Extension, Path, State},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

/// `POST /v1/messages/:id/tags` with `{add, remove}`: change the message's
/// tags and answer the message.
pub(in crate::api) async fn retag_message(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
    Json(request): Json<RetagRequest>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(state.retag_message(
        &auth.organization_id,
        parse_uuid(&id)?,
        request
    )?)))
}

/// `POST /v1/views/:name/tags` with `{add, remove}`: change the tags of
/// every message the saved view lists, and answer `{view, messages}`.
pub(in crate::api) async fn retag_view(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(name): Path<String>,
    Json(request): Json<RetagRequest>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(state.retag_view(
        &auth.organization_id,
        &name,
        request
    )?)))
}

/// `POST /v1/messages/:id/thread/tags` with `{add, remove}`: change the tags
/// of every message of the conversation, and answer `{thread, messages}`.
pub(in crate::api) async fn retag_thread(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
    Json(request): Json<RetagRequest>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(state.retag_thread(
        &auth.organization_id,
        parse_uuid(&id)?,
        request
    )?)))
}

/// `GET /v1/tags?mailbox_id=&contains=&since=&until=`: what `message tags
/// --mailbox --contains --since --until` lists.
#[derive(Deserialize)]
pub(in crate::api) struct TagsQuery {
    mailbox_id: Option<String>,
    contains: Option<String>,
    since: Option<String>,
    until: Option<String>,
}

/// `GET /v1/tags`: every tag the organization's messages carry, or one
/// mailbox's, or those holding a text, counted on the mail of a window.
pub(in crate::api) async fn list_tags(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    axum::extract::Query(query): axum::extract::Query<TagsQuery>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(
        state.list_tags(&auth.organization_id, &query.filter()?)?
    )))
}

/// `GET /v1/tag-counts/csv` with the parameters of `GET /v1/tags`: the tags
/// listed as the CSV table `message tags --csv` writes, offered as
/// `tags.csv`. It is not under `/v1/tags/`, where a path names one tag.
pub(in crate::api) async fn tags_csv(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    axum::extract::Query(query): axum::extract::Query<TagsQuery>,
) -> Result<axum::response::Response, AppError> {
    let tags = state.list_tags(&auth.organization_id, &query.filter()?)?;
    Ok(super::listing::csv_file(
        "tags.csv",
        crate::service::tag_table(&tags),
    ))
}

impl TagsQuery {
    /// The tags these parameters ask for, as `message tags` takes them.
    fn filter(self) -> Result<TagFilter, AppError> {
        let mailbox_id = self.mailbox_id.as_deref().map(parse_uuid).transpose()?;
        Ok(TagFilter {
            mailbox_id,
            contains: self.contains,
            since: self.since,
            until: self.until,
        })
    }
}

/// `POST /v1/tags/:tag/rename` with `{to}`: rename the tag on every message,
/// tag rule and saved view; answers `{tag, to, messages, rules, views}`.
#[derive(Deserialize)]
pub(in crate::api) struct RenameTag {
    to: String,
}

pub(in crate::api) async fn rename_tag(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(tag): Path<String>,
    Json(request): Json<RenameTag>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(state.rename_tag(
        &auth.organization_id,
        &tag,
        &request.to
    )?)))
}

/// `DELETE /v1/tags/:tag`: take the tag off every message; answers
/// `{tag, messages}` with rules and views left as they are.
pub(in crate::api) async fn remove_tag(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(tag): Path<String>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(state.remove_tag(&auth.organization_id, &tag)?)))
}
