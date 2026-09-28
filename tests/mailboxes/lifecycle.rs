use serde_json::Value;
use std::process::{Command, Stdio};

use super::fixture::*;

#[test]
fn a_mailbox_exists_while_its_skarbiec_item_carries_the_tag() {
    let fixture = MailboxFixture::new("declared-by-tag");
    fixture.seed_mailbox_item("team-inbox");
    let mailbox = fixture.declare_in_vault("team-inbox");
    assert_eq!(mailbox["enabled"], true);
    assert_eq!(mailbox["email"], "team@example.invalid");
    let id = MailboxFixture::mailbox_id(&mailbox).to_owned();

    fixture.set_vault_tags("team-inbox", "");
    let undeclared = fixture.listed_mailbox("team-inbox");
    assert_eq!(undeclared["id"], id.as_str());
    assert_eq!(undeclared["enabled"], false);
    assert_eq!(undeclared["last_error_code"], "MAILBOX_NOT_DECLARED");
    assert_eq!(
        undeclared["last_error_message"],
        "Skarbiec item 'team-inbox' does not carry skrzynka:mailbox; Skrzynka keeps its mail and no longer polls it"
    );

    fixture.set_vault_tags("team-inbox", "skrzynka:mailbox");
    let redeclared = fixture.listed_mailbox("team-inbox");
    assert_eq!(redeclared["id"], id.as_str());
    assert_eq!(redeclared["enabled"], true);
}

#[test]
fn undeclare_removes_only_the_mailbox_tag_and_keeps_the_mailbox() {
    let fixture = MailboxFixture::new("undeclare");
    fixture.seed_mailbox_item("team-inbox");
    fixture.set_vault_tags("team-inbox", "team,skrzynka:mailbox");
    let mailbox = fixture.listed_mailbox("team-inbox");
    let id = MailboxFixture::mailbox_id(&mailbox);

    let undeclared = fixture.skrzynka(&["mailbox", "undeclare", id]);
    assert_success("undeclare mailbox", &undeclared);
    assert_eq!(fixture.vault_tags("team-inbox"), vec!["team".to_owned()]);
    assert_eq!(fixture.shown_mailbox(id)["enabled"], false);

    let unknown = fixture.skrzynka(&["mailbox", "undeclare", UNKNOWN_MAILBOX]);
    assert_exit_one_with(
        &unknown,
        r#"{"error":{"code":"NOT_FOUND","message":"mailbox was not found","retryable":false}}"#,
    );
}

#[test]
fn a_mailbox_cannot_be_added_beside_skarbiec() {
    let fixture = MailboxFixture::new("no-local-add");
    let refused = fixture.skrzynka(&["mailbox", "add", "--skarbiec-item", "team-inbox"]);
    assert_eq!(refused.status.code(), Some(2));
}

#[test]
fn mailbox_remove_requires_undeclare_and_confirmation_and_preserves_skarbiec() {
    let fixture = MailboxFixture::new("remove");
    fixture.seed_mailbox_item("team-inbox");
    let mailbox = fixture.declare_in_vault("team-inbox");
    let id = MailboxFixture::mailbox_id(&mailbox);

    let declared = fixture.skrzynka(&["mailbox", "remove", id, "--confirm"]);
    assert_exit_one_with(
        &declared,
        r#"{"error":{"code":"MAILBOX_STILL_DECLARED","message":"Skarbiec item 'team-inbox' still carries skrzynka:mailbox; undeclare the mailbox before removing its local mail","retryable":false}}"#,
    );
    fixture.assert_success(
        "undeclare before removal",
        fixture.skrzynka(&["mailbox", "undeclare", id]),
    );

    let unconfirmed = fixture.skrzynka(&["mailbox", "remove", id]);
    assert_exit_one_with(
        &unconfirmed,
        r#"{"error":{"code":"CONFIRMATION_REQUIRED","message":"mailbox removal requires --confirm","retryable":false}}"#,
    );
    assert_eq!(
        fixture.shown_mailbox(id)["id"],
        id,
        "a refused removal keeps the mailbox"
    );

    let removed = fixture.skrzynka(&["mailbox", "remove", id, "--confirm"]);
    fixture.assert_success("remove confirmed mailbox", removed);
    assert!(
        fixture.listed_mailboxes().is_empty(),
        "the removed mailbox must be gone from the organization"
    );

    let credential = fixture.skarbiec(&["get", "team-inbox", "--field", "password"]);
    assert_success("read preserved Skarbiec item", &credential);
    assert_eq!(
        String::from_utf8_lossy(&credential.stdout),
        format!("{PASSWORD}\n")
    );

    let missing = fixture.skrzynka(&["mailbox", "remove", id, "--confirm"]);
    assert_exit_one_with(
        &missing,
        r#"{"error":{"code":"NOT_FOUND","message":"mailbox was not found","retryable":false}}"#,
    );
}

#[test]
fn without_a_home_directory_nothing_is_guessed() {
    let mut status = Command::new(env!("CARGO_BIN_EXE_skrzynka"));
    status
        .arg("status")
        .env_remove("HOME")
        .env_remove("SKRZYNKA_FLEET_HOME")
        .env_remove("XDG_STATE_HOME");
    let output = status.output().expect("run real Skrzynka binary");
    assert!(!output.status.success());
    let refusal: Value =
        serde_json::from_slice(&output.stderr).expect("the refusal is a JSON error");
    assert_eq!(refusal["error"]["code"], "DATABASE_UNREACHABLE");
    assert!(
        refusal["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("neither SKRZYNKA_FLEET_HOME nor HOME is set")),
        "{refusal}"
    );

    let mut onboarding = Command::new(env!("CARGO_BIN_EXE_skrzynka"));
    onboarding
        .arg("onboarding")
        .env_remove("HOME")
        .env_remove("XDG_STATE_HOME")
        .stdin(Stdio::null());
    let output = onboarding.output().expect("run real Skrzynka binary");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("neither XDG_STATE_HOME nor HOME is set"),
        "{stderr}"
    );
}
