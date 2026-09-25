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
        // Keep the GPG socket below Darwin's 104-byte Unix socket limit while
        // retaining all isolated test state inside the checkout's build tree.
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target/t")
            .join(format!(
                "{:x}{:08x}{sequence:x}",
                std::process::id(),
                unique & 0xffff_ffff
            ));
        let gnupg = root.join("g");
        fs::create_dir_all(&gnupg).expect("create isolated GPG home");
        fs::set_permissions(&gnupg, fs::Permissions::from_mode(0o700))
            .expect("protect isolated GPG home");
        fs::write(root.join("test-name"), test_name).expect("record isolated test identity");

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
            r#"{{"schema":"skarbiec.item.v2","kind":"bundle","fields":{{"username":"team@example.invalid","password":"{PASSWORD}","display_name":"Team Inbox","email":"team@example.invalid","imap_host":"imap.example.invalid","imap_port":"993","smtp_host":"smtp.example.invalid","smtp_port":"587","smtp_security":"starttls"}},"context":{{"test_item":"{item_id}"}}}}"#
        );
        let output =
            self.skarbiec_with_stdin(&["set-json", item_id, "--type", "bundle"], &document);
        self.assert_success("seed real Skarbiec mailbox bundle", output);
    }

    /// Tag the item `skrzynka:mailbox` in the isolated vault the way an
    /// operator would, then let `mailbox list` reconcile it into a mailbox.
    /// No IMAP server is contacted.
    pub(crate) fn declare_in_vault(&self, item_id: &str) -> Value {
        self.set_vault_tags(item_id, "skrzynka:mailbox");
        self.listed_mailbox(item_id)
    }

    pub(crate) fn set_vault_tags(&self, item_id: &str, tags: &str) {
        let output = self.skarbiec(&["retag", item_id, "--tags", tags]);
        assert_success("retag the Skarbiec item", &output);
    }

    /// The mailbox `mailbox list` reports for one Skarbiec item.
    pub(crate) fn listed_mailbox(&self, item_id: &str) -> Value {
        let output = self.skrzynka(&["mailbox", "list"]);
        assert_success("list declared mailboxes", &output);
        let mailboxes: Vec<Value> =
            serde_json::from_slice(&output.stdout).expect("mailbox list must return JSON");
        mailboxes
            .into_iter()
            .find(|mailbox| mailbox["skarbiec_item_id"] == item_id)
            .expect("the declared item must be listed as a mailbox")
    }

    /// The tags Skarbiec reports for one item.
    pub(crate) fn vault_tags(&self, item_id: &str) -> Vec<String> {
        let output = self.skarbiec(&["list"]);
        assert_success("list Skarbiec items", &output);
        let items: Vec<Value> =
            serde_json::from_slice(&output.stdout).expect("skarbiec list must return JSON");
        items
            .into_iter()
            .find(|item| item["id"] == item_id)
            .and_then(|item| item["tags"].as_array().cloned())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|tag| tag.as_str().map(str::to_owned))
            .collect()
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
