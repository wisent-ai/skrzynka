//! Connecting Gmail accounts: through a delegated service account or OAuth grant, or with
//! an app-specific password, which of those paths this installation can actually use,
//! and disconnecting an account again.

mod delegated;
mod disconnect;
mod password;
mod readiness;
