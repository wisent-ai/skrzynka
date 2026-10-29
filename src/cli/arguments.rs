//! Every subcommand and argument the `skrzynka` binary accepts; the global
//! options that carry them are `Cli`, beside `run`.
use {
    super::{
        actions::{DeleteArgs, MarkArgs, MoveArgs},
        gmail::MailProvider,
        mailbox::DeclareMailboxArgs,
        suppressed::{RecipientsArgs, SuppressedArgs},
        ServeArgs,
    },
    clap::Subcommand,
    std::net::SocketAddr,
};
#[derive(Subcommand)]
pub(super) enum Command {
    /// Run the loopback HTTP API and read every declared mailbox as its provider reports new mail.
    Serve(ServeArgs),
    /// Report the database, version, mailbox and message counts, and whether `skarbiec` is found.
    Status,
    /// Walk through first use: declare a mailbox, sync it and read its mail.
    Onboarding {
        /// Discard the walkthrough's progress and start again.
        #[arg(long)]
        reset: bool,
    },
    /// Print the binary's version.
    Version,
    /// Declare, list, show, undeclare and remove mailboxes.
    Mailbox {
        #[command(subcommand)]
        command: MailboxCommand,
    },
    /// Connect mail accounts through a provider's paths and report which work; `--provider` names it.
    Account {
        /// Mail provider the account is held by. No provider is assumed.
        #[arg(long, value_enum, global = true)]
        provider: MailProvider,
        #[command(subcommand)]
        command: AccountCommand,
    },
    /// Read, reply to, send and handle stored messages, and read back what went out.
    Message {
        #[command(subcommand)]
        command: MessageCommand,
    },
    /// Import each mailbox's INBOX past its cursor, one message per commit; --inbound-domain reads that domain's feed.
    Sync(super::mailbox::SyncArgs),
}

#[derive(Subcommand)]
pub(super) enum AccountCommand {
    /// Report which connection paths this account can use, each verdict measured against the provider.
    Connection {
        /// The account to measure; without it, only each path's account-independent state.
        #[arg(long)]
        email: Option<String>,
        /// Loopback address `account authorize` would listen on; its callback URI goes to Google.
        #[arg(long)]
        bind: SocketAddr,
    },
    /// Authorize one Google identity through the loopback OAuth callback.
    Authorize {
        /// Skarbiec item that receives the authorized grant.
        #[arg(long)]
        skarbiec_item: String,
        /// Loopback address the OAuth callback listens on; none is assumed: one the client accepts.
        #[arg(long)]
        bind: SocketAddr,
    },
    /// Connect one Workspace mailbox through domain-wide delegation.
    Delegate(super::gmail::ConnectedAccount),
    /// Connect one Gmail account using an app-specific password read from stdin.
    Password(super::gmail::ConnectedAccount),
    /// Revoke an OAuth grant and trash the account's credential item; refused while it is a declared mailbox.
    Disconnect(super::gmail::DisconnectedAccount),
}

/// Skarbiec owns the mailbox list: an item tagged `skrzynka:mailbox` is a mailbox; these read and retag.
#[derive(Subcommand)]
pub(super) enum MailboxCommand {
    /// Tag a Skarbiec item skrzynka:mailbox and import its INBOX; with --inbound-domain, declare a receiving domain.
    Declare(DeclareMailboxArgs),
    /// Remove the skrzynka:mailbox tag; the mailbox keeps its mail and stops being read.
    Undeclare(super::mailbox::MailboxRef),
    /// Every mailbox Skarbiec declares, after reading the vault; with --inbound, every inbound domain.
    List(super::mailbox::ListArgs),
    /// One mailbox's state as Skrzynka holds it.
    Show(super::mailbox::MailboxRef),
    /// How the mailbox's sends ended, by status and refusal code, with their bounces; --since/--until narrow it, --days-csv/--domains-csv write tables.
    Delivery(super::mailbox::DeliveryArgs),
    /// What the mailbox holds, counted: messages, unanswered, snoozed, tagged, bounces and more; --since/--until narrow it, --days-csv writes its days.
    Summary(super::mailbox::SummaryArgs),
    /// The folders the mailbox's provider holds, read now: the names `message move` takes.
    Folders(super::mailbox::MailboxRef),
    /// Read the daily send limit, set it with --per-day or remove it with --clear.
    Limit(super::mailbox::LimitArgs),
    /// Read the signature replies and sent mail carry, set it with --file or remove it with --clear.
    Signature(super::mailbox::SignatureArgs),
    /// Read the automatic reply, set it with --file and --every, remove it with --clear, or read --decisions.
    Responder(super::auto_replies::AutoReplyArgs),
    /// Delete a no longer declared mailbox's local mail; with --inbound, an inbound domain and its mail.
    Remove(super::mailbox::RemoveArgs),
}

/// `message attachment`: the files a stored message carries.
#[derive(Subcommand)]
pub(super) enum AttachmentCommand {
    /// What files a message carries; one imported before they were recorded is read from its provider.
    List(super::attachment::ListArgs),
    /// Read one attachment from the provider and write it to a new file.
    Save(super::attachment::SaveArgs),
    /// Every recorded file of the stored mail, newest first; --name, --type, --over and --sender narrow it.
    Find(super::attachment::FindArgs),
}

/// `message rule`: rules that tag mail as every import writes it.
#[derive(Subcommand)]
pub(super) enum RuleCommand {
    /// Add a rule: a tag and at least one condition.
    Add(super::rules::AddArgs),
    /// The stored messages a rule matches now, newest first, without tagging any.
    Matches(super::rules::RuleRef),
    /// Every rule, oldest first, each with when it was paused; --contains, --paused or --active narrow it.
    List(super::rules::ListArgs),
    /// One rule by its ID, as `rule list` prints it.
    Show(super::rules::RuleRef),
    /// A new rule with a rule's conditions and another tag; the rule copied is left as it is.
    Copy(super::rules::CopyArgs),
    /// Replace a rule's tag, conditions and mailbox, keeping its ID; the tags it gave stay.
    Update(super::rules::UpdateArgs),
    /// Stop a rule tagging imported mail, keeping it; the tags it gave stay.
    Pause(super::rules::RuleRef),
    /// Let a paused rule tag imported mail again; `apply --rule` tags what arrived meanwhile.
    Resume(super::rules::RuleRef),
    /// Remove a rule; the tags it gave stay.
    Remove(super::rules::RuleRef),
    /// Tag the mail already stored by every rule not paused, or by the one --rule names.
    Apply(super::rules::ApplyArgs),
    /// Write the rules `rule list` keeps for the same flags to a new JSON file another installation imports.
    Export(super::rules::ExportArgs),
    /// Add every rule a file `rule export` wrote; all are checked before any is kept.
    Import(super::rules::ImportArgs),
}

/// `message template`: texts kept to reuse in replies and new messages.
#[derive(Subcommand)]
pub(super) enum TemplateCommand {
    /// Save a template's text from a file, replacing one of the same name.
    Save(super::templates::SaveArgs),
    /// Every template, by name; --contains keeps those whose name or text contains a text.
    List(super::templates::ListArgs),
    /// One template and its text.
    Show(super::templates::NameArg),
    /// A template's text with its {{KEY}} placeholders filled from --set KEY=VALUE.
    Render(super::templates::RenderArgs),
    /// Give a template another name, keeping its text.
    Rename(super::templates::RenameArgs),
    /// Keep a template's text under another name too.
    Copy(super::templates::RenameArgs),
    /// Delete a template.
    Delete(super::templates::NameArg),
    /// Write every template, or those holding a text, to a new JSON file another installation imports.
    Export(super::templates::ExportArgs),
    /// Keep every template a file `template export` wrote; all are checked before any is kept.
    Import(super::templates::ImportArgs),
}

/// `message followup`: sent messages you expect an answer to by an instant.
#[derive(Subcommand)]
pub(super) enum FollowupCommand {
    /// Expect an answer to a sent outbound message by an instant.
    Set(super::follow_ups::SetArgs),
    /// Every follow-up and whether it is waiting, due or answered.
    List(super::follow_ups::ListArgs),
    /// Where one sent message's follow-up stands now.
    Show(super::follow_ups::OutboundRef),
    /// Stop expecting an answer: to one sent message, or to every follow-up standing so.
    Clear(super::follow_ups::ClearArgs),
    /// Draft a reminder from a template to every follow-up still owed an answer, narrowed as `list` narrows.
    Nudge(super::follow_ups::NudgeArgs),
}

/// `message view`: listing filters kept under a name; `message list --save-view` keeps one.
#[derive(Subcommand)]
pub(super) enum ViewCommand {
    /// Every view, by name; --count says how many messages each lists now.
    List(super::views::ListArgs),
    /// One view and the filters it keeps.
    Show(super::views::NameArg),
    /// Give a view another name, keeping its filters.
    Rename(super::views::RenameArgs),
    /// Keep a view's filters under a second name too, leaving the view as it is.
    Copy(super::views::RenameArgs),
    /// Delete a view.
    Delete(super::views::NameArg),
    /// Write every view, or those whose name holds a text, to a new JSON file another installation imports.
    Export(super::views::ExportArgs),
    /// Keep every view a file `view export` wrote; all are checked before any is kept.
    Import(super::views::ImportArgs),
}

/// `message draft`: messages written over time, kept in Skrzynka until sent.
#[derive(Subcommand)]
pub(super) enum DraftCommand {
    /// Start a draft from a mailbox.
    Save(super::drafts::SaveArgs),
    /// Start a draft from an originated message: its mailbox, addresses, subject and body.
    Again(super::drafts::OutboundRef),
    /// A new draft holding another's addresses, subject, body and files, from its mailbox or --mailbox.
    Copy(super::drafts::CopyArgs),
    /// Change the fields given and keep the others.
    Update(super::drafts::UpdateArgs),
    /// Every draft, or one mailbox's, most recently changed first; --scheduled and --contains narrow it.
    List(super::drafts::ListArgs),
    /// One draft.
    Show(super::drafts::DraftRef),
    /// Remove a draft, or every draft mentioning --contains.
    Delete(super::drafts::ChosenDrafts),
    /// Send a draft as `message send` would; it is removed once accepted.
    Send(super::drafts::SendArgs),
    /// Have a running `skrzynka serve` send the draft at a later instant.
    Schedule(super::drafts::ScheduleArgs),
    /// Take a draft, or every scheduled draft mentioning --contains, off its schedule; they stay drafts.
    Unschedule(super::drafts::ChosenDrafts),
}

#[derive(Subcommand)]
pub(super) enum MessageCommand {
    /// Imported messages, newest first: every one, or a page with --limit.
    List(super::listing::ListArgs),
    /// One imported message.
    Show(super::reading::MessageRef),
    /// Print every message stored from now on, or after --after, one JSON object per line as it is stored.
    Watch(super::watch::WatchArgs),
    /// List a message's attachments, or save one to a new file.
    Attachment {
        #[command(subcommand)]
        command: AttachmentCommand,
    },
    /// Write messages over time and send them when ready.
    Draft {
        #[command(subcommand)]
        command: DraftCommand,
    },
    /// Every mailing list the stored mail came from, with its ways out; --subscribed keeps those not left.
    Subscriptions(super::reading::SubscriptionsArgs),
    /// Ask the list a message came from, or the list --list names, to stop sending (RFC 8058 or mailto).
    Unsubscribe(super::reading::UnsubscribeArgs),
    /// Mark a message seen, unseen, flagged or unflagged at its provider (IMAP flags).
    Mark(MarkArgs),
    /// Move a message out of INBOX into another folder at its provider.
    Move(MoveArgs),
    /// Delete a message at its provider; Skrzynka keeps its stored copy.
    Delete(DeleteArgs),
    /// Every change asked of a message at its provider, newest first.
    Actions(super::reading::MessageRef),
    /// Reply to a message with the body in a file; terminal or ambiguous, never assumed sent.
    Reply(super::sending::ReplyArgs),
    /// Every reply attempt made to one message, or to every message (of one mailbox), newest first.
    Replies(super::sending::RepliesArgs),
    /// The conversation a message belongs to, oldest first, tied by its headers.
    Thread(super::reading::MessageRef),
    /// Leave a message's conversation, or every conversation of a saved view, and later mail of it, out of listings.
    Mute(super::reading::mutes::MuteArgs),
    /// Show a muted conversation, or every muted conversation of a saved view, in listings again.
    Unmute(super::reading::mutes::MuteArgs),
    /// Save the original message as the provider holds it to a new .eml file.
    Source(super::reading::SourceArgs),
    /// Save every stored message of a mailbox as its original into a new directory.
    Export(super::export::ExportArgs),
    /// Put an export's originals back into a provider folder, none twice.
    Restore(super::export::RestoreArgs),
    /// Check every file of an export against its manifest's SHA-256, without contacting a provider.
    Verify(super::export::VerifyArgs),
    /// Send a stored message on from the mailbox that received it, its original attached.
    Forward(super::forward::ForwardArgs),
    /// Everyone the stored mail came from or originated mail went to; --vcard writes them as vCards.
    Contacts(super::reading::ContactsArgs),
    /// Add or remove your own labels on a stored message; never sent to the provider.
    Tag(super::reading::TagArgs),
    /// Every tag the stored messages carry, most used first; --rename or --remove one everywhere.
    Tags(super::reading::TagsArgs),
    /// Keep your own note on a stored message, or remove it; never sent to the provider.
    Note(super::reading::NoteArgs),
    /// Leave a sender's mail (an address, or @domain) out of listings; nothing is deleted.
    Block(super::reading::SenderArg),
    /// Show a blocked sender's mail in listings again.
    Unblock(super::reading::SenderArg),
    /// Every blocked sender; --count says how many stored messages each leaves out.
    Blocked(super::reading::BlockedArgs),
    /// Every address a stored bounce report says delivery to failed for good; --clear one that works again.
    Bounced(super::reading::BouncedArgs),
    /// Recipients no originated mail may be sent to; --add one with --reason, --import a file of them, or --remove one.
    Suppressed(SuppressedArgs),
    /// How a send would meet each address: suppressed, bounced, or sendable; nothing is sent.
    Recipients(RecipientsArgs),
    /// Leave a message, a view's messages or a conversation out of listings until an instant; nothing changes at the provider.
    Snooze(super::reading::SnoozeArgs),
    /// Show a snoozed message, or every snoozed message of a view or conversation, in listings again now.
    Wake(super::reading::WakeArgs),
    /// Your rules that tag mail as it arrives.
    Rule {
        #[command(subcommand)]
        command: RuleCommand,
    },
    /// Texts kept to reuse in replies and new messages.
    Template {
        #[command(subcommand)]
        command: TemplateCommand,
    },
    /// Sent messages you expect an answer to, and which are owed one.
    Followup {
        #[command(subcommand)]
        command: FollowupCommand,
    },
    /// Listing filters kept under a name.
    View {
        #[command(subcommand)]
        command: ViewCommand,
    },
    /// Send a new message, claimed by its idempotency key before the provider sees it.
    Send(super::sending::SendArgs),
    /// Outbound messages and their delivery state; with --id, one of them.
    Outbound(super::sending::OutboundArgs),
}
