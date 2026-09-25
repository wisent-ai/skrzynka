//! Gmail through an app-specific password, supplied directly or selected in Skarbiec Desktop.

use super::super::AppState;
use crate::{
    error::AppError,
    mail,
    models::Mailbox,
    skarbiec::{ResolvedCredentials, SkarbiecResolver},
};
use lettre::Address;
use std::str::FromStr;

impl AppState {
    /// Connect one Gmail account with an app-specific password supplied
    /// directly to the CLI. Authentication is proved before the credential is
    /// written to Skarbiec and declared a mailbox there.
    pub async fn connect_gmail_app_password(
        &self,
        organization_id: &str,
        email: &str,
        password: &str,
        display_name: Option<String>,
    ) -> Result<Mailbox, AppError> {
        let email = validated_gmail_email(email)?;
        if password.is_empty() {
            return Err(AppError::invalid(
                "GMAIL_APP_PASSWORD_INPUT_INVALID",
                "Google app-specific password supplied through stdin must not be empty",
            ));
        }
        let item_id = SkarbiecResolver::gmail_app_password_item_id(&email)?;
        verify_gmail_app_password(&email, password, &item_id).await?;
        let item_id = self
            .resolver
            .save_gmail_app_password(&email, password, display_name.as_deref())
            .await?;
        self.declare_and_reconcile(organization_id, &item_id)
            .await
            .map_err(|error| {
                AppError::new(
                    error.status,
                    error.code,
                    format!(
                        "Google app-specific password was saved in Skarbiec item '{item_id}', but it was not declared a mailbox: {}",
                        error.message
                    ),
                    error.retryable,
                )
            })
    }

    /// Connect an existing password item selected in Skarbiec Desktop. The
    /// loopback request carries only the item ID; the secret is resolved inside
    /// Skrzynka and never enters the API payload or response.
    pub async fn connect_gmail_app_password_item(
        &self,
        organization_id: &str,
        skarbiec_item_id: &str,
        display_name: Option<String>,
    ) -> Result<Mailbox, AppError> {
        let credentials = self.resolver.resolve_credentials(skarbiec_item_id).await?;
        let (email, password) = match credentials {
            ResolvedCredentials::Password { username, password } => {
                (validated_gmail_email(&username)?, password)
            }
            ResolvedCredentials::OAuth2 { .. } => {
                return Err(AppError::invalid(
                    "GMAIL_APP_PASSWORD_ITEM_INVALID",
                    "selected Skarbiec item does not contain a password credential",
                ));
            }
        };
        verify_gmail_app_password(&email, &password, skarbiec_item_id).await?;
        let item_id = self
            .resolver
            .save_gmail_app_password(&email, &password, display_name.as_deref())
            .await?;
        self.declare_and_reconcile(organization_id, &item_id).await
    }

    /// Tag one item `skrzynka:mailbox` in Skarbiec, reconcile, and return the
    /// mailbox that now stands for it. Every connection path ends here: the
    /// declaration lives in the vault, not in Skrzynka.
    pub(super) async fn declare_and_reconcile(
        &self,
        organization_id: &str,
        skarbiec_item_id: &str,
    ) -> Result<Mailbox, AppError> {
        self.resolver
            .resolve_mailbox_config(skarbiec_item_id, self.poll_interval_seconds)
            .await?;
        self.resolver
            .set_mailbox_declared(skarbiec_item_id, true)
            .await?;
        self.reconcile_mailboxes(Some(organization_id)).await?;
        self.database
            .list_all_mailboxes()?
            .into_iter()
            .find(|mailbox| mailbox.skarbiec_item_id == skarbiec_item_id)
            .ok_or_else(|| AppError::not_found("mailbox"))
    }
}

fn validated_gmail_email(email: &str) -> Result<String, AppError> {
    let email = email.trim();
    Address::from_str(email)
        .map_err(|_| AppError::invalid("GMAIL_PROFILE_INVALID", "email is not a valid address"))?;
    Ok(email.to_string())
}

async fn verify_gmail_app_password(
    email: &str,
    password: &str,
    skarbiec_item_id: &str,
) -> Result<(), AppError> {
    let owned_email = email.to_string();
    let password = password.to_string();
    tokio::task::spawn_blocking(move || mail::verify_gmail_app_password(&owned_email, &password))
        .await
        .map_err(|_| AppError::internal("Gmail credential verification stopped unexpectedly"))?
        .map_err(|refusal| refusal.into_error(email, skarbiec_item_id))
}
