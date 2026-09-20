//! Connecting Gmail accounts: through a delegated service account or OAuth grant, or with
//! an app-specific password, and which of those paths this installation can actually use.

mod delegated;
mod password;
mod readiness;
