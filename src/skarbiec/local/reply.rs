//! What Skarbiec answered, or what the credentials file standing in for it
//! answered: one shape, so a reader of the answer never asks which of the
//! two gave it.

use std::fmt;
use std::process::{ExitStatus, Output};

/// How an answer ended.
pub enum Status {
    /// The Skarbiec process exited with this status.
    Exited(ExitStatus),
    /// The credentials file did what was asked.
    Served,
    /// The credentials file refused; the reason is on `stderr`.
    Refused,
}

impl Status {
    /// Whether what was asked was done.
    pub fn success(&self) -> bool {
        match self {
            Self::Exited(status) => status.success(),
            Self::Served => true,
            Self::Refused => false,
        }
    }
}

impl fmt::Display for Status {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Exited(status) => status.fmt(formatter),
            Self::Served => formatter.write_str("served by the credentials file"),
            Self::Refused => formatter.write_str("refused by the credentials file"),
        }
    }
}

/// One answer: how it ended, what was printed and why it refused.
pub struct Reply {
    pub status: Status,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

impl From<Output> for Reply {
    fn from(output: Output) -> Self {
        Self {
            status: Status::Exited(output.status),
            stdout: output.stdout,
            stderr: output.stderr,
        }
    }
}
