//! `skrzynka gmail connection`: what every Gmail connection path can do, and
//! what it must never claim.
//!
//! Reception on this installation has never succeeded, and the repair was
//! hunted across two sessions through four commands because each connection
//! path answered separately and the password refusal recommended a fifth that
//! could not work. The report exists so that question is answered once. These
//! cases defend the property that makes it worth trusting: a path is `usable`
//! only after something authenticated, and an absent declaration is reported
//! as the exact item an operator has to store.

use crate::fixture::{assert_exit_one_with, assert_success, MailboxFixture};
use serde_json::Value;

/// A well-formed service-account key that never signs anything here: the
/// consumer-account refusal is taken before any token is minted, which is the
/// property under test.
const SERVICE_ACCOUNT: &str = r#"{"schema":"skarbiec.item.v2","kind":"stado-secret","fields":{"value":{"type":"service_account","value":"{\"type\":\"service_account\",\"project_id\":\"skrzynka-test\",\"client_email\":\"delegated-mail@skrzynka-test.iam.gserviceaccount.invalid\",\"client_id\":\"110000000000000000001\",\"private_key_id\":\"test\",\"private_key\":\"-----BEGIN PRIVATE KEY-----\\ntest\\n-----END PRIVATE KEY-----\\n\"}"}},"context":{}}"#;

fn report(fixture: &MailboxFixture, arguments: &[&str]) -> Value {
    let output = fixture.skrzynka(arguments);
    assert_success("gmail connection report", &output);
    serde_json::from_slice(&output.stdout).expect("the connection report must be JSON")
}

fn path<'a>(report: &'a Value, name: &str) -> &'a Value {
    report["paths"]
        .as_array()
        .expect("the report must list its paths")
        .iter()
        .find(|path| path["path"] == name)
        .unwrap_or_else(|| panic!("the report must carry the {name} path"))
}

#[test]
fn every_connection_path_is_reported_and_none_is_usable_without_its_declaration() {
    let fixture = MailboxFixture::new("readiness-undeclared");
    let report = report(&fixture, &["gmail", "connection"]);

    assert_eq!(
        report["usable_paths"], 0,
        "an empty vault declares no connection path, so none may be reported usable"
    );
    assert_eq!(report["account"], Value::Null);
    assert_eq!(report["mailbox_id"], Value::Null);

    // No account was named, so no credential could be authenticated. The path
    // still says which command connects one, because an app-specific password
    // needs neither an administrator nor an OAuth client.
    let app_password = path(&report, "app_password");
    assert_eq!(app_password["verdict"], "unproven");
    assert_eq!(app_password["code"], "GMAIL_APP_PASSWORD_NOT_STORED");
    assert_eq!(
        app_password["action"],
        "skrzynka gmail app-password --email <address>"
    );

    // Both remaining paths rest on a fixed Skarbiec item. Neither is in this
    // vault, so each is refused naming the item that would carry it.
    for (name, item) in [
        ("oauth", "skrzynka-google-oauth-desktop"),
        ("delegation", "skrzynka-google-service-account"),
    ] {
        let path = path(&report, name);
        assert_eq!(path["verdict"], "refused", "{name} has no declaration");
        assert!(
            path["action"]
                .as_str()
                .expect("every path names its next step")
                .contains(item),
            "the {name} path must name {item}, the item an operator has to store"
        );
    }
}

#[test]
fn a_consumer_account_is_refused_for_delegation_without_asking_google() {
    let fixture = MailboxFixture::new("readiness-consumer");
    let output = fixture.skarbiec_with_stdin(
        &[
            "set-json",
            "skrzynka-google-service-account",
            "--type",
            "stado-secret",
        ],
        SERVICE_ACCOUNT,
    );
    fixture.assert_success("seed the delegated-mail service account", output);

    let report = report(
        &fixture,
        &["gmail", "connection", "--email", "someone@gmail.com"],
    );
    let delegation = path(&report, "delegation");
    assert_eq!(delegation["verdict"], "refused");
    assert_eq!(delegation["code"], "GOOGLE_DELEGATION_NOT_APPLICABLE");
    assert_eq!(
        delegation["detail"],
        "someone@gmail.com is a consumer Google account, and domain-wide delegation exists only \
         inside a Workspace domain. No administrator can grant it for this address."
    );
    // The refusal hands over the path that does work for a consumer account —
    // the Weles command that creates the app password — rather than the
    // console URL no administrator can act on.
    assert_eq!(
        delegation["action"],
        "weles app-password --login-item <Skarbiec Google login of someone@gmail.com>"
    );
    // The service account is still reported, because an operator reading this
    // needs to recognise the key that was checked.
    assert_eq!(
        delegation["observed"]["service_account"],
        "delegated-mail@skrzynka-test.iam.gserviceaccount.invalid"
    );
    assert_eq!(report["usable_paths"], 0);
}

#[test]
fn an_address_that_is_not_an_address_is_refused_before_anything_is_contacted() {
    let fixture = MailboxFixture::new("readiness-malformed");
    let output = fixture.skrzynka(&["gmail", "connection", "--email", "not an address"]);
    assert_exit_one_with(
        &output,
        r#"{"error":{"code":"GMAIL_PROFILE_INVALID","message":"email is not a valid address","retryable":false}}"#,
    );
}
