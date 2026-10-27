//! `GET /v1/messages`: the stored messages a listing keeps, filtered as
//! `message list` filters them; `GET /v1/replies`: every reply sent.

use super::parse_uuid;
use crate::{
    auth::{AuthContext, OrganizationRole},
    error::AppError,
    service::{AppState, ReplyFilter, SavedFilters},
};
use axum::{
    extract::{Extension, Path, Query, State},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

mod files;
pub(in crate::api) use files::{files_csv, find_files};

/// The filters of `GET /v1/messages`, named and read as `message list` takes
/// them (`text` is its `--contains`). Every parameter is optional: an absent
/// text is no filter, an absent flag `false`; `view` lists what a saved view
/// keeps, and no other filter may be given beside it.
#[derive(Deserialize, Default)]
#[serde(default)]
pub(in crate::api) struct MessageQuery {
    mailbox_id: Option<String>,
    recipient: Option<String>,
    cc: Option<String>,
    sender: Option<String>,
    text: Option<String>,
    since: Option<String>,
    until: Option<String>,
    tag: Option<String>,
    list: Option<String>,
    attachment: Option<String>,
    with_attachments: bool,
    bounces: bool,
    include_blocked: bool,
    include_snoozed: bool,
    include_muted: bool,
    unanswered: bool,
    failed_authentication: bool,
    oldest_first: bool,
    snoozed: bool,
    noted: bool,
    muted: bool,
    untagged: bool,
    answered: bool,
    files_over: Option<u64>,
    view: Option<String>,
    limit: Option<u32>,
    offset: Option<u32>,
}

pub(in crate::api) async fn list_messages(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Query(query): Query<MessageQuery>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(listed(&state, &auth, query)?)))
}

/// `GET /v1/messages/csv` with the listing's parameters: the messages it
/// keeps as the CSV table `message list --csv` writes, typed `text/csv` and
/// offered as `messages.csv`.
pub(in crate::api) async fn messages_csv(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Query(query): Query<MessageQuery>,
) -> Result<axum::response::Response, AppError> {
    Ok(csv_file("messages.csv", crate::service::message_table(&listed(&state, &auth, query)?)))
}

/// A CSV table answered as a download: typed `text/csv` in UTF-8 and
/// offered under `name`. Every `…/csv` route answers through it.
pub(in crate::api) fn csv_file(name: &str, table: String) -> axum::response::Response {
    use axum::{
        http::header::{CONTENT_DISPOSITION, CONTENT_TYPE},
        response::IntoResponse,
    };
    let headers = [
        (CONTENT_TYPE, "text/csv; charset=utf-8".to_string()),
        (CONTENT_DISPOSITION, format!("attachment; filename=\"{name}\"")),
    ];
    (headers, table).into_response()
}

/// The stored messages `query` keeps, as `message list` reads them.
fn listed(
    state: &AppState,
    auth: &AuthContext,
    query: MessageQuery,
) -> Result<Vec<crate::models::Message>, AppError> {
    let filters = SavedFilters {
        mailbox_id: query.mailbox_id.as_deref().map(parse_uuid).transpose()?,
        recipient: query.recipient,
        cc: query.cc,
        sender: query.sender,
        text: query.text,
        since: query.since,
        until: query.until,
        tag: query.tag,
        list: query.list,
        attachment: query.attachment,
        with_attachments: query.with_attachments,
        bounces: query.bounces,
        include_blocked: query.include_blocked,
        include_snoozed: query.include_snoozed,
        include_muted: query.include_muted,
        unanswered: query.unanswered,
        failed_authentication: query.failed_authentication,
        oldest_first: query.oldest_first,
        snoozed: query.snoozed,
        noted: query.noted,
        muted: query.muted,
        untagged: query.untagged,
        answered: query.answered,
        files_over: query.files_over,
    };
    state.list_listing(&auth.organization_id, &filters, query.view.as_deref(), None, query.limit, query.offset)
}

/// `GET /v1/replies?mailbox_id=&status=&since=&until=&contains=&attachment=&oldest_first=`,
/// as `message replies --mailbox --status --since --until --contains
/// --attachment --oldest-first` takes them.
#[derive(Deserialize)]
pub(in crate::api) struct RepliesQuery {
    mailbox_id: Option<String>,
    status: Option<String>,
    since: Option<String>,
    until: Option<String>,
    contains: Option<String>,
    attachment: Option<String>,
    #[serde(default)]
    oldest_first: bool,
}

/// Every reply attempt of the organization, or of one mailbox's messages,
/// newest first, in one delivery status or window when they are named.
pub(in crate::api) async fn list_all_replies(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Query(query): Query<RepliesQuery>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(state.all_replies(&auth.organization_id, &query.filter()?)?)))
}

/// `GET /v1/replies/csv` with the same parameters: the replies kept as the
/// CSV table `message replies --csv` writes, offered as `replies.csv`.
pub(in crate::api) async fn replies_csv(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Query(query): Query<RepliesQuery>,
) -> Result<axum::response::Response, AppError> {
    let replies = state.all_replies(&auth.organization_id, &query.filter()?)?;
    Ok(csv_file("replies.csv", crate::service::reply_table(&replies)))
}

/// `POST /v1/replies/resend` with the listing's parameters and
/// `{idempotency_key}`: every failed reply the listing keeps sent again, as
/// `message replies --resend-failed` does; role `member`.
#[derive(Deserialize)]
pub(in crate::api) struct ResendReplies {
    idempotency_key: String,
}

pub(in crate::api) async fn resend_failed_replies(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Query(query): Query<RepliesQuery>,
    Json(request): Json<ResendReplies>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    let filter = query.filter()?;
    Ok(Json(json!(state.resend_failed_replies(&auth.organization_id, &filter, &request.idempotency_key).await?)))
}

impl RepliesQuery {
    /// The replies these parameters ask for, as `message replies` takes them.
    fn filter(self) -> Result<ReplyFilter, AppError> {
        Ok(ReplyFilter {
            mailbox_id: self.mailbox_id.as_deref().map(parse_uuid).transpose()?,
            status: self.status,
            since: self.since,
            until: self.until,
            contains: self.contains,
            attachment: self.attachment,
            oldest_first: self.oldest_first,
        })
    }
}

/// `GET /v1/bounced?since=&until=&contains=`: the addresses `message bounced
/// [--since] [--until] [--contains]` lists.
#[derive(Deserialize)]
pub(in crate::api) struct BouncedQuery {
    since: Option<String>,
    until: Option<String>,
    contains: Option<String>,
}

/// `GET /v1/bounced`: every address a stored bounce report says delivery to
/// failed for good, or those whose newest report falls in a window or that
/// hold a text, as `message bounced` prints them.
pub(in crate::api) async fn list_bounced(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Query(query): Query<BouncedQuery>,
) -> Result<Json<Value>, AppError> {
    let (since, until, contains) = (query.since.as_deref(), query.until.as_deref(), query.contains.as_deref());
    Ok(Json(json!(state.find_bounced(&auth.organization_id, since, until, contains)?)))
}

/// `GET /v1/bounced/csv` with the listing's parameters: the addresses it
/// keeps as the CSV table `message bounced --csv` writes, offered as
/// `bounced.csv`.
pub(in crate::api) async fn bounced_csv(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Query(query): Query<BouncedQuery>,
) -> Result<axum::response::Response, AppError> {
    let (since, until, contains) = (query.since.as_deref(), query.until.as_deref(), query.contains.as_deref());
    let addresses = state.find_bounced(&auth.organization_id, since, until, contains)?;
    Ok(csv_file("bounced.csv", crate::service::bounced_table(&addresses)))
}

/// `DELETE /v1/bounced/:address`: the address works again, as `message
/// bounced --clear` says; reports received until now no longer refuse sends.
pub(in crate::api) async fn clear_bounced(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(address): Path<String>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(state.clear_bounced(&auth.organization_id, &address)?)))
}

/// `DELETE /v1/bounced?since=&until=&contains=` (at least one of them): every
/// address the listing keeps works again, as `message bounced --clear-listed`
/// says, each answered with when it was cleared.
pub(in crate::api) async fn clear_bounced_listed(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Query(query): Query<BouncedQuery>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    let (since, until, contains) = (query.since.as_deref(), query.until.as_deref(), query.contains.as_deref());
    Ok(Json(json!(state.clear_bounced_listed(&auth.organization_id, since, until, contains)?)))
}
