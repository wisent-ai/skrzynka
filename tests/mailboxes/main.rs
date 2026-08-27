use rusqlite::{params, Connection};
use serde_json::Value;
use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);
const UNKNOWN_MAILBOX: &str = "00000000-0000-0000-0000-000000000001";
const PASSWORD: &str = "correct-horse-battery-staple";

struct MailboxFixture {
    root: PathBuf,
    gnupg: PathBuf,
    vault: PathBuf,
    audit: PathBuf,
    database: PathBuf,
    skarbiec: OsString,
}

impl MailboxFixture {
    fn new(test_name: &str) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock must follow the Unix epoch")
            .as_nanos();
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        let root = PathBuf::from(env!("HOME"))
            .join(".stado")
            .join("work")
            .join(format!(
                "skrzynka-{test_name}-{:x}{:08x}{sequence:x}",
                std::process::id(),
                unique & 0xffff_ffff
            ));
        let gnupg = root.join("gnupg");
        fs::create_dir_all(&gnupg).expect("create isolated GPG home");
        fs::set_permissions(&gnupg, fs::Permissions::from_mode(0o700))
            .expect("protect isolated GPG home");

        let fixture = Self {
            vault: root.join("vault.json"),
            audit: root.join("audit.jsonl"),
            database: root.join("skrzynka.db"),
            skarbiec: std::env::var_os("SKRZYNKA_TEST_SKARBIEC_BIN")
                .unwrap_or_else(|| OsString::from("skarbiec")),
            root,
            gnupg,
        };
        fixture.assert_success(
            "initialize isolated Skarbiec",
            fixture.skarbiec(&[
                "init",
                "Skrzynka mailbox test <skrzynka-mailbox-test@example.invalid>",
            ]),
        );
        fixture
    }

    fn seed_mailbox_item(&self, item_id: &str) {
        let document = format!(
            r#"{{"schema":"skarbiec.item.v2","kind":"bundle","fields":{{"username":"team@example.invalid","password":"{PASSWORD}","display_name":"Team Inbox","email":"team@example.invalid","imap_host":"imap.example.invalid","imap_port":"993","smtp_host":"smtp.example.invalid","smtp_port":"587","smtp_security":"starttls"}},"context":{{}}}}"#
        );
        let output = self.skarbiec_with_stdin(
            &["set-json", item_id, "--type", "bundle"],
            &document,
        );
        self.assert_success("seed real Skarbiec mailbox bundle", output);
    }

    fn add_mailbox(&self, item_id: &str) -> Value {
        let output = self.skrzynka(&["mailbox", "add", "--skarbiec-item", item_id]);
        assert_success("add mailbox fixture", &output);
        serde_json::from_slice(&output.stdout).expect("mailbox add must return JSON")
    }

    fn skrzynka(&self, args: &[&str]) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_skrzynka"));
        command
            .arg("--database")
            .arg(&self.database)
            .arg("--skarbiec-bin")
            .arg(&self.skarbiec)
            .args(args);
        self.isolated_environment(&mut command);
        command.output().expect("run real Skrzynka binary")
    }

    fn skarbiec(&self, args: &[&str]) -> Output {
        let mut command = Command::new(&self.skarbiec);
        command.args(args);
        self.isolated_environment(&mut command);
        command.output().expect("run real Skarbiec binary")
    }

    fn skarbiec_with_stdin(&self, args: &[&str], input: &str) -> Output {
        let mut command = Command::new(&self.skarbiec);
        command
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        self.isolated_environment(&mut command);
        let mut child = command.spawn().expect("start real Skarbiec binary");
        child
            .stdin
            .take()
            .expect("open Skarbiec stdin")
            .write_all(input.as_bytes())
            .expect("write Skarbiec item JSON");
        child.wait_with_output().expect("collect Skarbiec output")
    }

    fn isolated_environment(&self, command: &mut Command) {
        command
            .env("HOME", &self.root)
            .env("GNUPGHOME", &self.gnupg)
            .env("SKARBIEC_VAULT_FILE", &self.vault)
            .env("SKARBIEC_AUDIT_FILE", &self.audit);
    }

    fn connection(&self) -> Connection {
        Connection::open(&self.database).expect("open isolated Skrzynka database")
    }

    fn mailbox_id(mailbox: &Value) -> &str {
        mailbox["id"].as_str().expect("mailbox id must be text")
    }

    fn assert_success(&self, context: &str, output: Output) {
        assert!(
            output.status.success(),
            "{context} failed\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

impl Drop for MailboxFixture {
    fn drop(&mut self) {
        let _ = Command::new("gpgconf")
            .env("GNUPGHOME", &self.gnupg)
            .args(["--kill", "all"])
            .status();
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn assert_success(context: &str, output: &Output) {
    assert!(
        output.status.success(),
        "{context} failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).trim().to_owned()
}

fn assert_exit_one_with(output: &Output, expected: &str) {
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(stderr(output), expected);
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

    let duplicate = fixture.skrzynka(&[
        "mailbox",
        "add",
        "--skarbiec-item",
        "team-inbox",
    ]);
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

#[test]
fn mailbox_disable_changes_only_enabled_state_and_refuses_unknown_accounts() {
    let fixture = MailboxFixture::new("disable");
    fixture.seed_mailbox_item("team-inbox");
    let mailbox = fixture.add_mailbox("team-inbox");
    let id = MailboxFixture::mailbox_id(&mailbox);

    let disabled = fixture.skrzynka(&["mailbox", "disable", id]);
    fixture.assert_success("disable mailbox", disabled);
    let state = fixture
        .connection()
        .query_row(
            "SELECT enabled, skarbiec_item_id, email, last_uid FROM mailboxes WHERE id=?1",
            [id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            },
        )
        .expect("read disabled mailbox state");
    assert_eq!(
        state,
        (0, "team-inbox".into(), "team@example.invalid".into(), 0)
    );

    let unknown = fixture.skrzynka(&["mailbox", "disable", UNKNOWN_MAILBOX]);
    assert_exit_one_with(
        &unknown,
        r#"{"error":{"code":"NOT_FOUND","message":"mailbox was not found","retryable":false}}"#,
    );
}

#[test]
fn mailbox_enable_changes_only_enabled_state_and_refuses_unknown_accounts() {
    let fixture = MailboxFixture::new("enable");
    fixture.seed_mailbox_item("team-inbox");
    let mailbox = fixture.add_mailbox("team-inbox");
    let id = MailboxFixture::mailbox_id(&mailbox);
    fixture.assert_success(
        "prepare disabled mailbox",
        fixture.skrzynka(&["mailbox", "disable", id]),
    );

    let enabled = fixture.skrzynka(&["mailbox", "enable", id]);
    fixture.assert_success("enable mailbox", enabled);
    let state = fixture
        .connection()
        .query_row(
            "SELECT enabled, skarbiec_item_id, email, last_uid FROM mailboxes WHERE id=?1",
            [id],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            },
        )
        .expect("read enabled mailbox state");
    assert_eq!(
        state,
        (1, "team-inbox".into(), "team@example.invalid".into(), 0)
    );

    let unknown = fixture.skrzynka(&["mailbox", "enable", UNKNOWN_MAILBOX]);
    assert_exit_one_with(
        &unknown,
        r#"{"error":{"code":"NOT_FOUND","message":"mailbox was not found","retryable":false}}"#,
    );
}

#[test]
fn mailbox_remove_requires_confirmation_deletes_local_state_and_preserves_skarbiec() {
    let fixture = MailboxFixture::new("remove");
    fixture.seed_mailbox_item("team-inbox");
    let mailbox = fixture.add_mailbox("team-inbox");
    let id = MailboxFixture::mailbox_id(&mailbox);

    let unconfirmed = fixture.skrzynka(&["mailbox", "remove", id]);
    assert_exit_one_with(
        &unconfirmed,
        r#"{"error":{"code":"CONFIRMATION_REQUIRED","message":"mailbox removal requires --confirm","retryable":false}}"#,
    );
    let still_present: i64 = fixture
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM mailboxes WHERE id=?1",
            params![id],
            |row| row.get(0),
        )
        .expect("check mailbox after refused removal");
    assert_eq!(still_present, 1);

    let removed = fixture.skrzynka(&["mailbox", "remove", id, "--confirm"]);
    fixture.assert_success("remove confirmed mailbox", removed);
    let remaining: i64 = fixture
        .connection()
        .query_row("SELECT COUNT(*) FROM mailboxes", [], |row| row.get(0))
        .expect("count mailboxes after removal");
    assert_eq!(remaining, 0);

    let credential = fixture.skarbiec(&["get", "team-inbox", "--field", "password"]);
    assert_success("read preserved Skarbiec item", &credential);
    assert_eq!(String::from_utf8_lossy(&credential.stdout), format!("{PASSWORD}\n"));

    let missing = fixture.skrzynka(&["mailbox", "remove", id, "--confirm"]);
    assert_exit_one_with(
        &missing,
        r#"{"error":{"code":"NOT_FOUND","message":"mailbox was not found","retryable":false}}"#,
    );
}
