//! The mailbox suite: real `skrzynka` runs against an isolated Skarbiec and
//! database, one story per case.
//!
//! This was one file of 647 lines until 2026-09-09, and no file in this
//! workshop may exceed three hundred. The seams are meaningful rather than
//! arithmetic: `fixture` builds and tears down the throwaway mailbox and
//! holds the shared assertions. `lifecycle` covers enable, disable, removal and
//! schema migration; `credentials` covers profile creation, Gmail refusals and
//! the explicitly selected live-provider pagination regression; `readiness`
//! covers the report that says which Gmail connection paths an account can
//! actually use.

mod account_sources;
mod credentials;
mod fixture;
mod lifecycle;
mod readiness;
