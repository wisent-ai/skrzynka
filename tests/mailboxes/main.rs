//! The mailbox suite: real `skrzynka` runs against an isolated Skarbiec and
//! database, one story per case.
//!
//! This was one file of 647 lines until 2026-09-09, and no file in this
//! workshop may exceed three hundred. The seams are meaningful rather than
//! arithmetic: `fixture` builds and tears down the throwaway mailbox and
//! holds the shared assertions, `lifecycle` covers adding, disabling,
//! enabling and removing a mailbox, and `credentials` covers the Gmail
//! app-password refusals and the schema-three migration.

mod credentials;
mod fixture;
mod lifecycle;
