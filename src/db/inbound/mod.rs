//! Inbound domain rows and the messages received for them.

use super::sql::{self, params, OptionalExtension, Row};
use super::{parse_uuid, Database};
use crate::{
    error::AppError,
    models::{InboundDomain, InboundMessage},
};
use chrono::Utc;
use uuid::Uuid;

const DOMAIN_COLUMNS: &str = "id, organization_id, domain, last_sync_at, last_error_code,
     last_error_message, created_at, updated_at";
const MESSAGE_COLUMNS: &str =
    "id, domain_id, provider_id, sender, recipients, subject, received_at, body_text";

impl Database {
    /// Declare `domain` for the organization, or answer the declaration it
    /// already has: declaring twice is the same declaration.
    pub fn declare_inbound_domain(
        &self,
        organization_id: &str,
        domain: &str,
    ) -> Result<InboundDomain, AppError> {
        let now = Utc::now().to_rfc3339();
        self.lock()?.execute(
            "INSERT INTO inbound_domains (id, organization_id, domain, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $4)
             ON CONFLICT (organization_id, domain) DO NOTHING",
            params![Uuid::new_v4().to_string(), organization_id, domain, now],
        )?;
        self.inbound_domain_named(organization_id, domain)?
            .ok_or_else(|| {
                AppError::internal(format!(
                    "inbound domain {domain} was declared, but reading it back found no row"
                ))
            })
    }

    pub fn list_inbound_domains(
        &self,
        organization_id: &str,
    ) -> Result<Vec<InboundDomain>, AppError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(&format!(
            "SELECT {DOMAIN_COLUMNS} FROM inbound_domains WHERE organization_id=$1 ORDER BY domain"
        ))?;
        let rows = statement.query_map(params![organization_id], domain_from_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// The organization's declaration of `domain`, compared without case.
    pub fn inbound_domain_named(
        &self,
        organization_id: &str,
        domain: &str,
    ) -> Result<Option<InboundDomain>, AppError> {
        Ok(self
            .lock()?
            .query_row(
                &format!(
                    "SELECT {DOMAIN_COLUMNS} FROM inbound_domains
                     WHERE organization_id=$1 AND lower(domain)=lower($2)"
                ),
                params![organization_id, domain],
                domain_from_row,
            )
            .optional()?)
    }

    pub fn get_inbound_domain(
        &self,
        organization_id: &str,
        id: Uuid,
    ) -> Result<InboundDomain, AppError> {
        self.lock()?
            .query_row(
                &format!(
                    "SELECT {DOMAIN_COLUMNS} FROM inbound_domains WHERE id=$1 AND organization_id=$2"
                ),
                params![id.to_string(), organization_id],
                domain_from_row,
            )
            .optional()?
            .ok_or_else(|| AppError::not_found("inbound domain"))
    }

    /// Delete the declaration and every message received for it.
    pub fn delete_inbound_domain(&self, organization_id: &str, id: Uuid) -> Result<(), AppError> {
        self.get_inbound_domain(organization_id, id)?;
        self.lock()?.execute(
            "DELETE FROM inbound_domains WHERE id=$1 AND organization_id=$2",
            params![id.to_string(), organization_id],
        )?;
        Ok(())
    }

    /// Record how the last read of the receiving feed ended: a success clears
    /// the stored failure, a failure keeps its code and cause.
    pub fn record_inbound_read(
        &self,
        id: Uuid,
        failure: Option<(&str, &str)>,
    ) -> Result<(), AppError> {
        let now = Utc::now().to_rfc3339();
        let connection = self.lock()?;
        match failure {
            None => connection.execute(
                "UPDATE inbound_domains SET last_sync_at=$2, last_error_code=NULL,
                        last_error_message=NULL, updated_at=$2 WHERE id=$1",
                params![id.to_string(), now],
            )?,
            Some((code, message)) => connection.execute(
                "UPDATE inbound_domains SET last_error_code=$2, last_error_message=$3,
                        updated_at=$4 WHERE id=$1",
                params![id.to_string(), code, message, now],
            )?,
        };
        Ok(())
    }

    /// Whether the provider message `provider_id` is already stored.
    pub fn inbound_message_stored(
        &self,
        domain_id: Uuid,
        provider_id: &str,
    ) -> Result<bool, AppError> {
        Ok(self
            .lock()?
            .query_row(
                "SELECT id FROM inbound_messages WHERE domain_id=$1 AND provider_id=$2",
                params![domain_id.to_string(), provider_id],
                |row| row.get::<_, String>("id"),
            )
            .optional()?
            .is_some())
    }

    /// Store one received message; one already stored is left as it is.
    pub fn store_inbound_message(&self, message: &InboundMessage) -> Result<(), AppError> {
        self.lock()?.execute(
            "INSERT INTO inbound_messages (
                id, domain_id, provider_id, sender, recipients, subject, received_at, body_text
             ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT (domain_id, provider_id) DO NOTHING",
            params![
                message.id.to_string(),
                message.domain_id.to_string(),
                message.provider_id,
                message.sender,
                message.recipients,
                message.subject,
                message.received_at,
                message.body_text
            ],
        )?;
        Ok(())
    }

    /// Every message stored for the domain, newest first.
    pub fn list_inbound_messages(&self, domain_id: Uuid) -> Result<Vec<InboundMessage>, AppError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(&format!(
            "SELECT {MESSAGE_COLUMNS} FROM inbound_messages
             WHERE domain_id=$1 ORDER BY received_at DESC"
        ))?;
        let rows = statement.query_map(params![domain_id.to_string()], message_from_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }
}

fn domain_from_row(row: &Row<'_>) -> sql::Result<InboundDomain> {
    Ok(InboundDomain {
        id: parse_uuid(row.get::<_, String>("id")?)?,
        organization_id: row.get("organization_id")?,
        domain: row.get("domain")?,
        last_sync_at: row.get("last_sync_at")?,
        last_error_code: row.get("last_error_code")?,
        last_error_message: row.get("last_error_message")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

fn message_from_row(row: &Row<'_>) -> sql::Result<InboundMessage> {
    Ok(InboundMessage {
        id: parse_uuid(row.get::<_, String>("id")?)?,
        domain_id: parse_uuid(row.get::<_, String>("domain_id")?)?,
        provider_id: row.get("provider_id")?,
        sender: row.get("sender")?,
        recipients: row.get("recipients")?,
        subject: row.get("subject")?,
        received_at: row.get("received_at")?,
        body_text: row.get("body_text")?,
    })
}
