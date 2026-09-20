//! The Gmail subcommands: the browser authorization and the app-password prompt.

use super::{print_json, AUTHORIZATION_POLL_MILLIS, LOCAL_CLI_ORGANIZATION};
use crate::{
    db::Database, error::AppError, gmail::StartGmailOAuthRequest, service::AppState,
    skarbiec::SkarbiecResolver,
};
use axum::http::StatusCode;
use std::{
    io::{self, Read},
    net::SocketAddr,
};

pub(super) async fn authorize_gmail(
    database: Database,
    resolver: SkarbiecResolver,
    skarbiec_item: String,
    bind: SocketAddr,
) -> Result<(), AppError> {
    if !bind.ip().is_loopback() {
        return Err(AppError::new(
            StatusCode::BAD_REQUEST,
            "NON_LOOPBACK_BIND_REFUSED",
            "Skrzynka serves OAuth callbacks only on loopback addresses",
            false,
        ));
    }
    let callback_base_url = format!("http://{bind}");
    let state = AppState::new(database, resolver, 60, &callback_base_url)?;
    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .map_err(|_| AppError::internal("loopback OAuth callback address could not be bound"))?;
    let flow = state
        .start_gmail_oauth(
            LOCAL_CLI_ORGANIZATION,
            StartGmailOAuthRequest {
                skarbiec_item_id: skarbiec_item,
            },
        )
        .await?;
    print_json(&flow)?;

    let callback_state = state.clone();
    let server =
        tokio::spawn(
            async move { axum::serve(listener, crate::api::router(callback_state)).await },
        );
    loop {
        let status = state
            .gmail_oauth_status(LOCAL_CLI_ORGANIZATION, flow.flow_id)
            .await?;
        if status.status == "completed" {
            server.abort();
            print_json(&status)?;
            return Ok(());
        }
        if status.status == "failed" {
            server.abort();
            let error = status.error.as_ref();
            // A flow that expired without a callback is the shape an
            // unregistered redirect URI takes here: Google refuses inside the
            // browser, nothing ever reaches this listener, and the ten-minute
            // lifetime runs out saying only that it expired. Ask Google why
            // before reporting that, so the operator gets the cause instead of
            // the symptom.
            if error.map(|error| error.code) == Some("GMAIL_OAUTH_FLOW_EXPIRED") {
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
            }
            return Err(AppError::dependency(
                "GMAIL_OAUTH_FAILED",
                error
                    .map(|error| format!("{}: {}", error.code, error.message))
                    .unwrap_or_else(|| "Gmail authorization failed".to_string()),
                error.map(|error| error.retryable).unwrap_or(false),
            ));
        }
        tokio::time::sleep(std::time::Duration::from_millis(AUTHORIZATION_POLL_MILLIS)).await;
    }
}

pub(super) fn read_gmail_app_password() -> Result<String, AppError> {
    const MAX_APP_PASSWORD_BYTES: u64 = 4 * 1024;
    let mut input = Vec::new();
    io::stdin()
        .lock()
        .take(MAX_APP_PASSWORD_BYTES + 1)
        .read_to_end(&mut input)
        .map_err(|_| {
            AppError::invalid(
                "GMAIL_APP_PASSWORD_INPUT_INVALID",
                "Google app-specific password could not be read from stdin",
            )
        })?;
    if input.len() as u64 > MAX_APP_PASSWORD_BYTES {
        return Err(AppError::invalid(
            "GMAIL_APP_PASSWORD_INPUT_INVALID",
            "Google app-specific password supplied through stdin is too long",
        ));
    }
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
