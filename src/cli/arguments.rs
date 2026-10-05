//! Every subcommand and argument the `skrzynka` binary accepts.

use clap::{Args, Parser, Subcommand};
use std::{net::SocketAddr, path::PathBuf};
use uuid::Uuid;

#[derive(Parser)]
#[command(
    name = "skrzynka",
    version,
    about = "Receive and reply across multiple mailboxes without moving credentials out of Skarbiec",
    after_help = "Mailboxes are the Skarbiec items tagged skrzynka:mailbox. Safe first result: skrzynka mailbox declare --skarbiec-item <ITEM_ID>, which imports every message the INBOX holds"
)]
pub struct Cli {
    /// The organization a command acts for in the fleet database. Every
    /// command except `serve`, `version` and `onboarding` needs one; none is assumed.
    #[arg(long, global = true, value_name = "ID")]
    pub(super) organization: Option<String>,
    /// The `skarbiec` executable that reads and tags mailbox items.
    #[arg(long, global = true, default_value = "skarbiec", value_name = "PATH")]
    pub(super) skarbiec_bin: PathBuf,
    /// Print results as `path: value` lines for people instead of JSON.
    #[arg(long, global = true)]
    pub(super) text: bool,
    #[command(subcommand)]
    pub(super) command: Command,
}

#[derive(Subcommand)]
pub(super) enum Command {
    /// Run the loopback HTTP API and read every declared mailbox as its provider reports new mail.
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
    /// Connect mail accounts through a provider's connection paths and report
    /// which of them work; `--provider` names the provider.
    Account {
        /// Mail provider the account is held by. No provider is assumed.
        #[arg(long, value_enum, global = true)]
        provider: MailProvider,
        #[command(subcommand)]
        command: AccountCommand,
    },
    /// List, read, reply to and send messages, and read back what went out.
    Message {
        #[command(subcommand)]
        command: MessageCommand,
    },
    /// Import every message each mailbox's INBOX holds past its cursor, one message per commit.
    Sync {
        /// Import only this mailbox.
        #[arg(long)]
        mailbox: Option<Uuid>,
    },
}

#[derive(Args)]
pub(super) struct ServeArgs {
    /// Loopback address the API listens on; a non-loopback address is refused.
    /// No address is assumed: the service declaration that runs `serve` names it.
    #[arg(long)]
    pub(super) bind: SocketAddr,
}

/// The mail providers `skrzynka account` implements.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub(super) enum MailProvider {
    /// Google Gmail and Workspace: app-specific password, OAuth, delegation.
    Gmail,
}

#[derive(Subcommand)]
pub(super) enum AccountCommand {
    /// Report which connection paths this account can actually use, each
    /// verdict measured against the provider.
    Connection {
        /// The account to measure. Without it only the account-independent
        /// state of each path is reported.
        #[arg(long)]
        email: Option<String>,
        /// Loopback address `account authorize` would listen on; its
        /// callback URI is the one handed to Google for the OAuth verdict.
        #[arg(long)]
        bind: SocketAddr,
    },
    /// Authorize one Google identity through the loopback OAuth callback.
    Authorize {
        /// Skarbiec item that receives the authorized grant.
        #[arg(long)]
        skarbiec_item: String,
        /// Loopback address the OAuth callback listens on. No address is
        /// assumed: it has to be one the OAuth client accepts.
        #[arg(long)]
        bind: SocketAddr,
    },
    /// Connect one Workspace mailbox through domain-wide delegation.
    Delegate {
        /// Workspace address to connect.
        #[arg(long)]
        email: String,
        /// Name shown for the mailbox; the address when omitted.
        #[arg(long)]
        display_name: Option<String>,
    },
    /// Connect one Gmail account using an app-specific password read from stdin.
    AppPassword {
        /// Gmail address to connect.
        #[arg(long)]
        email: String,
        /// Name shown for the mailbox; the address when omitted.
        #[arg(long)]
        display_name: Option<String>,
    },
    /// Take back what a connection path wrote: revoke an OAuth grant at the
    /// provider and move the account's credential item to Skarbiec's trash.
    /// Refused while the account is a declared mailbox (`mailbox undeclare`).
    Disconnect {
        /// Account to disconnect.
        #[arg(long)]
        email: String,
    },
}

/// Skarbiec owns the mailbox list: an item tagged `skrzynka:mailbox` is a
/// mailbox. These commands read Skrzynka's state for those items and edit
/// the tag; they keep no list of their own.
#[derive(Subcommand)]
pub(super) enum MailboxCommand {
    /// Tag a Skarbiec item skrzynka:mailbox and import every message its INBOX holds.
    Declare(DeclareMailboxArgs),
    /// Remove the skrzynka:mailbox tag; the mailbox keeps its mail and stops being read.
    Undeclare {
        /// Mailbox ID, as `skrzynka mailbox list` prints it.
        id: Uuid,
    },
    /// Every mailbox Skarbiec declares, after reading the vault.
    List,
    /// One mailbox's state as Skrzynka holds it.
    Show {
        /// Mailbox ID, as `skrzynka mailbox list` prints it.
        id: Uuid,
    },
    /// Delete the local mail of a mailbox Skarbiec no longer declares.
    Remove {
        /// Mailbox ID, as `skrzynka mailbox list` prints it.
        id: Uuid,
        /// Required: the local mail is deleted and cannot be restored.
        #[arg(long)]
        confirm: bool,
    },
}

#[derive(Args)]
pub(super) struct DeclareMailboxArgs {
    /// Skarbiec item that holds the mailbox's credentials.
    #[arg(long)]
    pub(super) skarbiec_item: String,
}

#[derive(Subcommand)]
pub(super) enum MessageCommand {
    /// Imported messages, newest first: every one, or a page with --limit.
    List {
        /// Only this mailbox's messages.
        #[arg(long)]
        mailbox: Option<Uuid>,
        /// Messages per page; without it, every message from --offset on.
        #[arg(long)]
        limit: Option<u32>,
        /// Messages to skip before the page.
        #[arg(long, default_value_t = 0)]
        offset: u32,
    },
    /// One imported message.
    Show {
        /// Message ID, as `skrzynka message list` prints it.
        id: Uuid,
    },
    /// Reply to a message with the body in a file; the result is terminal or
    /// ambiguous, never assumed sent.
    Reply {
        /// Message ID being answered.
        id: Uuid,
        /// File holding the reply body.
        #[arg(long)]
        body_file: PathBuf,
        /// Key that makes a repeated reply the same reply; one is drawn when omitted.
        #[arg(long)]
        idempotency_key: Option<String>,
    },
    /// Send a new message, claimed by its idempotency key before the provider sees it.
    Send {
        /// Mailbox the message is sent from.
        #[arg(long)]
        mailbox: String,
        /// Recipient address; repeatable.
        #[arg(long = "to", required = true)]
        to: Vec<String>,
        /// Copied address; repeatable.
        #[arg(long = "cc")]
        cc: Vec<String>,
        /// Subject line.
        #[arg(long)]
        subject: String,
        /// File holding the message body.
        #[arg(long)]
        body_file: PathBuf,
        /// Key that makes a repeated send the same send; one is drawn when omitted.
        #[arg(long)]
        idempotency_key: Option<String>,
    },
    /// Outbound messages and their delivery state.
    Outbound {
        /// Only messages sent from this mailbox.
        #[arg(long)]
        mailbox: Option<String>,
        /// Messages per page; without it, every message from --offset on.
        #[arg(long)]
        limit: Option<u32>,
        /// Messages to skip before the page.
        #[arg(long, default_value_t = 0)]
        offset: u32,
    },
}
