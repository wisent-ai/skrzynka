//! Expecting an answer as a message is sent: `message send`, `message
//! forward` and `message draft send` with `--follow-up <BY>` set the
//! follow-up `message followup set` would, once the provider accepted the
//! message, so a sequence never depends on remembering to come back.

use super::{super::AppState, due_instant, instant};
use crate::{
    error::AppError,
    models::{CreateOutboundRequest, DeliveryStatus, OutboundMessage},
};
use uuid::Uuid;

impl AppState {
    /// Originate mail from one mailbox, as `originate` does. With
    /// `request.follow_up`, the instant is read before anything is claimed
    /// (`FOLLOW_UP_INVALID` sends nothing), and a send that ends `sent`
    /// expects an answer by then. A repeated key answers its first attempt
    /// and leaves its follow-up as it stands; a send that does not end
    /// `sent` gets none, since nothing reached anyone to answer it.
    pub async fn send_outbound(
        &self,
        organization_id: &str,
        mailbox_id: Uuid,
        request: CreateOutboundRequest,
    ) -> Result<OutboundMessage, AppError> {
        let due = request.follow_up.as_deref().map(due_instant).transpose()?;
        let fresh = self
            .database
            .get_outbound_by_key(organization_id, request.idempotency_key.trim())?
            .is_none();
        let sent = self.originate(organization_id, mailbox_id, request).await?;
        if let Some(due) = due {
            if fresh && sent.status == DeliveryStatus::Sent {
                self.database
                    .set_follow_up(organization_id, sent.id, &instant(due))
                    .map_err(|error| {
                        AppError::internal(format!(
                            "outbound message {} was sent, but its follow-up could not be stored: {}; set it with skrzynka message followup set {} --by {}",
                            sent.id,
                            error.message,
                            sent.id,
                            instant(due)
                        ))
                    })?;
            }
        }
        Ok(sent)
    }
}
