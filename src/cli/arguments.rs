//! Every subcommand and argument the `skrzynka` binary accepts.

use clap::{Args, Parser, Subcommand};
use std::{net::SocketAddr, path::PathBuf};
use uuid::Uuid;

#[derive(Parser)]
#[command(
    name = "skrzynka",
    version,
    about = "Receive and reply across multiple mailboxes without moving credentials out of Skarbiec",
    after_help = "Mailboxes are the Skarbiec items tagged skrzynka:mailbox. Safe first result: skrzynka mailbox declare --skarbiec-item <ITEM_ID>; skrzynka sync while has_more=true"
)]
pub struct Cli {
    /// The organization a command acts for in the fleet database.
    #[arg(long, global = true, default_value = "legacy-local", value_name = "ID")]
    pub(super) organization: String,
    #[arg(long, global = true, default_value = "skarbiec", value_name = "PATH")]
    pub(super) skarbiec_bin: PathBuf,
    #[command(subcommand)]
    pub(super) command: Command,
}

#[derive(Subcommand)]
pub(super) enum Command {
    /// Run the loopback HTTP API and poll every declared mailbox on its interval.
    Serve(ServeArgs),
    /// Report the database path, version, mailbox and message counts, and
    /// whether `skarbiec` can be found.
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
    /// Connect Gmail accounts and report which connection paths work.
    Gmail {
        #[command(subcommand)]
        command: GmailCommand,
    },
    /// List, read, reply to and send messages, and read back what went out.
    Message {
        #[command(subcommand)]
        command: MessageCommand,
    },
    /// Run one bounded import pass; run it again while the result says has_more=true.
    Sync {
        /// Import only this mailbox.
        #[arg(long)]
        mailbox: Option<Uuid>,
    },
}

#[derive(Args)]
pub(super) struct ServeArgs {
    /// Loopback address the API listens on; a non-loopback address is refused.
    #[arg(long, default_value = "127.0.0.1:8788")]
    pub(super) bind: SocketAddr,
    /// Seconds between polls of each mailbox.
    #[arg(long, default_value_t = 60)]
    pub(super) poll_seconds: u64,
}

#[derive(Subcommand)]
pub(super) enum GmailCommand {
    /// Report which Gmail connection paths this account can actually use,
    /// each verdict measured against Google.
    Connection {
        /// The account to measure. Without it only the account-independent
        /// state of each path is reported.
        #[arg(long)]
        email: Option<String>,
    },
    /// Authorize one Google identity through the loopback OAuth callback.
    Authorize {
        #[arg(long)]
        skarbiec_item: String,
        #[arg(long, default_value = "127.0.0.1:8790")]
        bind: SocketAddr,
    },
    /// Connect one Workspace mailbox through domain-wide delegation.
    Delegate {
        #[arg(long)]
        email: String,
        #[arg(long)]
        display_name: Option<String>,
    },
    /// Connect one Gmail account using an app-specific password read from stdin.
    AppPassword {
        #[arg(long)]
        email: String,
        #[arg(long)]
        display_name: Option<String>,
    },
}

/// Skarbiec owns the mailbox list: an item tagged `skrzynka:mailbox` is a
/// mailbox. These commands read Skrzynka's state for those items and edit
/// the tag; they keep no list of their own.
#[derive(Subcommand)]
pub(super) enum MailboxCommand {
    /// Tag a Skarbiec item skrzynka:mailbox and import its first INBOX page.
    Declare(DeclareMailboxArgs),
    /// Remove the skrzynka:mailbox tag; the mailbox keeps its mail and stops polling.
    Undeclare {
        id: Uuid,
    },
    /// Every mailbox Skarbiec declares, after reading the vault.
    List,
    /// One mailbox's state as Skrzynka holds it.
    Show {
        id: Uuid,
    },
    /// Delete the local mail of a mailbox Skarbiec no longer declares.
    Remove {
        id: Uuid,
        #[arg(long)]
        confirm: bool,
    },
}

#[derive(Args)]
pub(super) struct DeclareMailboxArgs {
    #[arg(long)]
    pub(super) skarbiec_item: String,
}

#[derive(Subcommand)]
pub(super) enum MessageCommand {
    /// Imported messages, one page at a time (--limit, --offset).
    List {
        #[arg(long)]
        mailbox: Option<Uuid>,
        #[arg(long, default_value_t = 100)]
        limit: u32,
        #[arg(long, default_value_t = 0)]
        offset: u32,
    },
    /// One imported message.
    Show {
        id: Uuid,
    },
    /// Reply to a message with the body in a file; the result is terminal or
    /// ambiguous, never assumed sent.
    Reply {
        id: Uuid,
        #[arg(long)]
        body_file: PathBuf,
        #[arg(long)]
        idempotency_key: Option<String>,
    },
    /// Send a new message, claimed by its idempotency key before the provider sees it.
    Send {
        #[arg(long)]
        mailbox: String,
        #[arg(long = "to", required = true)]
        to: Vec<String>,
        #[arg(long = "cc")]
        cc: Vec<String>,
        #[arg(long)]
        subject: String,
        #[arg(long)]
        body_file: PathBuf,
        #[arg(long)]
        idempotency_key: Option<String>,
    },
    /// Outbound messages and their delivery state.
    Outbound {
        #[arg(long)]
        mailbox: Option<String>,
        #[arg(long, default_value_t = 100)]
        limit: u32,
        #[arg(long, default_value_t = 0)]
        offset: u32,
    },
}
