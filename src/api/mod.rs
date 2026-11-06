//! The HTTP API: the router and the handlers that need no organization context; the
//! Gmail, mailbox and message handlers live in their own modules.

use crate::{
    auth::{require_auth, AuthContext},
    error::AppError,
    models::HealthResponse,
    service::AppState,
};
use axum::{
    extract::{Extension, State},
    http::StatusCode,
    middleware,
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};
use uuid::Uuid;

mod actions;
mod auto_replies;
mod blocked;
mod conversations;
mod drafts;
mod follow_ups;
mod gmail;
mod inbound;
mod listing;
mod mailboxes;
mod messages;
mod rules;
mod tags;
mod templates;
mod unsubscribe;
mod views;
mod watch;

use actions::{
    delete_message, list_message_actions, mark_message, mark_thread, mark_view, move_message,
    move_thread, move_view,
};
use conversations::{
    forward_message, get_thread, mute_conversation, mute_view, set_message_note, snooze_message,
    snooze_thread, snooze_view, unmute_conversation, unmute_view, wake_message, wake_thread,
    wake_view,
};
use follow_ups::{clear_follow_up, list_follow_ups, set_follow_up, show_follow_up};
use gmail::{
    connect_gmail_app_password, connect_gmail_delegated, disconnect_gmail,
    gmail_connection_readiness_handler, gmail_oauth_callback, gmail_oauth_settled,
    gmail_oauth_status, start_gmail_oauth,
};
use inbound::{
    declare_inbound_domain, list_inbound_domains, read_inbound, remove_inbound_domain,
    sync_inbound_domain,
};
use listing::{list_all_replies, list_bounced, list_messages, messages_csv};
use mailboxes::{
    append_message, declare_mailbox, delete_mailbox, get_mailbox, list_mailboxes, mailbox_delivery,
    mailbox_folders, mailbox_send_limit, mailbox_signature, mailbox_summary,
    set_mailbox_send_limit, set_mailbox_signature, sync_all, sync_mailbox, undeclare_mailbox,
};
use messages::{
    create_outbound, create_reply, get_attachment, get_message, get_outbound, get_source,
    list_attachments, list_mailbox_outbound, list_outbound, list_replies, outbound_csv,
};
use tags::{list_tags, remove_tag, rename_tag, retag_message, retag_thread, retag_view};
use templates::{
    copy_template, delete_template, export_templates, get_template, import_templates,
    list_templates, rename_template, render_template, save_template,
};
use unsubscribe::{
    contacts_csv, contacts_vcard, list_contacts, list_subscriptions, unsubscribe, unsubscribe_list,
};

/// The loopback API: every route but the health answer and Google's callback
/// needs a bearer `auth_verifier` accepts. Only a process that serves the API
/// builds a verifier, so a command that only reads the fleet database never
/// needs the identity authority's address.
pub fn router(state: AppState, auth_verifier: crate::auth::AuthVerifier) -> Router {
    let protected = Router::new()
        .route("/v1/status", get(status))
        .route("/v1/skarbiec/items", get(list_skarbiec_items))
        .route("/v1/gmail/profiles", get(list_gmail_profiles))
        .route("/v1/gmail/oauth/start", post(start_gmail_oauth))
        .route("/v1/gmail/oauth/:flow_id", get(gmail_oauth_status))
        .route("/v1/gmail/oauth/:flow_id/settled", get(gmail_oauth_settled))
        .route(
            "/v1/gmail/connection",
            get(gmail_connection_readiness_handler),
        )
        .route("/v1/gmail/delegate", post(connect_gmail_delegated))
        .route("/v1/gmail/app-password", post(connect_gmail_app_password))
        .route("/v1/gmail/disconnect", post(disconnect_gmail))
        .route("/v1/mailboxes", get(list_mailboxes))
        .route("/v1/mailboxes/csv", get(mailboxes::mailboxes_csv))
        .route("/v1/mailboxes/declare", post(declare_mailbox))
        .route("/v1/mailboxes/:id", get(get_mailbox).delete(delete_mailbox))
        .route("/v1/mailboxes/:id/undeclare", post(undeclare_mailbox))
        .route("/v1/mailboxes/:id/sync", post(sync_mailbox))
        .route("/v1/mailboxes/:id/delivery", get(mailbox_delivery))
        .route(
            "/v1/mailboxes/:id/delivery/:table/csv",
            get(mailboxes::mailbox_delivery_csv),
        )
        .route("/v1/mailboxes/:id/summary", get(mailbox_summary))
        .route(
            "/v1/mailboxes/:id/summary/days/csv",
            get(mailboxes::mailbox_summary_csv),
        )
        .route("/v1/mailboxes/:id/folders", get(mailbox_folders))
        .route("/v1/mailboxes/:id/append", post(append_message))
        .route(
            "/v1/mailboxes/:id/limit",
            get(mailbox_send_limit).put(set_mailbox_send_limit),
        )
        .route(
            "/v1/mailboxes/:id/signature",
            get(mailbox_signature).put(set_mailbox_signature),
        )
        .route(
            "/v1/mailboxes/:id/auto-reply",
            get(auto_replies::mailbox_auto_reply).put(auto_replies::set_mailbox_auto_reply),
        )
        .route(
            "/v1/mailboxes/:id/auto-reply/decisions",
            get(auto_replies::auto_reply_decisions),
        )
        .route(
            "/v1/auto-replies/answer",
            post(auto_replies::answer_automatically),
        )
        .route("/v1/sync", post(sync_all))
        .route("/v1/messages", get(list_messages))
        .route("/v1/messages/csv", get(messages_csv))
        .route("/v1/messages/watch", get(watch::watch_messages))
        .route("/v1/messages/:id", get(get_message))
        .route("/v1/messages/:id/thread", get(get_thread))
        .route(
            "/v1/messages/:id/mute",
            post(mute_conversation).delete(unmute_conversation),
        )
        .route(
            "/v1/messages/:id/snooze",
            post(snooze_message).delete(wake_message),
        )
        .route(
            "/v1/messages/:id/thread/snooze",
            post(snooze_thread).delete(wake_thread),
        )
        .route(
            "/v1/messages/:id/note",
            axum::routing::put(set_message_note),
        )
        .route("/v1/messages/:id/source", get(get_source))
        .route("/v1/messages/:id/forward", post(forward_message))
        .route(
            "/v1/messages/:id/replies",
            get(list_replies).post(create_reply),
        )
        .route("/v1/attachments", get(listing::find_files))
        .route("/v1/attachments/csv", get(listing::files_csv))
        .route("/v1/messages/:id/attachments", get(list_attachments))
        .route("/v1/messages/:id/attachments/:index", get(get_attachment))
        .route("/v1/messages/:id/mark", post(mark_message))
        .route("/v1/messages/:id/move", post(move_message))
        .route("/v1/messages/:id/thread/mark", post(mark_thread))
        .route("/v1/messages/:id/thread/move", post(move_thread))
        .route("/v1/messages/:id/delete", post(delete_message))
        .route("/v1/messages/:id/actions", get(list_message_actions))
        .route(
            "/v1/mailboxes/:id/outbound",
            get(list_mailbox_outbound).post(create_outbound),
        )
        .route("/v1/outbound", get(list_outbound))
        .route("/v1/outbound/csv", get(outbound_csv))
        .route("/v1/outbound/follow-ups", post(messages::expect_answers))
        .route("/v1/outbound/resend", post(messages::resend_failed))
        .route("/v1/outbound/:id", get(get_outbound))
        .route(
            "/v1/outbound/:id/follow-up",
            get(show_follow_up)
                .put(set_follow_up)
                .delete(clear_follow_up),
        )
        .route(
            "/v1/follow-ups",
            get(list_follow_ups).delete(follow_ups::clear_follow_ups),
        )
        .route("/v1/follow-ups/csv", get(follow_ups::follow_ups_csv))
        .route("/v1/follow-ups/nudge", post(follow_ups::nudge_follow_ups))
        .route("/v1/replies", get(list_all_replies))
        .route("/v1/replies/csv", get(listing::replies_csv))
        .route("/v1/replies/resend", post(listing::resend_failed_replies))
        .route(
            "/v1/bounced",
            get(list_bounced).delete(listing::clear_bounced_listed),
        )
        .route("/v1/bounced/csv", get(listing::bounced_csv))
        .route(
            "/v1/bounced/:address",
            axum::routing::delete(listing::clear_bounced),
        )
        .route(
            "/v1/drafts",
            get(drafts::list_drafts)
                .post(drafts::create_draft)
                .delete(drafts::delete_drafts),
        )
        .route("/v1/drafts/csv", get(drafts::drafts_csv))
        .route("/v1/drafts/merge", post(drafts::merge_drafts))
        .route(
            "/v1/drafts/schedule",
            post(drafts::schedule_drafts).delete(drafts::unschedule_drafts),
        )
        .route("/v1/drafts/send", post(drafts::send_drafts))
        .route("/v1/outbound/:id/draft", post(drafts::draft_from_outbound))
        .route(
            "/v1/drafts/:id",
            get(drafts::get_draft)
                .put(drafts::update_draft)
                .delete(drafts::delete_draft),
        )
        .route("/v1/drafts/:id/send", post(drafts::send_draft))
        .route("/v1/drafts/:id/copy", post(drafts::copy_draft))
        .route(
            "/v1/drafts/:id/schedule",
            post(drafts::schedule_draft).delete(drafts::unschedule_draft),
        )
        .route("/v1/subscriptions", get(list_subscriptions))
        .route("/v1/subscriptions/csv", get(unsubscribe::subscriptions_csv))
        .route("/v1/contacts", get(list_contacts))
        .route("/v1/contacts/vcard", get(contacts_vcard))
        .route("/v1/contacts/csv", get(contacts_csv))
        .route(
            "/v1/contacts/suppress",
            post(unsubscribe::suppress_contacts),
        )
        .route("/v1/contacts/block", post(unsubscribe::block_contacts))
        .route("/v1/tags", get(list_tags))
        .route("/v1/tag-counts/csv", get(tags::tags_csv))
        .route("/v1/tags/:tag", axum::routing::delete(remove_tag))
        .route("/v1/tags/:tag/rename", post(rename_tag))
        .route("/v1/views", get(views::list_views))
        .route(
            "/v1/view-document",
            get(views::export_views).post(views::import_views),
        )
        .route(
            "/v1/views/:name",
            get(views::get_view)
                .put(views::save_view)
                .delete(views::delete_view),
        )
        .route("/v1/views/:name/tags", post(retag_view))
        .route("/v1/views/:name/mark", post(mark_view))
        .route("/v1/views/:name/move", post(move_view))
        .route(
            "/v1/views/:name/snooze",
            post(snooze_view).delete(wake_view),
        )
        .route("/v1/views/:name/mute", post(mute_view).delete(unmute_view))
        .route("/v1/views/:name/rename", post(views::rename_view))
        .route("/v1/views/:name/copy", post(views::copy_view))
        .route("/v1/views/:name/forward", post(views::forward_view))
        .route("/v1/views/:name/replies", post(views::reply_view))
        .route("/v1/templates", get(list_templates))
        .route(
            "/v1/template-document",
            get(export_templates).post(import_templates),
        )
        .route(
            "/v1/templates/:name",
            get(get_template).put(save_template).delete(delete_template),
        )
        .route("/v1/templates/:name/rename", post(rename_template))
        .route("/v1/templates/:name/copy", post(copy_template))
        .route("/v1/templates/:name/render", post(render_template))
        .route("/v1/messages/:id/tags", post(retag_message))
        .route("/v1/messages/:id/thread/tags", post(retag_thread))
        .route(
            "/v1/blocked",
            get(blocked::list_blocked).post(blocked::block_sender),
        )
        .route("/v1/blocked/import", post(blocked::import_blocked))
        .route("/v1/blocked/export", get(blocked::export_blocked))
        .route("/v1/blocked/csv", get(blocked::blocked_csv))
        .route(
            "/v1/blocked/:address",
            axum::routing::delete(blocked::unblock_sender),
        )
        .route(
            "/v1/suppressed",
            get(blocked::list_suppressed).post(blocked::suppress),
        )
        .route("/v1/suppressed/import", post(blocked::import_suppressed))
        .route("/v1/suppressed/export", get(blocked::export_suppressed))
        .route("/v1/suppressed/csv", get(blocked::suppressed_csv))
        .route(
            "/v1/suppressed/:entry",
            axum::routing::delete(blocked::unsuppress),
        )
        .route("/v1/recipients/check", post(blocked::check_recipients))
        .route(
            "/v1/tag-rules",
            get(rules::list_rules).post(rules::add_rule),
        )
        .route("/v1/tag-rules/apply", post(rules::apply_rules))
        .route("/v1/tag-rules/export", get(rules::export_rules))
        .route("/v1/tag-rules/import", post(rules::import_rules))
        .route("/v1/tag-rules/:id/apply", post(rules::apply_rule))
        .route("/v1/tag-rules/:id/matches", get(rules::rule_matches))
        .route("/v1/tag-rules/:id/copy", post(rules::copy_rule))
        .route(
            "/v1/tag-rules/:id/pause",
            post(rules::pause_rule).delete(rules::resume_rule),
        )
        .route(
            "/v1/tag-rules/:id",
            get(rules::show_rule)
                .put(rules::replace_rule)
                .delete(rules::remove_rule),
        )
        .route("/v1/messages/:id/unsubscribe", post(unsubscribe))
        .route("/v1/subscriptions/unsubscribe", post(unsubscribe_list))
        .route("/v1/subscriptions/leave", post(unsubscribe::leave_listed))
        .route(
            "/v1/inbound/domains",
            get(list_inbound_domains).post(declare_inbound_domain),
        )
        .route(
            "/v1/inbound/domains/:id",
            axum::routing::delete(remove_inbound_domain),
        )
        .route("/v1/inbound/domains/:id/sync", post(sync_inbound_domain))
        .route("/v1/inbound/messages", get(read_inbound))
        .route_layer(middleware::from_fn_with_state(auth_verifier, require_auth));

    Router::new()
        .route("/healthz", get(health))
        .route("/v1/gmail/oauth/callback", get(gmail_oauth_callback))
        .merge(protected)
        .with_state(state)
}

/// Ready while the service takes up every declared mailbox as it is declared;
/// 503 with the cause once mailbox announcements stopped reaching it.
async fn health(State(state): State<AppState>) -> (StatusCode, Json<HealthResponse>) {
    let watching_stopped = state.watching_stopped().map(str::to_owned);
    let (code, status) = match watching_stopped {
        Some(_) => (StatusCode::SERVICE_UNAVAILABLE, "degraded"),
        None => (StatusCode::OK, "ready"),
    };
    (
        code,
        Json(HealthResponse {
            status,
            product: "skrzynka",
            version: env!("CARGO_PKG_VERSION"),
            schema_version: crate::db::SCHEMA_VERSION,
            watching_stopped,
        }),
    )
}

async fn status(
    State(state): State<AppState>,
    Extension(auth): Extension<AuthContext>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(state.status(&auth.organization_id).await?)))
}

async fn list_skarbiec_items(
    State(state): State<AppState>,
    Extension(_auth): Extension<AuthContext>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(state.list_skarbiec_items().await?)))
}

async fn list_gmail_profiles(
    State(state): State<AppState>,
    Extension(_auth): Extension<AuthContext>,
) -> Result<Json<Value>, AppError> {
    Ok(Json(json!(state.list_gmail_profiles().await?)))
}

pub(super) fn parse_uuid(value: &str) -> Result<Uuid, AppError> {
    Uuid::parse_str(value).map_err(Into::into)
}
