//! The message subcommands: reading mail and its attachments, replying and
//! sending, and changing a message at its provider.

use super::{
    actions::{run_provider, ProviderCommand},
    arguments::MessageCommand,
    attachment::run_attachment,
    drafts,
    forward,
    listing::{self, ListArgs},
    print_json,
    reading::{
        self, MessageRef, NoteArgs, SenderArg, SnoozeArgs, UnsubscribeArgs,
        TagArgs, TagsArgs, WakeArgs,
    },
    sending, suppressed,
};
use crate::{
    error::AppError,
    mail::UnsubscribeRequest,
    service::{AppState, RetagRequest},
};
use std::path::Path;

/// The key a provider-facing send is claimed by. A key drawn per invocation
/// would make a repeated command a second real message, so none is drawn:
/// the caller states the key, and the same key on a repeated command returns
/// the first attempt.
pub(super) fn required_key(key: Option<String>, command: &str) -> Result<String, AppError> {
    match key {
        Some(key) => Ok(key),
        None => Err(AppError::invalid(
            "IDEMPOTENCY_KEY_REQUIRED",
            format!(
                "{command} needs --idempotency-key: the same key on a repeated command returns the first attempt instead of sending again"
            ),
        )),
    }
}

pub(super) async fn run_message(
    state: AppState,
    organization: &str,
    command: MessageCommand,
) -> Result<(), AppError> {
    match command {
        // An address of a declared inbound domain is read from the receiving
        // feed (`skrzynka mailbox declare --inbound-domain`): the feed is read
        // first, then that address's stored mail is listed.
        MessageCommand::List(ListArgs {
            mailbox: None,
            recipient: Some(recipient),
            cc,
            sender,
            contains,
            since,
            until,
            tag,
            list,
            attachment,
            with_attachments,
            bounces,
            include_blocked,
            include_snoozed,
            include_muted,
            unanswered,
            failed_authentication,
            oldest_first,
            snoozed,
            noted,
            muted,
            untagged,
            answered,
            files_over,
            view,
            save_view,
            limit,
            offset,
            csv,
        }) if state.is_inbound_recipient(organization, &recipient)? => {
            let narrowed = contains.is_some() || since.is_some() || until.is_some();
            let flagged = with_attachments || bounces || include_blocked || include_snoozed || include_muted || answered;
            let judged = unanswered || failed_authentication || oldest_first || snoozed || noted || muted || untagged;
            let viewed = view.is_some() || save_view.is_some() || attachment.is_some() || cc.is_some() || files_over.is_some();
            if narrowed || flagged || judged || viewed || tag.is_some() || list.is_some() || csv.is_some() {
                return Err(AppError::invalid(
                    "MESSAGE_FILTER_INVALID",
                    &format!(
                        "{recipient} is an address of a declared inbound domain, whose feed is read by recipient and sender only; leave the other filters out"
                    ),
                ));
            }
            let received = state
                .read_inbound(organization, Some(&recipient), sender.as_deref(), None)
                .await?;
            let after_offset: Vec<_> = match offset {
                Some(offset) => received.into_iter().skip(offset as usize).collect(),
                None => received,
            };
            let page = after_offset.into_iter();
            print_json(&match limit {
                Some(limit) => page.take(limit as usize).collect::<Vec<_>>(),
                None => page.collect::<Vec<_>>(),
            })
        }
        MessageCommand::List(list) => listing::list(&state, organization, list),
        MessageCommand::Show(MessageRef { id }) => print_json(&state.get_message(organization, id)?),
        MessageCommand::Watch(args) => super::watch::watch(&state, organization, args).await,
        MessageCommand::Replies(args) => sending::replies(&state, organization, args).await,
        MessageCommand::Thread(MessageRef { id }) => {
            print_json(&state.message_thread(organization, id)?)
        }
        MessageCommand::Source(source) => reading::save_source(&state, organization, source).await,
        MessageCommand::Export(args) => super::export::export(&state, organization, args).await,
        MessageCommand::Restore(args) => super::export::restore(&state, organization, args).await,
        MessageCommand::Verify(args) => super::export::verify_export(args),
        MessageCommand::Forward(args) => forward::forward(&state, organization, args).await,
        MessageCommand::Contacts(args) => reading::contacts(&state, organization, args),
        MessageCommand::Tag(TagArgs {
            id: Some(id),
            add,
            remove,
            ..
        }) => print_json(&state.retag_message(organization, id, RetagRequest { add, remove })?),
        MessageCommand::Tag(TagArgs {
            view: Some(view),
            add,
            remove,
            ..
        }) => print_json(&state.retag_view(organization, &view, RetagRequest { add, remove })?),
        MessageCommand::Tag(TagArgs {
            thread: Some(thread),
            add,
            remove,
            ..
        }) => print_json(&state.retag_thread(organization, thread, RetagRequest { add, remove })?),
        // Clap requires one of <ID>, --view and --thread, and refuses two.
        MessageCommand::Tag(TagArgs { .. }) => Err(AppError::invalid(
            "MESSAGE_TAG_INVALID",
            "name a message ID, --view <NAME> or --thread <ID> to change",
        )),
        MessageCommand::Tags(TagsArgs { rename: Some(from), to: Some(to), .. }) => {
            print_json(&state.rename_tag(organization, &from, &to)?)
        }
        MessageCommand::Tags(TagsArgs { remove: Some(tag), .. }) => {
            print_json(&state.remove_tag(organization, &tag)?)
        }
        MessageCommand::Tags(args) => reading::list_tags(&state, organization, args),
        MessageCommand::Note(NoteArgs { id, file, append, .. }) => {
            let note = file
                .map(|file| read_body_file(&file, "MESSAGE_NOTE_INVALID"))
                .transpose()?;
            match (append, note) {
                (true, Some(text)) => print_json(&state.append_message_note(organization, id, &text)?),
                (_, note) => print_json(&state.set_message_note(organization, id, note.as_deref())?),
            }
        }
        MessageCommand::Block(SenderArg { address }) => {
            print_json(&state.block_sender(organization, &address)?)
        }
        MessageCommand::Unblock(SenderArg { address }) => {
            print_json(&state.unblock_sender(organization, &address)?)
        }
        MessageCommand::Blocked(args) => reading::blocked(&state, organization, args),
        MessageCommand::Bounced(args) => reading::bounced(&state, organization, args),
        MessageCommand::Suppressed(args) => suppressed::suppressed(&state, organization, args),
        MessageCommand::Recipients(args) => suppressed::check_recipients(&state, organization, args),
        MessageCommand::Snooze(SnoozeArgs {
            id: Some(id),
            until,
            ..
        }) => print_json(&state.snooze_message(organization, id, &until)?),
        MessageCommand::Snooze(SnoozeArgs {
            view: Some(view),
            until,
            ..
        }) => print_json(&state.snooze_view(organization, &view, &until)?),
        MessageCommand::Snooze(SnoozeArgs {
            thread: Some(thread),
            until,
            ..
        }) => print_json(&state.snooze_thread(organization, thread, &until)?),
        // Clap requires one of <ID>, --view and --thread, and refuses two.
        MessageCommand::Snooze(SnoozeArgs { .. }) => Err(AppError::invalid(
            "SNOOZE_INVALID",
            "name a message ID, --view <NAME> or --thread <ID> to snooze",
        )),
        MessageCommand::Wake(WakeArgs { id: Some(id), .. }) => {
            print_json(&state.wake_message(organization, id)?)
        }
        MessageCommand::Wake(WakeArgs { view: Some(view), .. }) => {
            print_json(&state.wake_view(organization, &view)?)
        }
        MessageCommand::Wake(WakeArgs { thread: Some(thread), .. }) => {
            print_json(&state.wake_thread(organization, thread)?)
        }
        // Clap requires one of <ID>, --view and --thread, and refuses two.
        MessageCommand::Wake(WakeArgs { .. }) => Err(AppError::invalid(
            "SNOOZE_INVALID",
            "name a message ID, --view <NAME> or --thread <ID> to wake",
        )),
        MessageCommand::Mute(args) => super::reading::mutes::mute(&state, organization, args, true),
        MessageCommand::Unmute(args) => super::reading::mutes::mute(&state, organization, args, false),
        MessageCommand::Rule { command } => super::rules::run_rule(&state, organization, command),
        MessageCommand::Template { command } => {
            super::templates::run_template(&state, organization, command)
        }
        MessageCommand::Followup { command } => {
            super::follow_ups::run_follow_up(&state, organization, command)
        }
        MessageCommand::View { command } => super::views::run_view(&state, organization, command),
        MessageCommand::Reply(args) => sending::reply(&state, organization, args).await,
        MessageCommand::Attachment { command } => {
            run_attachment(state, organization, command).await
        }
        MessageCommand::Draft { command } => drafts::run_draft(&state, organization, command).await,
        MessageCommand::Subscriptions(args) => reading::subscriptions(&state, organization, args).await,
        MessageCommand::Unsubscribe(UnsubscribeArgs { id, list, mailbox, idempotency_key }) => {
            let request = UnsubscribeRequest { idempotency_key: required_key(idempotency_key, "message unsubscribe")? };
            match (id, list) {
                (Some(id), _) => print_json(&state.unsubscribe(organization, id, request).await?),
                (None, Some(list)) => {
                    let mailbox_id = reading::chosen_mailbox(&state, organization, mailbox)?;
                    print_json(&state.unsubscribe_list(organization, &list, mailbox_id, request).await?)
                }
                (None, None) => Err(AppError::invalid("UNSUBSCRIBE_LIST_INVALID", "name a message ID or a --list")),
            }
        }
        MessageCommand::Mark(args) => run_provider(state, organization, ProviderCommand::Mark(args)).await,
        MessageCommand::Move(args) => run_provider(state, organization, ProviderCommand::Move(args)).await,
        MessageCommand::Delete(args) => {
            run_provider(state, organization, ProviderCommand::Delete(args)).await
        }
        MessageCommand::Actions(MessageRef { id }) => {
            run_provider(state, organization, ProviderCommand::Actions(id)).await
        }
        MessageCommand::Send(args) => sending::send(&state, organization, args).await,
        MessageCommand::Outbound(args) => sending::outbound(&state, organization, args).await,
    }
}

pub(super) fn read_body_file(path: &Path, code: &'static str) -> Result<String, AppError> {
    let metadata = std::fs::metadata(path).map_err(|error| {
        AppError::invalid(
            code,
            format!(
                "message body file {} could not be read: {error}",
                path.display()
            ),
        )
    })?;
    if !metadata.is_file() {
        return Err(AppError::invalid(
            code,
            format!("message body file {} must be a regular file", path.display()),
        ));
    }
    // A permission or I/O failure is not a UTF-8 failure; only the decoding
    // error is reported as one.
    std::fs::read_to_string(path).map_err(|error| match error.kind() {
        std::io::ErrorKind::InvalidData => AppError::invalid(
            code,
            format!(
                "message body file {} must contain valid UTF-8 text: {error}",
                path.display()
            ),
        ),
        _ => AppError::invalid(
            code,
            format!(
                "message body file {} could not be read: {error}",
                path.display()
            ),
        ),
    })
}
