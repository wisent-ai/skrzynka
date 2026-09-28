use serde_json::Value;
use std::fs;

use super::fixture::*;
#[test]
#[ignore = "requires a real password-backed Skarbiec item and more than 200 INBOX UIDs"]
fn provider_import_does_not_advance_past_unprocessed_mail() {
    use sha2::{Digest, Sha256};
    use std::{path::PathBuf, process::Command};
    let item = std::env::var("SKRZYNKA_TEST_IMAP_ITEM").expect("set SKRZYNKA_TEST_IMAP_ITEM");
    let skarbiec =
        std::env::var("SKRZYNKA_TEST_SKARBIEC_BIN").unwrap_or_else(|_| "skarbiec".into());
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
    let host = document["fields"]["imap_host"]
        .as_str()
        .expect("Skarbiec IMAP host");
    let port = document["fields"]
        .get("imap_port")
        .map(|value| {
            value
                .as_u64()
                .and_then(|port| u16::try_from(port).ok())
                .or_else(|| value.as_str().and_then(|port| port.parse::<u16>().ok()))
                .expect("Skarbiec IMAP port")
        })
        .unwrap_or(993);
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target/provider-tests")
        .join(uuid::Uuid::new_v4().to_string());
    fs::create_dir_all(&root).expect("create retained product evidence");
    let organization = format!("skrzynka-provider-{}", uuid::Uuid::new_v4());
    let binary = env!("CARGO_BIN_EXE_skrzynka");
    let skrzynka = |arguments: &[&str]| {
        Command::new(binary)
            .args(["--organization", &organization, "--skarbiec-bin", &skarbiec])
            .args(arguments)
            .output()
            .expect("run real Skrzynka binary")
    };
    let args = ["mailbox", "declare", "--skarbiec-item", item.as_str()];
    let output = skrzynka(&args);
    let mut report = serde_json::json!({
        "source_revision": std::env::var("SKRZYNKA_SOURCE_REVISION").expect("bind source revision"),
        "receiver_sha256": format!("{:x}", Sha256::digest(include_bytes!("../../src/mail/incoming.rs"))),
        "binary_sha256": format!("{:x}", Sha256::digest(fs::read(binary).unwrap())),
        "organization": organization,
        "command": args, "exit_code": output.status.code(),
        "stdout": String::from_utf8_lossy(&output.stdout), "stderr": String::from_utf8_lossy(&output.stderr),
        "passed": false
    });
    let status: Value = serde_json::from_slice(&skrzynka(&["status"]).stdout)
        .expect("inspect real import final state");
    report["persisted_mailboxes"] = status["mailbox_count"].clone();
    report["persisted_messages"] = status["message_count"].clone();
    let evidence = root.join("report.json");
    fs::write(&evidence, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    eprintln!("provider import evidence: {}", evidence.display());
    assert_success(
        "real provider import (failure remains failed, never skipped)",
        &output,
    );
    let imported: Value = serde_json::from_slice(&output.stdout).expect("import result");
    let client = imap::ClientBuilder::new(host, port)
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
    let mailbox_id = imported["mailbox"]["id"].as_str().expect("imported mailbox id");
    let shown: Value = serde_json::from_slice(&skrzynka(&["mailbox", "show", mailbox_id]).stdout)
        .expect("inspect persisted import");
    let cursor = shown["last_uid"].as_u64().expect("persisted cursor");
    assert_eq!(
        cursor,
        u64::from(uids[199]),
        "persisted cursor must not skip unprocessed UIDs"
    );
    let mut beyond = 0usize;
    let mut offset = 0usize;
    loop {
        let page: Vec<Value> = serde_json::from_slice(
            &skrzynka(&["message", "list", "--mailbox", mailbox_id, "--offset", &offset.to_string()]).stdout,
        )
        .expect("read persisted messages");
        beyond += page
            .iter()
            .filter(|message| message["external_uid"].as_u64() > Some(cursor))
            .count();
        if page.is_empty() {
            break;
        }
        offset += page.len();
    }
    assert_eq!(beyond, 0);
    report["persisted_cursor"] = serde_json::json!(cursor);
    report["passed"] = serde_json::json!(true);
    fs::write(&evidence, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    let _ = skrzynka(&["mailbox", "undeclare", mailbox_id]);
    let removed = skrzynka(&["mailbox", "remove", mailbox_id, "--confirm"]);
    assert_success("remove the journey's mail from the fleet database; retain metadata report", &removed);
}
#[test]
fn a_declared_mailbox_persists_only_the_profile_and_refuses_profile_overrides() {
    let fixture = MailboxFixture::new("declare-profile");
    fixture.seed_mailbox_item("team-inbox");

    let created = fixture.declare_in_vault("team-inbox");
    let id = MailboxFixture::mailbox_id(&created);
    let stored = fixture.shown_mailbox(id);
    for (field, expected) in [
        ("skarbiec_item_id", Value::from("team-inbox")),
        ("display_name", Value::from("Team Inbox")),
        ("email", Value::from("team@example.invalid")),
        ("imap_host", Value::from("imap.example.invalid")),
        ("imap_port", Value::from(993)),
        ("smtp_host", Value::from("smtp.example.invalid")),
        ("smtp_port", Value::from(587)),
        ("smtp_security", Value::from("starttls")),
        ("poll_interval_seconds", Value::from(60)),
        ("enabled", Value::from(true)),
        ("last_uid", Value::from(0)),
    ] {
        assert_eq!(stored[field], expected, "stored {field}");
    }
    assert!(
        !stored.to_string().contains(PASSWORD),
        "the mailbox password must never enter the fleet database"
    );

    let relisted = fixture.listed_mailbox("team-inbox");
    assert_eq!(
        relisted["id"], id,
        "listing again must not add a second mailbox"
    );

    let override_attempt = fixture.skrzynka(&[
        "mailbox",
        "declare",
        "--skarbiec-item",
        "team-inbox",
        "--email",
        "other@example.invalid",
    ]);
    assert_eq!(override_attempt.status.code(), Some(2));
    assert_eq!(
        fixture.mailbox_count(),
        1,
        "refused creates must not write mailbox state"
    );
}
