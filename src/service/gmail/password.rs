//! Gmail through an app-specific password, supplied directly or selected in Skarbiec Desktop.

use super::super::AppState;
use crate::{
    error::AppError,
    mail,
    models::{CreateMailboxRequest, Mailbox, SmtpSecurity},
    skarbiec::{ResolvedCredentials, SkarbiecResolver},
};
use lettre::Address;
use std::str::FromStr;

impl AppState {
    /// Connect one Gmail account with an app-specific password supplied
    /// directly to the CLI. Authentication is proved before the credential is
    /// written to Skarbiec or a mailbox row is created.
    pub async fn connect_gmail_app_password(
        &self,
        organization_id: &str,
        email: &str,
        password: &str,
        display_name: Option<String>,
        mailbox_selector: Option<&str>,
    ) -> Result<Mailbox, AppError> {
        let email = validated_gmail_email(email)?;
        if password.is_empty() {
            return Err(AppError::invalid(
                "GMAIL_APP_PASSWORD_INPUT_INVALID",
                "Google app-specific password supplied through stdin must not be empty",
            ));
        }
        let target = mailbox_selector
            .map(|selector| self.resolve_mailbox(organization_id, selector))
            .transpose()?;
        let item_id = SkarbiecResolver::gmail_app_password_item_id(&email)?;
        verify_gmail_app_password(&email, password, &item_id).await?;
        let item_id = self
            .resolver
            .save_gmail_app_password(&email, password)
            .await?;
        let result = match target {
            Some(mailbox) => self.attach_gmail_password_mailbox(mailbox, item_id.clone()),
            None => {
                self.ensure_gmail_password_mailbox(
                    organization_id,
                    item_id.clone(),
                    email.clone(),
                    display_name,
                )
                .await
            }
        };
        result.map_err(|error| {
            AppError::new(
                error.status,
                error.code,
                format!(
                    "Google app-specific password was saved in Skarbiec item '{item_id}', but mailbox '{email}' was not created or updated: {}",
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
        mailbox_selector: Option<&str>,
    ) -> Result<Mailbox, AppError> {
        let target = mailbox_selector
            .map(|selector| self.resolve_mailbox(organization_id, selector))
            .transpose()?;
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
        match target {
            Some(mailbox) => {
                self.attach_gmail_password_mailbox(mailbox, skarbiec_item_id.to_string())
            }
            None => {
                self.ensure_gmail_password_mailbox(
                    organization_id,
                    skarbiec_item_id.to_string(),
                    email,
                    display_name,
                )
                .await
            }
        }
    }

    pub(super) fn attach_gmail_password_mailbox(
        &self,
        mut mailbox: Mailbox,
        skarbiec_item_id: String,
    ) -> Result<Mailbox, AppError> {
        if mailbox.smtp_skarbiec_item_id.is_none() {
            mailbox.smtp_skarbiec_item_id = Some(mailbox.skarbiec_item_id.clone());
        }
        mailbox.skarbiec_item_id = skarbiec_item_id;
        mailbox.imap_host = "imap.gmail.com".to_string();
        mailbox.imap_port = 993;
        mailbox.enabled = true;
        self.database.update_mailbox(&mailbox)
    }
    pub(super) async fn ensure_gmail_password_mailbox(
        &self,
        organization_id: &str,
        skarbiec_item_id: String,
        email: String,
        display_name: Option<String>,
    ) -> Result<Mailbox, AppError> {
        let mut matches = self
            .database
            .list_mailboxes(organization_id)?
            .into_iter()
            .filter(|mailbox| {
                mailbox.skarbiec_item_id == skarbiec_item_id
                    || mailbox.email.eq_ignore_ascii_case(&email)
            })
            .collect::<Vec<_>>();
        if matches.len() > 1 {
            let ids = matches
                .iter()
                .map(|mailbox| mailbox.id.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            return Err(AppError::conflict(
                "MAILBOX_SELECTOR_AMBIGUOUS",
                format!(
                    "{email} names {} mailboxes ({ids}); select one by id",
                    matches.len()
                ),
            ));
        }
        if let Some(mut mailbox) = matches.pop() {
            mailbox.skarbiec_item_id = skarbiec_item_id;
            mailbox.imap_host = "imap.gmail.com".to_string();
            mailbox.imap_port = 993;
            mailbox.enabled = true;
            return self.database.update_mailbox(&mailbox);
        }
        self.create_mailbox(
            organization_id,
            CreateMailboxRequest {
                skarbiec_item_id,
                display_name: display_name.or_else(|| Some(email.clone())),
                email: Some(email),
                imap_host: Some("imap.gmail.com".to_string()),
                imap_port: Some(993),
                smtp_host: Some("smtp.gmail.com".to_string()),
                smtp_port: Some(587),
                smtp_security: Some(SmtpSecurity::Starttls),
                poll_interval_seconds: None,
            },
        )
        .await
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
