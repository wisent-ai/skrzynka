//! Mailing-list routes: the lists the stored mail came from, and a request to
//! leave the one a message came from.

use super::parse_uuid;
use crate::{
    auth::{AuthContext, OrganizationRole},
    error::AppError,
    mail::UnsubscribeRequest,
    service::{AppState, ContactFilter, SubscriptionFilter},
};
use axum::{
    extract::{Extension, Path, Query, State},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

/// `GET /v1/subscriptions?mailbox_id=&subscribed=true&contains=&since=&until=&leavable=true`.
#[derive(Deserialize)]
pub(in crate::api) struct SubscriptionQuery {
    mailbox_id: Option<String>,
    #[serde(default)]
    subscribed: bool,
    contains: Option<String>,
    since: Option<String>,
    until: Option<String>,
    #[serde(default)]
    leavable: bool,
}

pub(in crate::api) async fn list_subscriptions(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Query(query): Query<SubscriptionQuery>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(state.list_subscriptions(&auth.organization_id, &query.filter()?)?)))
}

/// `GET /v1/subscriptions/csv` with the same parameters: the lists kept as
/// the CSV table `message subscriptions --csv` writes, offered as
/// `subscriptions.csv`.
pub(in crate::api) async fn subscriptions_csv(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Query(query): Query<SubscriptionQuery>,
) -> Result<axum::response::Response, AppError> {
    let lists = state.list_subscriptions(&auth.organization_id, &query.filter()?)?.lists;
    Ok(super::listing::csv_file("subscriptions.csv", crate::service::subscription_table(&lists)))
}

impl SubscriptionQuery {
    /// The lists these parameters ask for, as `message subscriptions` takes them.
    fn filter(self) -> Result<SubscriptionFilter, AppError> {
        Ok(SubscriptionFilter {
            mailbox_id: self.mailbox_id.as_deref().map(parse_uuid).transpose()?,
            subscribed: self.subscribed,
            contains: self.contains,
            since: self.since,
            until: self.until,
            leavable: self.leavable,
        })
    }
}

/// What a contacts read narrows to: one mailbox, a text the address or a
/// name must contain, the addresses that bounced, those the suppression list
/// covers, those a blocked entry covers, those whose latest exchange falls at or after `since` and before
/// `until`, and those written to one way only; `recent` orders the latest
/// exchanged with first.
#[derive(Deserialize)]
pub(in crate::api) struct ContactsQuery {
    mailbox_id: Option<String>,
    contains: Option<String>,
    #[serde(default)]
    recent: bool,
    #[serde(default)]
    bounced: bool,
    #[serde(default)]
    suppressed: bool,
    #[serde(default)]
    blocked: bool,
    since: Option<String>,
    until: Option<String>,
    #[serde(default)]
    never_written: bool,
    #[serde(default)]
    never_received: bool,
}

impl ContactsQuery {
    fn filter(self) -> Result<ContactFilter, AppError> {
        Ok(ContactFilter {
            mailbox_id: self.mailbox_id.as_deref().map(parse_uuid).transpose()?,
            contains: self.contains,
            recent: self.recent,
            bounced: self.bounced,
            suppressed: self.suppressed,
            blocked: self.blocked,
            since: self.since,
            until: self.until,
            never_written: self.never_written,
            never_received: self.never_received,
        })
    }
}

/// `GET /v1/contacts[?mailbox_id=&contains=&recent=true&bounced=true&suppressed=true&blocked=true&since=&until=&never_written=true&never_received=true]`: every
/// correspondent of the stored mail and originated mail, most exchanged first.
pub(in crate::api) async fn list_contacts(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Query(query): Query<ContactsQuery>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(state.list_contacts(&auth.organization_id, &query.filter()?)?)))
}

/// `GET /v1/contacts/vcard?mailbox_id=&contains=&recent=&bounced=`: the same
/// correspondents as vCards (RFC 6350), typed `text/vcard` and offered as `contacts.vcf`.
pub(in crate::api) async fn contacts_vcard(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Query(query): Query<ContactsQuery>,
) -> Result<axum::response::Response, AppError> {
    use axum::{
        http::header::{CONTENT_DISPOSITION, CONTENT_TYPE},
        response::IntoResponse,
    };
    let cards = state.contacts_vcard(&auth.organization_id, &query.filter()?)?;
    Ok((
        [
            (CONTENT_TYPE, "text/vcard; charset=utf-8"),
            (CONTENT_DISPOSITION, "attachment; filename=\"contacts.vcf\""),
        ],
        cards,
    )
        .into_response())
}

/// `GET /v1/contacts/csv` with the same parameters: the same correspondents
/// as a CSV table (RFC 4180), offered as `contacts.csv`.
pub(in crate::api) async fn contacts_csv(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Query(query): Query<ContactsQuery>,
) -> Result<axum::response::Response, AppError> {
    let table = state.contacts_csv(&auth.organization_id, &query.filter()?)?;
    Ok(super::listing::csv_file("contacts.csv", table))
}

/// `POST /v1/contacts/suppress` with the listing's parameters and
/// `{reason}` (optional): every correspondent the listing keeps put on the
/// suppression list, as `message contacts --suppress-listed` does; role
/// `member`.
#[derive(Deserialize)]
pub(in crate::api) struct SuppressContacts {
    reason: Option<String>,
}

pub(in crate::api) async fn suppress_contacts(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Query(query): Query<ContactsQuery>,
    Json(request): Json<SuppressContacts>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    let done = state.suppress_contacts(&auth.organization_id, &query.filter()?, request.reason.as_deref())?;
    Ok(Json(json!(done)))
}

/// `POST /v1/contacts/block` with the listing's parameters: every
/// correspondent the listing keeps blocked, as `message contacts
/// --block-listed` does; role `member`.
pub(in crate::api) async fn block_contacts(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Query(query): Query<ContactsQuery>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(state.block_contacts(&auth.organization_id, &query.filter()?)?)))
}

pub(in crate::api) async fn unsubscribe(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Path(id): Path<String>,
    Json(request): Json<UnsubscribeRequest>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    Ok(Json(json!(
        state
            .unsubscribe(&auth.organization_id, parse_uuid(&id)?, request)
            .await?
    )))
}

/// `POST /v1/subscriptions/unsubscribe` with `{list_id, mailbox,
/// idempotency_key}` (`mailbox` optional, an ID or address): leave the list by
/// its List-Id, as `message unsubscribe --list --mailbox` does.
#[derive(Deserialize)]
pub(in crate::api) struct LeaveList {
    list_id: String,
    #[serde(default)]
    mailbox: Option<String>,
    idempotency_key: String,
}

pub(in crate::api) async fn unsubscribe_list(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Json(request): Json<LeaveList>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    let mailbox_id = match request.mailbox.as_deref() {
        Some(selector) => Some(state.resolve_mailbox(&auth.organization_id, selector)?.id),
        None => None,
    };
    let key = UnsubscribeRequest { idempotency_key: request.idempotency_key };
    Ok(Json(json!(
        state
            .unsubscribe_list(&auth.organization_id, &request.list_id, mailbox_id, key)
            .await?
    )))
}

/// `POST /v1/subscriptions/leave` with the listing's parameters and
/// `{idempotency_key}`: ask every list the listing keeps to stop sending, as
/// `message subscriptions --leave` does; role `member`.
pub(in crate::api) async fn leave_listed(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
    Query(query): Query<SubscriptionQuery>,
    Json(request): Json<UnsubscribeRequest>,
) -> Result<Json<Value>, AppError> {
    auth.require_role(OrganizationRole::Member)?;
    let filter = query.filter()?;
    Ok(Json(json!(state.unsubscribe_listed(&auth.organization_id, &filter, &request.idempotency_key).await?)))
}
