use rusqlite::Connection;
use serde_json::Value;
use std::ffi::OsString;
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);
pub(crate) const UNKNOWN_MAILBOX: &str = "00000000-0000-0000-0000-000000000001";
pub(crate) const PASSWORD: &str = "correct-horse-battery-staple";

pub(crate) struct MailboxFixture {
    pub(crate) root: PathBuf,
    pub(crate) gnupg: PathBuf,
    pub(crate) vault: PathBuf,
    pub(crate) audit: PathBuf,
    pub(crate) database: PathBuf,
    pub(crate) skarbiec: OsString,
}

impl MailboxFixture {
    pub(crate) fn new(test_name: &str) -> Self {
        let fixture = Self::without_skarbiec(test_name);
        fixture.assert_success(
            "initialize isolated Skarbiec",
            fixture.skarbiec(&[
                "init",
                "Skrzynka mailbox test <skrzynka-mailbox-test@example.invalid>",
            ]),
        );
        fixture
    }

    pub(crate) fn without_skarbiec(test_name: &str) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock must follow the Unix epoch")
            .as_nanos();
        let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
        // This fixture is the one case that cannot live in the package's own
        // build directory. It runs a real gpg-agent, whose socket path is a
        // `sun_path` and so limited to 104 bytes;
        // `<checkout>/target/tmp/<name>/gnupg/S.gpg-agent` is longer than
        // that, and gpg answers "can't connect to the gpg-agent: File name
        // too long" — measured on 2026-09-09, five of six cases failing.
        // So the root is short and belongs to this product, next to the
        // `~/.skrzynka` state its own commands use: never `/tmp`, which this
        // machine sweeps on sight, and never `~/.stado/work`, which belongs
        // to Stado. `Drop` removes the whole root, so nothing accumulates.
        // The case name is carried into the directory so a leftover root says
        // which test made it, bounded to twelve bytes because the gpg socket
        // path above is the hard limit.
        let label: String = test_name
            .chars()
            .filter(|character| character.is_ascii_alphanumeric() || *character == '-')
            .take(12)
            .collect();
        let root = PathBuf::from(env!("HOME"))
            .join(".skrzynka")
            .join("test-runs")
            .join(format!(
                "{label}-{:x}{:08x}{sequence:x}",
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
        fixture
    }

    pub(crate) fn seed_mailbox_item(&self, item_id: &str) {
        let document = format!(
            r#"{{"schema":"skarbiec.item.v2","kind":"bundle","fields":{{"username":"team@example.invalid","password":"{PASSWORD}","display_name":"Team Inbox","email":"team@example.invalid","imap_host":"imap.example.invalid","imap_port":"993","smtp_host":"smtp.example.invalid","smtp_port":"587","smtp_security":"starttls"}},"context":{{}}}}"#
        );
        let output =
            self.skarbiec_with_stdin(&["set-json", item_id, "--type", "bundle"], &document);
        self.assert_success("seed real Skarbiec mailbox bundle", output);
    }

    pub(crate) fn add_mailbox(&self, item_id: &str) -> Value {
        let output = self.skrzynka(&["mailbox", "add", "--skarbiec-item", item_id]);
        assert_success("add mailbox fixture", &output);
        serde_json::from_slice(&output.stdout).expect("mailbox add must return JSON")
    }

    pub(crate) fn skrzynka(&self, args: &[&str]) -> Output {
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

    pub(crate) fn skrzynka_with_stdin(&self, args: &[&str], input: &str) -> Output {
        let mut command = Command::new(env!("CARGO_BIN_EXE_skrzynka"));
        command
            .arg("--database")
            .arg(&self.database)
            .arg("--skarbiec-bin")
            .arg(&self.skarbiec)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        self.isolated_environment(&mut command);
        let mut child = command.spawn().expect("start real Skrzynka binary");
        child
            .stdin
            .take()
            .expect("open Skrzynka stdin")
            .write_all(input.as_bytes())
            .expect("write Gmail app password");
        child.wait_with_output().expect("collect Skrzynka output")
    }

    pub(crate) fn skarbiec(&self, args: &[&str]) -> Output {
        let mut command = Command::new(&self.skarbiec);
        command.args(args);
        self.isolated_environment(&mut command);
        command.output().expect("run real Skarbiec binary")
    }

    pub(crate) fn skarbiec_with_stdin(&self, args: &[&str], input: &str) -> Output {
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

    pub(crate) fn isolated_environment(&self, command: &mut Command) {
        command
            .env("HOME", &self.root)
            .env("GNUPGHOME", &self.gnupg)
            .env("SKARBIEC_VAULT_FILE", &self.vault)
            .env("SKARBIEC_AUDIT_FILE", &self.audit);
    }

    pub(crate) fn connection(&self) -> Connection {
        Connection::open(&self.database).expect("open isolated Skrzynka database")
    }

    pub(crate) fn mailbox_id(mailbox: &Value) -> &str {
        mailbox["id"].as_str().expect("mailbox id must be text")
    }

    pub(crate) fn assert_skarbiec_item_absent(&self, item_id: &str) {
        let output = self.skarbiec(&["get", item_id]);
        assert!(
            !output.status.success(),
            "Skarbiec item '{item_id}' must not exist\nstdout: {}\nstderr: {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    pub(crate) fn assert_success(&self, context: &str, output: Output) {
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

pub(crate) fn assert_success(context: &str, output: &Output) {
    assert!(
        output.status.success(),
        "{context} failed\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

pub(crate) fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).trim().to_owned()
}

pub(crate) fn assert_exit_one_with(output: &Output, expected: &str) {
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(stderr(output), expected);
}

pub(crate) fn mailbox_receiving_state(
    fixture: &MailboxFixture,
) -> Vec<(String, String, Option<String>, String, i64)> {
    let connection = fixture.connection();
    let mut statement = connection
        .prepare(
            "SELECT id, skarbiec_item_id, smtp_skarbiec_item_id, imap_host, enabled
             FROM mailboxes ORDER BY id",
        )
        .expect("prepare mailbox receiving-state query");
    statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, i64>(4)?,
            ))
        })
        .expect("read mailbox receiving state")
        .collect::<Result<Vec<_>, _>>()
        .expect("collect mailbox receiving state")
}
