use rusqlite::Connection;
use serde_json::Value;
use std::fs;

use super::fixture::*;
#[test]
fn gmail_app_password_mailbox_selector_refusals_leave_credentials_and_mailboxes_unchanged() {
    const GMAIL_EMAIL: &str = "selector-refusal@gmail.com";
    const GMAIL_ITEM: &str = "skrzynka-gmail-app-password-0d4931795b42e0dc4226";
    const SHARED_ADDRESS: &str = "team@example.invalid";

    let fixture = MailboxFixture::new("gmail-selector");
    fixture.seed_mailbox_item("alpha-inbox");
    fixture.seed_mailbox_item("beta-inbox");

    let alpha_output = fixture.skrzynka(&[
        "mailbox",
        "add",
        "--skarbiec-item",
        "alpha-inbox",
        "--display-name",
        "Alpha Inbox",
    ]);
    assert_success("add first same-address mailbox", &alpha_output);
    let alpha: Value =
        serde_json::from_slice(&alpha_output.stdout).expect("first mailbox add must return JSON");

    let beta_output = fixture.skrzynka(&[
        "mailbox",
        "add",
        "--skarbiec-item",
        "beta-inbox",
        "--display-name",
        "Beta Inbox",
    ]);
    assert_success("add second same-address mailbox", &beta_output);
    let beta: Value =
        serde_json::from_slice(&beta_output.stdout).expect("second mailbox add must return JSON");

    let baseline = mailbox_receiving_state(&fixture);
    fixture.assert_skarbiec_item_absent(GMAIL_ITEM);

    let missing = fixture.skrzynka_with_stdin(
        &[
            "gmail",
            "app-password",
            "--email",
            GMAIL_EMAIL,
            "--mailbox",
            UNKNOWN_MAILBOX,
        ],
        PASSWORD,
    );
    assert_exit_one_with(
        &missing,
        r#"{"error":{"code":"NOT_FOUND","message":"mailbox was not found","retryable":false}}"#,
    );
    assert_eq!(
        mailbox_receiving_state(&fixture),
        baseline,
        "unknown selector refusal must not modify or add mailbox rows"
    );
    fixture.assert_skarbiec_item_absent(GMAIL_ITEM);

    let ambiguous = fixture.skrzynka_with_stdin(
        &[
            "gmail",
            "app-password",
            "--email",
            GMAIL_EMAIL,
            "--mailbox",
            SHARED_ADDRESS,
        ],
        PASSWORD,
    );
    let ambiguous_error = format!(
        r#"{{"error":{{"code":"MAILBOX_SELECTOR_AMBIGUOUS","message":"{SHARED_ADDRESS} names 2 mailboxes ({}, {}); select one by id","retryable":false}}}}"#,
        MailboxFixture::mailbox_id(&alpha),
        MailboxFixture::mailbox_id(&beta)
    );
    assert_exit_one_with(&ambiguous, &ambiguous_error);
    assert_eq!(
        mailbox_receiving_state(&fixture),
        baseline,
        "ambiguous selector refusal must not modify or add mailbox rows"
    );
    fixture.assert_skarbiec_item_absent(GMAIL_ITEM);
}

#[test]
#[ignore = "requires a real password-backed Skarbiec item and more than 200 INBOX UIDs"]
fn provider_import_does_not_advance_past_unprocessed_mail() {
    use sha2::{Digest, Sha256};
    use std::{path::PathBuf, process::Command};
    let item = std::env::var("SKRZYNKA_TEST_IMAP_ITEM").expect("set SKRZYNKA_TEST_IMAP_ITEM");
    let skarbiec =
        std::env::var("SKRZYNKA_TEST_SKARBIEC_BIN").unwrap_or_else(|_| "skarbiec".into());
    let host = std::env::var("SKRZYNKA_TEST_IMAP_HOST").unwrap_or_else(|_| "imap.gmail.com".into());
    let credential = Command::new(&skarbiec)
        .args(["get", &item])
        .output()
        .expect("read real credential");
    assert!(
        credential.status.success(),
        "real Skarbiec credential unavailable"
    );
    let document: Value =
        serde_json::from_slice(&credential.stdout).expect("canonical Skarbiec item");
    let username = document["fields"]["username"]
        .as_str()
        .expect("canonical username");
    let password = document["fields"]["password"]
        .as_str()
        .expect("canonical password");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target/provider-tests")
        .join(uuid::Uuid::new_v4().to_string());
    fs::create_dir_all(&root).expect("create retained product evidence");
    let database = root.join("mail.db");
    let binary = env!("CARGO_BIN_EXE_skrzynka");
    let args = [
        "--database",
        database.to_str().unwrap(),
        "--skarbiec-bin",
        &skarbiec,
        "mailbox",
        "import",
        "--skarbiec-item",
        &item,
        "--email",
        username,
        "--imap-host",
        &host,
        "--smtp-host",
        "smtp.gmail.com",
    ];
    let output = Command::new(binary)
        .args(args)
        .output()
        .expect("run real mailbox import");
    let mut report = serde_json::json!({
        "source_revision": std::env::var("SKRZYNKA_SOURCE_REVISION").expect("bind source revision"),
        "receiver_sha256": format!("{:x}", Sha256::digest(include_bytes!("../../src/mail/incoming.rs"))),
        "binary_sha256": format!("{:x}", Sha256::digest(fs::read(binary).unwrap())),
        "command": args, "exit_code": output.status.code(),
        "stdout": String::from_utf8_lossy(&output.stdout), "stderr": String::from_utf8_lossy(&output.stderr),
        "passed": false
    });
    let evidence = root.join("report.json");
    fs::write(&evidence, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    eprintln!("provider import evidence: {}", evidence.display());
    assert_success(
        "real provider import (failure remains failed, never skipped)",
        &output,
    );
    let imported: Value = serde_json::from_slice(&output.stdout).expect("import result");
    let client = imap::ClientBuilder::new(host.as_str(), 993)
        .mode(imap::ConnectionMode::Tls)
        .tls_kind(imap::TlsKind::Native)
        .connect()
        .expect("connect to real provider over verified TLS");
    let mut session = client
        .login(username, password)
        .map_err(|_| "real provider refused reference IMAP login")
        .unwrap();
    session.select("INBOX").expect("select real INBOX");
    let mut uids = session
        .uid_search("ALL")
        .expect("read real provider UID set")
        .into_iter()
        .collect::<Vec<_>>();
    session.logout().expect("close read-only provider session");
    uids.sort_unstable();
    report["provider_uid_count"] = serde_json::json!(uids.len());
    fs::write(&evidence, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    assert!(uids.len() > 200, "real INBOX must span more than one page");
    assert_eq!(
        imported["mailbox"]["last_uid"].as_u64(),
        Some(uids[199] as u64)
    );
    assert_eq!(imported["has_more"], true);
    let connection = Connection::open(&database).expect("inspect persisted import");
    let cursor: u32 = connection
        .query_row("SELECT last_uid FROM mailboxes", [], |row| row.get(0))
        .unwrap();
    assert_eq!(
        cursor, uids[199],
        "persisted cursor must not skip unprocessed UIDs"
    );
    let beyond: u32 = connection
        .query_row(
            "SELECT COUNT(*) FROM messages WHERE external_uid > ?",
            [cursor],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(beyond, 0);
    report["persisted_cursor"] = serde_json::json!(cursor);
    report["passed"] = serde_json::json!(true);
    fs::write(&evidence, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    drop(connection);
    fs::remove_file(database).expect("remove isolated mail data; retain metadata report");
}
#[test]
fn mailbox_add_persists_only_the_profile_and_refuses_duplicate_or_invalid_accounts() {
    let fixture = MailboxFixture::new("add");
    fixture.seed_mailbox_item("team-inbox");
    fixture.seed_mailbox_item("invalid-inbox");

    let created = fixture.add_mailbox("team-inbox");
    let id = MailboxFixture::mailbox_id(&created);
    let stored = fixture
        .connection()
        .query_row(
            "SELECT skarbiec_item_id, display_name, email, imap_host, imap_port, smtp_host, smtp_port, smtp_security, poll_interval_seconds, enabled, last_uid FROM mailboxes WHERE id=?1",
            [id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, i64>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, i64>(8)?,
                    row.get::<_, i64>(9)?,
                    row.get::<_, i64>(10)?,
                ))
            },
        )
        .expect("read persisted mailbox state");
    assert_eq!(
        stored,
        (
            "team-inbox".to_string(),
            "Team Inbox".to_string(),
            "team@example.invalid".to_string(),
            "imap.example.invalid".to_string(),
            993,
            "smtp.example.invalid".to_string(),
            587,
            "starttls".to_string(),
            60,
            1,
            0,
        )
    );
    let database_bytes = fs::read(&fixture.database).expect("read SQLite state");
    assert!(
        !database_bytes
            .windows(PASSWORD.len())
            .any(|window| window == PASSWORD.as_bytes()),
        "the mailbox password must never enter SQLite"
    );

    let duplicate = fixture.skrzynka(&["mailbox", "add", "--skarbiec-item", "team-inbox"]);
    assert_exit_one_with(
        &duplicate,
        r#"{"error":{"code":"MAILBOX_ALREADY_EXISTS","message":"a mailbox already uses this Skarbiec item","retryable":false}}"#,
    );

    let invalid = fixture.skrzynka(&[
        "mailbox",
        "add",
        "--skarbiec-item",
        "invalid-inbox",
        "--email",
        "not-an-address",
    ]);
    assert_exit_one_with(
        &invalid,
        r#"{"error":{"code":"MAILBOX_PROFILE_INVALID","message":"email is not a valid address","retryable":false}}"#,
    );
    let count: i64 = fixture
        .connection()
        .query_row("SELECT COUNT(*) FROM mailboxes", [], |row| row.get(0))
        .expect("count persisted mailboxes");
    assert_eq!(count, 1, "refused creates must not write mailbox state");
}
