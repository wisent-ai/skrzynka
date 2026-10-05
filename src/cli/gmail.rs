//! The Gmail subcommands: the browser authorization and the app-password prompt.

use super::{loopback_callback, print_json};
use crate::{
    db::Database, error::AppError, gmail::StartGmailOAuthRequest, service::AppState,
    skarbiec::SkarbiecResolver,
};
use std::{
    io::{self, Read},
    net::SocketAddr,
};

pub(super) async fn authorize_gmail(
    database: Database,
    resolver: SkarbiecResolver,
    organization: &str,
    skarbiec_item: String,
    bind: SocketAddr,
) -> Result<(), AppError> {
    let callback_base_url = loopback_callback(bind)?;
    let state = AppState::new(database, resolver, None, Some(&callback_base_url))?;
    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .map_err(|_| AppError::internal("loopback OAuth callback address could not be bound"))?;
    let flow = state
        .start_gmail_oauth(
            organization,
            StartGmailOAuthRequest {
                skarbiec_item_id: skarbiec_item,
            },
        )
        .await?;
    print_json(&flow)?;

    // An unregistered redirect URI is refused by Google inside the browser:
    // nothing ever reaches this listener, so no callback would ever settle the
    // flow. Ask Google before waiting, and report that cause instead.
    if crate::gmail::diagnose_authorization(&flow.authorization_url)
        .await
        .as_deref()
        == Some("redirect_uri_mismatch")
    {
        if let Some((client_id, redirect_uri)) =
            crate::gmail::authorization_operands(&flow.authorization_url)
        {
            return Err(crate::gmail::redirect_not_registered(
                &client_id,
                &redirect_uri,
            ));
        }
    }

    let callback_state = state.clone();
    let server =
        tokio::spawn(
            async move { axum::serve(listener, crate::api::router(callback_state)).await },
        );
    let status = state.gmail_oauth_settled(organization, flow.flow_id).await;
    server.abort();
    let status = status?;
    if status.status == "completed" {
        print_json(&status)?;
        return Ok(());
    }
    let error = status.error.as_ref();
    Err(AppError::dependency(
        "GMAIL_OAUTH_FAILED",
        error
            .map(|error| format!("{}: {}", error.code, error.message))
            .unwrap_or_else(|| "Gmail authorization failed".to_string()),
        error.map(|error| error.retryable).unwrap_or(false),
    ))
}

pub(super) fn read_gmail_app_password() -> Result<String, AppError> {
    let mut input = Vec::new();
    io::stdin().lock().read_to_end(&mut input).map_err(|_| {
        AppError::invalid(
            "GMAIL_APP_PASSWORD_INPUT_INVALID",
            "Google app-specific password could not be read from stdin",
        )
    })?;
    let input = String::from_utf8(input).map_err(|_| {
        AppError::invalid(
            "GMAIL_APP_PASSWORD_INPUT_INVALID",
            "Google app-specific password supplied through stdin must be valid UTF-8",
        )
    })?;
    let password = input
        .trim_end_matches(|character| character == '\r' || character == '\n')
        .to_string();
    if password.is_empty() {
        return Err(AppError::invalid(
            "GMAIL_APP_PASSWORD_INPUT_INVALID",
            "Google app-specific password supplied through stdin must not be empty",
        ));
    }
    Ok(password)
}
