//! What an operator asks Skrzynka to change about a stored message at its
//! provider, and the record each such request leaves.

use crate::models::ParseModelError;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// One change to the provider's copy of an INBOX message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderAction {
    /// Set the \Seen flag.
    Seen,
    /// Clear the \Seen flag.
    Unseen,
    /// Set the \Flagged flag (RFC 3501), which Gmail shows as starred.
    Flagged,
    /// Clear the \Flagged flag.
    Unflagged,
    /// Move the message out of INBOX into another provider folder.
    Move { folder: String },
    /// Mark the message deleted and expunge it at the provider.
    Delete,
}

impl ProviderAction {
    /// The name the action is recorded and listed under.
    pub fn kind(&self) -> ActionKind {
        match self {
            Self::Seen => ActionKind::Seen,
            Self::Unseen => ActionKind::Unseen,
            Self::Flagged => ActionKind::Flagged,
            Self::Unflagged => ActionKind::Unflagged,
            Self::Move { .. } => ActionKind::Move,
            Self::Delete => ActionKind::Delete,
        }
    }

    /// The folder a move names; no other action names one.
    pub fn folder(&self) -> Option<&str> {
        match self {
            Self::Move { folder } => Some(folder.as_str()),
            Self::Seen | Self::Unseen | Self::Flagged | Self::Unflagged | Self::Delete => None,
        }
    }

    /// What the action does, as a refusal sentence names it.
    pub fn verb(&self) -> &'static str {
        match self {
            Self::Seen => "mark seen",
            Self::Unseen => "mark unseen",
            Self::Flagged => "mark flagged",
            Self::Unflagged => "mark unflagged",
            Self::Move { .. } => "move",
            Self::Delete => "delete",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActionKind {
    Seen,
    Unseen,
    Flagged,
    Unflagged,
    Move,
    Delete,
}

impl ActionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Seen => "seen",
            Self::Unseen => "unseen",
            Self::Flagged => "flagged",
            Self::Unflagged => "unflagged",
            Self::Move => "move",
            Self::Delete => "delete",
        }
    }
}

impl std::str::FromStr for ActionKind {
    type Err = ParseModelError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "seen" => Ok(Self::Seen),
            "unseen" => Ok(Self::Unseen),
            "flagged" => Ok(Self::Flagged),
            "unflagged" => Ok(Self::Unflagged),
            "move" => Ok(Self::Move),
            "delete" => Ok(Self::Delete),
            _ => Err(ParseModelError(format!("unknown message action: {value}"))),
        }
    }
}

/// Where a requested action stands: claimed before the provider is asked,
/// then done once its effect was observed, failed when the provider refused
/// it or it could not start, or uncertain when the connection was lost after
/// the command was sent.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActionStatus {
    Pending,
    Done,
    Failed,
    Uncertain,
}

impl ActionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Uncertain => "uncertain",
        }
    }
}

impl std::str::FromStr for ActionStatus {
    type Err = ParseModelError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "pending" => Ok(Self::Pending),
            "done" => Ok(Self::Done),
            "failed" => Ok(Self::Failed),
            "uncertain" => Ok(Self::Uncertain),
            _ => Err(ParseModelError(format!(
                "unknown message action status: {value}"
            ))),
        }
    }
}

/// The record of one requested action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageAction {
    pub id: Uuid,
    pub message_id: Uuid,
    pub action: ActionKind,
    pub folder: Option<String>,
    pub status: ActionStatus,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub requested_at: String,
    pub completed_at: Option<String>,
}

/// `POST /v1/messages/:id/mark`: `{"as": "seen"}`, `"unseen"`, `"flagged"` or
/// `"unflagged"`.
#[derive(Debug, Clone, Deserialize)]
pub struct MarkMessageRequest {
    #[serde(rename = "as")]
    pub mark_as: MarkState,
}

/// A flag `message mark --as` sets or clears at the provider.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum MarkState {
    Seen,
    Unseen,
    Flagged,
    Unflagged,
}

impl MarkState {
    pub fn action(self) -> ProviderAction {
        match self {
            Self::Seen => ProviderAction::Seen,
            Self::Unseen => ProviderAction::Unseen,
            Self::Flagged => ProviderAction::Flagged,
            Self::Unflagged => ProviderAction::Unflagged,
        }
    }
}

/// `POST /v1/messages/:id/move`: the provider folder to move the message to,
/// by its name, or by what the provider says the folder is for.
#[derive(Debug, Clone, Deserialize)]
pub struct MoveMessageRequest {
    #[serde(default)]
    pub folder: Option<String>,
    #[serde(default)]
    pub to: Option<SpecialUse>,
}

/// What a folder is for, as the provider declares it (RFC 6154): where a
/// move goes without naming the provider's own folder.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum SpecialUse {
    /// Archived mail.
    Archive,
    /// Deleted mail awaiting removal.
    Trash,
    /// Mail judged unwanted.
    Junk,
    /// Every message, as Gmail's All Mail holds it.
    All,
}

impl SpecialUse {
    /// The name `mailbox folders` prints in `special_use`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Archive => "archive",
            Self::Trash => "trash",
            Self::Junk => "junk",
            Self::All => "all",
        }
    }
}
