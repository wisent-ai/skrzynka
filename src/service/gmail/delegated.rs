//! Gmail through a delegated service account or an OAuth grant.

use super::super::{AppState, GmailOAuthStatusError, GmailOAuthStatusResponse};
use crate::{
    error::AppError,
    gmail::{
        GmailOAuthCallback, GmailOAuthFlowSnapshot, GmailOAuthFlowStatus, StartGmailOAuthRequest,
        StartGmailOAuthResponse,
    },
    models::{CreateMailboxRequest, Mailbox},
};
use lettre::Address;
use std::str::FromStr;
use uuid::Uuid;

impl AppState {
    /// Connect a Workspace mailbox through domain-wide delegation: prove the
    /// grant by minting a token for the address, persist the credential bundle
    /// in Skarbiec, then create or return the mailbox.
    ///
    /// The grant itself is not Skrzynka's to perform. It exists only in the
    /// Workspace admin console, so a missing grant is reported as
    /// `GOOGLE_DELEGATION_NOT_GRANTED` with the client ID, the scope and the
    /// console URL the administrator needs — never attempted from here.
    pub async fn connect_gmail_delegated(
        &self,
        organization_id: &str,
        email: &str,
        display_name: Option<String>,
    ) -> Result<Mailbox, AppError> {
        let email = email.trim();
        Address::from_str(email).map_err(|_| {
            AppError::invalid("GMAIL_PROFILE_INVALID", "email is not a valid address")
        })?;
        let probe_key = format!("delegation-probe:{email}");
        self.resolver
            .delegated_access_token(&probe_key, email)
            .await?;
        let item_id = self
            .resolver
            .save_gmail_delegation(email, display_name.as_deref())
            .await?;
        if let Some(mailbox) = self
            .database
            .list_mailboxes(organization_id)?
            .into_iter()
            .find(|mailbox| mailbox.skarbiec_item_id == item_id)
        {
            return Ok(mailbox);
        }
        self.create_mailbox(
            organization_id,
            CreateMailboxRequest {
                skarbiec_item_id: item_id,
                poll_interval_seconds: None,
            },
        )
        .await
    }

    pub async fn start_gmail_oauth(
        &self,
        organization_id: &str,
        request: StartGmailOAuthRequest,
    ) -> Result<StartGmailOAuthResponse, AppError> {
        self.gmail_oauth.start(organization_id, request).await
    }

    pub async fn complete_gmail_oauth_callback(
        &self,
        callback: GmailOAuthCallback,
    ) -> Result<Mailbox, AppError> {
        let authorization = self.gmail_oauth.complete_callback(callback).await?;
        self.ensure_gmail_mailbox(&authorization).await
    }

    pub async fn gmail_oauth_status(
        &self,
        organization_id: &str,
        flow_id: Uuid,
    ) -> Result<GmailOAuthStatusResponse, AppError> {
        let snapshot = self.gmail_oauth.status(flow_id, organization_id).await?;
        self.gmail_status_response(snapshot).await
    }

    pub(super) async fn gmail_status_response(
        &self,
        snapshot: GmailOAuthFlowSnapshot,
    ) -> Result<GmailOAuthStatusResponse, AppError> {
        let (status, mailbox, error) = match snapshot.status {
            GmailOAuthFlowStatus::Pending => ("pending", None, None),
            GmailOAuthFlowStatus::Processing => ("processing", None, None),
            GmailOAuthFlowStatus::Completed(authorization) => (
                "completed",
                Some(self.ensure_gmail_mailbox(&authorization).await?),
                None,
            ),
            GmailOAuthFlowStatus::Failed(failure) => (
                "failed",
                None,
                Some(GmailOAuthStatusError {
                    code: failure.code,
                    message: failure.message,
                    retryable: failure.retryable,
                }),
            ),
        };
        Ok(GmailOAuthStatusResponse {
            flow_id: snapshot.flow_id,
            status,
            expires_at: snapshot.expires_at.to_rfc3339(),
            mailbox,
            error,
        })
    }

    pub(super) async fn ensure_gmail_mailbox(
        &self,
        authorization: &crate::gmail::GmailAuthorization,
    ) -> Result<Mailbox, AppError> {
        if let Some(mailbox) = self
            .database
            .list_mailboxes(&authorization.organization_id)?
            .into_iter()
            .find(|mailbox| mailbox.skarbiec_item_id == authorization.credential_item_id)
        {
            return Ok(mailbox);
        }
        self.create_mailbox(
            &authorization.organization_id,
            CreateMailboxRequest {
                skarbiec_item_id: authorization.credential_item_id.clone(),
                poll_interval_seconds: None,
            },
        )
        .await
    }
}
