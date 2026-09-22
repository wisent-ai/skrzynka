use serde_json::Value;

use super::fixture::*;

#[test]
fn import_refuses_incomplete_skarbiec_profile_without_creating_local_account() {
    let fixture = MailboxFixture::new("source-profile");
    fixture.seed_mailbox_item("source-inbox");
    let source = fixture.skarbiec(&["get", "source-inbox"]);
    assert_success("read account from real Skarbiec", &source);
    let mut document: Value = serde_json::from_slice(&source.stdout).unwrap();
    document["fields"]
        .as_object_mut()
        .unwrap()
        .remove("imap_host");
    let written = fixture.skarbiec_with_stdin(
        &["set-json", "source-inbox", "--type", "bundle"],
        &serde_json::to_string(&document).unwrap(),
    );
    assert_success("store incomplete source profile", &written);

    let import = fixture.skrzynka(&["mailbox", "import", "--skarbiec-item", "source-inbox"]);
    assert_eq!(import.status.code(), Some(1));
    let refusal: Value = serde_json::from_slice(&import.stderr).unwrap();
    assert_eq!(refusal["error"]["code"], "MAILBOX_PROFILE_INVALID");
    assert!(refusal["error"]["message"]
        .as_str()
        .unwrap()
        .contains("imap_host"));
    let retained: u64 = fixture
        .connection()
        .query_row("SELECT COUNT(*) FROM mailboxes", [], |row| row.get(0))
        .unwrap();
    assert_eq!(retained, 0);

    document["fields"]["imap_host"] = "imap.example.invalid".into();
    let written = fixture.skarbiec_with_stdin(
        &["set-json", "source-inbox", "--type", "bundle"],
        &serde_json::to_string(&document).unwrap(),
    );
    assert_success("complete the profile in Skarbiec", &written);
    let account = fixture.add_mailbox("source-inbox");
    assert_eq!(account["imap_host"], document["fields"]["imap_host"]);
    assert_eq!(account["email"], document["fields"]["email"]);
    assert_eq!(account["skarbiec_item_id"], "source-inbox");
    let retained: String = fixture
        .connection()
        .query_row("SELECT imap_host FROM mailboxes", [], |row| row.get(0))
        .unwrap();
    assert_eq!(retained, "imap.example.invalid");
}

#[test]
fn invalid_source_security_does_not_fall_back_to_a_different_transport() {
    let fixture = MailboxFixture::new("source-security");
    fixture.seed_mailbox_item("source-inbox");
    let source = fixture.skarbiec(&["get", "source-inbox"]);
    assert_success("read account from real Skarbiec", &source);
    let mut document: Value = serde_json::from_slice(&source.stdout).unwrap();
    document["fields"]["smtp_security"] = "plaintext".into();
    let written = fixture.skarbiec_with_stdin(
        &["set-json", "source-inbox", "--type", "bundle"],
        &serde_json::to_string(&document).unwrap(),
    );
    assert_success("store invalid source transport", &written);
    let import = fixture.skrzynka(&["mailbox", "import", "--skarbiec-item", "source-inbox"]);
    assert_eq!(import.status.code(), Some(1));
    let refusal: Value = serde_json::from_slice(&import.stderr).unwrap();
    assert_eq!(refusal["error"]["code"], "MAILBOX_PROFILE_INVALID");
    let retained: u64 = fixture
        .connection()
        .query_row("SELECT COUNT(*) FROM mailboxes", [], |row| row.get(0))
        .unwrap();
    assert_eq!(retained, 0);
}
