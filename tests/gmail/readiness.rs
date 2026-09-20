// Which account a Gmail connection path can belong to, attached to
// src/service/gmail/readiness/mod.rs. The paths themselves are driven through
// the real binary in tests/mailboxes/readiness.rs; these two decisions are
// taken before anything is contacted, so they are settled here.

use super::*;

#[test]
fn a_consumer_google_account_can_never_use_workspace_delegation() {
    // Domain-wide delegation is granted inside a Workspace domain, so these
    // addresses are refused without asking Google at all.
    assert!(is_consumer_account("lukasz.bartoszcze@gmail.com"));
    assert!(is_consumer_account("someone@googlemail.com"));
    assert!(is_consumer_account("Someone@GMAIL.COM"));
    // A Workspace address is the case a real token mint has to decide.
    assert!(!is_consumer_account("lukasz@wisent.com"));
    assert!(!is_consumer_account("user@notgmail.com"));
    // Not an address at all: no domain, so no consumer domain either.
    assert!(!is_consumer_account("gmail.com"));
}

#[test]
fn the_reported_account_is_an_address_or_a_refusal() {
    assert_eq!(
        validated_account("  lukasz@wisent.com  ").expect("address"),
        "lukasz@wisent.com"
    );
    let error = validated_account("not an address").expect_err("refusal");
    assert_eq!(error.code, "GMAIL_PROFILE_INVALID");
    assert_eq!(error.message, "email is not a valid address");
}
