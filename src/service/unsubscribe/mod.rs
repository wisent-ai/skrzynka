//! Mailing lists: which lists the stored mail came from, and asking one of
//! them to stop sending by the way out its own message names.

use super::AppState;
use crate::{
    error::AppError,
    mail::{
        self, MailingList, Subscription, Subscriptions, UnsubscribeAttempt, UnsubscribeMethod,
        UnsubscribeRequest, UnsubscribeTarget,
    },
    models::{DeliveryStatus, Message},
};
use std::collections::HashMap;
use uuid::Uuid;

mod filter;
mod list;
mod many;
mod table;
pub use filter::SubscriptionFilter;
pub use table::subscription_table;

impl AppState {
    /// Every mailing list the organization's stored mail came from, grouped
    /// by mailbox and List-Id (or sender, for a list that states no List-Id),
    /// most messages first, narrowed as `filter` says. Messages imported
    /// before list headers were recorded are counted, not guessed into a
    /// list. A blank `contains` or an unreadable or backwards window is
    /// `MESSAGE_FILTER_INVALID`, before any mail is read.
    pub fn list_subscriptions(
        &self,
        organization_id: &str,
        filter: &SubscriptionFilter,
    ) -> Result<Subscriptions, AppError> {
        let narrowing = filter.checked()?;
        let messages =
            self.database
                .list_messages(organization_id, filter.mailbox_id, None, None)?;
        let attempts = self.database.list_unsubscribes(organization_id)?;
        let mut messages_not_read = Vec::new();
        // Each list's messages, newest first, in the order lists are first seen.
        let mut groups: Vec<(&Message, &MailingList, Vec<Uuid>)> = Vec::new();
        let mut places: HashMap<(Uuid, String), usize> = HashMap::new();
        for message in &messages {
            let Some(list) = message.mailing_list.as_ref() else {
                messages_not_read.push(message.id);
                continue;
            };
            if list.list_id.is_none() && list.unsubscribe.is_none() {
                continue;
            }
            let key = match list.list_id.as_deref() {
                Some(list_id) => list_id.trim().to_ascii_lowercase(),
                None => message.sender.trim().to_ascii_lowercase(),
            };
            match places.get(&(message.mailbox_id, key.clone())) {
                Some(&place) => groups[place].2.push(message.id),
                None => {
                    places.insert((message.mailbox_id, key), groups.len());
                    groups.push((message, list, vec![message.id]));
                }
            }
        }
        let mut lists: Vec<Subscription> = groups
            .into_iter()
            .map(|(newest, list, ids)| {
                // Attempts arrive newest first.
                let last_attempt = attempts
                    .iter()
                    .find(|attempt| ids.contains(&attempt.message_id))
                    .cloned();
                subscription(newest, list, ids.len(), last_attempt)
            })
            .collect();
        lists.sort_by(|left, right| right.messages.cmp(&left.messages));
        Ok(Subscriptions {
            lists: narrowing.narrow(lists)?,
            messages_not_read: messages_not_read.len(),
        })
    }

    /// Ask the list `message_id` came from to stop sending. A one-click
    /// HTTPS address is used first, then a mail address; a list that names
    /// only pages a person opens is refused. The request is claimed by its
    /// idempotency key before the list is asked, and a repeated key returns
    /// the first attempt.
    pub async fn unsubscribe(
        &self,
        organization_id: &str,
        message_id: Uuid,
        request: UnsubscribeRequest,
    ) -> Result<UnsubscribeAttempt, AppError> {
        let key = super::outbound::checked_key(&request.idempotency_key)?;
        let message = self.database.get_message(organization_id, message_id)?;
        let list = self.mailing_list(organization_id, &message).await?;
        let targets = list.targets().map_err(|reason| {
            AppError::invalid(
                "UNSUBSCRIBE_UNAVAILABLE",
                format!("message {message_id} names no way to unsubscribe: {reason}"),
            )
        })?;
        let Some((method, chosen)) = choose(&targets) else {
            let pages = targets
                .iter()
                .map(address_of)
                .collect::<Vec<_>>()
                .join(", ");
            return Err(AppError::invalid(
                "UNSUBSCRIBE_NEEDS_BROWSER",
                format!(
                    "message {message_id} names only pages a person opens ({pages}); Skrzynka opens no browser, so open one of them yourself"
                ),
            ));
        };
        let (attempt, created) = self.database.begin_unsubscribe(
            organization_id,
            message_id,
            key,
            method,
            address_of(chosen),
        )?;
        if !created {
            return Ok(attempt);
        }
        let attempt = self.database.update_unsubscribe(
            attempt.id,
            DeliveryStatus::Sending,
            None,
            None,
            None,
        )?;
        let outcome = match chosen {
            UnsubscribeTarget::Mailto {
                address,
                subject,
                body,
            } => self
                .mail_unsubscribe(organization_id, &message, address, subject, body)
                .await
                .map(Some),
            one_click => mail::post_one_click(&reqwest::Client::new(), address_of(one_click))
                .await
                .map(|_| None),
        };
        self.settle_unsubscribe(attempt.id, outcome)
    }

    /// The message's list headers: as recorded, or, for a message imported
    /// before they were recorded, read from the provider and recorded now.
    async fn mailing_list(
        &self,
        organization_id: &str,
        message: &Message,
    ) -> Result<MailingList, AppError> {
        if let Some(list) = &message.mailing_list {
            return Ok(list.clone());
        }
        let (provider, _) = self.read_provider_message(organization_id, message).await?;
        self.database
            .record_message_details(organization_id, message.id, &provider)?;
        Ok(provider.mailing_list)
    }

    /// Mail the list's address from the mailbox that received the message.
    async fn mail_unsubscribe(
        &self,
        organization_id: &str,
        message: &Message,
        address: &str,
        subject: &Option<String>,
        body: &Option<String>,
    ) -> Result<String, AppError> {
        let mailbox = self
            .database
            .get_mailbox(organization_id, message.mailbox_id)?;
        let credentials = self
            .resolver
            .resolve_credentials(mailbox.outbound_skarbiec_item_id())
            .await?;
        let (address, subject, body) = (address.to_string(), subject.clone(), body.clone());
        tokio::task::spawn_blocking(move || {
            mail::send_unsubscribe_mail(
                &mailbox,
                &credentials,
                &address,
                subject.as_deref(),
                body.as_deref(),
            )
        })
        .await
        .map_err(|stopped| {
            AppError::dependency(
                "UNSUBSCRIBE_UNCERTAIN",
                format!(
                    "the unsubscribe mail task stopped before the SMTP server answered: {stopped}"
                ),
                false,
            )
        })?
    }

    /// Record how the request ended: accepted, refused, or, when the SMTP
    /// server's answer was lost, uncertain.
    fn settle_unsubscribe(
        &self,
        id: Uuid,
        outcome: Result<Option<String>, AppError>,
    ) -> Result<UnsubscribeAttempt, AppError> {
        match outcome {
            Ok(provider_message_id) => self.database.update_unsubscribe(
                id,
                DeliveryStatus::Sent,
                provider_message_id.as_deref(),
                None,
                None,
            ),
            Err(error)
                if error.code == "SMTP_UNCERTAIN" || error.code == "UNSUBSCRIBE_UNCERTAIN" =>
            {
                self.database
                    .update_unsubscribe(
                        id,
                        DeliveryStatus::Uncertain,
                        None,
                        Some("UNSUBSCRIBE_UNCERTAIN"),
                        Some(&error.message),
                    )
                    .map_err(|write| error.with_unrecorded::<()>(Err(write), "sending"))
            }
            Err(error) => {
                let recorded = self.database.update_unsubscribe(
                    id,
                    DeliveryStatus::Failed,
                    None,
                    Some(error.code),
                    Some(&error.message),
                );
                Err(error.with_unrecorded(recorded, "sending"))
            }
        }
    }
}

/// A one-click address first, since it needs nothing of the mailbox; then a
/// mail address. Pages are left to a person.
fn choose(targets: &[UnsubscribeTarget]) -> Option<(UnsubscribeMethod, &UnsubscribeTarget)> {
    let one_click = targets
        .iter()
        .find(|target| matches!(target, UnsubscribeTarget::OneClick { .. }))
        .map(|target| (UnsubscribeMethod::OneClick, target));
    one_click.or_else(|| {
        targets
            .iter()
            .find(|target| matches!(target, UnsubscribeTarget::Mailto { .. }))
            .map(|target| (UnsubscribeMethod::Mailto, target))
    })
}

/// The address a way out names: its URL, or its mail address.
fn address_of(target: &UnsubscribeTarget) -> &str {
    match target {
        UnsubscribeTarget::OneClick { url } | UnsubscribeTarget::Page { url } => url,
        UnsubscribeTarget::Mailto { address, .. } => address,
    }
}

/// A list as its newest stored message shows it.
fn subscription(
    newest: &Message,
    list: &MailingList,
    messages: usize,
    last_attempt: Option<UnsubscribeAttempt>,
) -> Subscription {
    let (ways_out, ways_out_unreadable) = match list.targets() {
        Ok(targets) => (targets, None),
        Err(reason) => (Vec::new(), Some(reason)),
    };
    Subscription {
        list_id: list.list_id.clone(),
        sender: newest.sender.clone(),
        mailbox_id: newest.mailbox_id,
        messages,
        latest_message_id: newest.id,
        latest_received_at: newest.received_at.clone(),
        ways_out,
        ways_out_unreadable,
        last_attempt,
    }
}
