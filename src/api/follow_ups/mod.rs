//! Follow-up routes: sent messages the operator expects an answer to.

use super::parse_uuid;
use crate::{
    auth::{AuthContext, OrganizationRole},
    error::AppError,
    service::{AppState, FollowUpFilter, FollowUpState},
};
use axum::{
    extract::{Extension, Path, Query, State},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

/// `GET /v1/follow-ups?state=waiting|due|answered&mailbox_id=UUID&to=TEXT&contains=TEXT&due_by=WHEN`.
#[derive(Deserialize)]
pub(in crate::api) struct FollowUpQuery {
    state: Option<FollowUpState>,
    mailbox_id: Option<String>,
    to: Option<String>,
    contains: Option<String>,
    due_by: Option<String>,
}

/// `PUT /v1/outbound/:id/follow-up` with `{by}`.
#[derive(Deserialize)]
pub(in crate::api) struct SetFollowUp {
    by: String,
}

pub(in crate::api) async fn list_follow_ups(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Query(query): Query<FollowUpQuery>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(state.follow_ups(&auth.organization_id, &query.filter()?)?)))
}

/// `GET /v1/follow-ups/csv` with the listing's parameters: the follow-ups it
/// keeps as the CSV table `message followup list --csv` writes, offered as
/// `follow-ups.csv`.
pub(in crate::api) async fn follow_ups_csv(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Query(query): Query<FollowUpQuery>,
) -> Result<axum::response::Response, AppError> {
    let follow_ups = state.follow_ups(&auth.organization_id, &query.filter()?)?;
    Ok(super::listing::csv_file("follow-ups.csv", crate::service::follow_up_table(&follow_ups)))
}

impl FollowUpQuery {
    /// The listing these parameters ask for, as `message followup list` takes it.
    fn filter(self) -> Result<FollowUpFilter, AppError> {
        let mailbox_id = self.mailbox_id.as_deref().map(super::parse_uuid).transpose()?;
        Ok(FollowUpFilter { state: self.state, mailbox_id, to: self.to, contains: self.contains, due_by: self.due_by })
    }
}

pub(in crate::api) async fn set_follow_up(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
    Json(request): Json<SetFollowUp>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(state.set_follow_up(
        &auth.organization_id,
        parse_uuid(&id)?,
        &request.by
    )?)))
}

/// `GET /v1/outbound/:id/follow-up`: where that message's follow-up stands,
/// as `message followup show` prints it.
pub(in crate::api) async fn show_follow_up(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(state.get_follow_up(&auth.organization_id, parse_uuid(&id)?)?)))
}

pub(in crate::api) async fn clear_follow_up(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(
        state.clear_follow_up(&auth.organization_id, parse_uuid(&id)?)?
    )))
}

/// `DELETE /v1/follow-ups?state=&mailbox_id=&to=&contains=`: stop expecting
/// an answer to every follow-up the listing keeps, as `message followup
/// clear --state` does; `state` is required, so nothing clears them all.
pub(in crate::api) async fn clear_follow_ups(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Query(query): Query<FollowUpQuery>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(state.clear_follow_ups(&auth.organization_id, &query.filter()?)?)))
}

/// `POST /v1/follow-ups/nudge` with the listing's parameters and
/// `{template}`: one draft per follow-up kept still owed an answer, as
/// `message followup nudge` drafts them; role `member`.
#[derive(Deserialize)]
pub(in crate::api) struct NudgeFollowUps {
    template: String,
}

pub(in crate::api) async fn nudge_follow_ups(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Query(query): Query<FollowUpQuery>,
    Json(request): Json<NudgeFollowUps>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    let drafts = state.nudge_follow_ups(&auth.organization_id, &query.filter()?, &request.template)?;
    Ok(Json(json!(drafts)))
}
