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
    #[arg(long, global = true, value_name = "PATH")]
    pub(super) database: Option<PathBuf>,
    #[arg(long, global = true, default_value = "skarbiec", value_name = "PATH")]
    pub(super) skarbiec_bin: PathBuf,
    #[command(subcommand)]
    pub(super) command: Command,
}

#[derive(Subcommand)]
pub(super) enum Command {
    Serve(ServeArgs),
    Status,
    Onboarding {
        #[arg(long)]
        reset: bool,
    },
    Version,
    Mailbox {
        #[command(subcommand)]
        command: MailboxCommand,
    },
    Gmail {
        #[command(subcommand)]
        command: GmailCommand,
    },
    Message {
        #[command(subcommand)]
        command: MessageCommand,
    },
    Sync {
        #[arg(long)]
        mailbox: Option<Uuid>,
    },
}

#[derive(Args)]
pub(super) struct ServeArgs {
    #[arg(long, default_value = "127.0.0.1:8788")]
    pub(super) bind: SocketAddr,
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
    List {
        #[arg(long)]
        mailbox: Option<Uuid>,
        #[arg(long, default_value_t = 100)]
        limit: u32,
        #[arg(long, default_value_t = 0)]
        offset: u32,
    },
    Show {
        id: Uuid,
    },
    Reply {
        id: Uuid,
        #[arg(long)]
        body_file: PathBuf,
        #[arg(long)]
        idempotency_key: Option<String>,
    },
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
    Outbound {
        #[arg(long)]
        mailbox: Option<String>,
        #[arg(long, default_value_t = 100)]
        limit: u32,
        #[arg(long, default_value_t = 0)]
        offset: u32,
    },
}
