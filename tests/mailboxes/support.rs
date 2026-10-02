use anyhow::{bail, ensure, Context, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

pub struct Run {
    pub organization: String,
    pub revision: String,
    directory: PathBuf,
    home: PathBuf,
    fleet_home: PathBuf,
    keyring: PathBuf,
    vault: Option<PathBuf>,
    skarbiec: String,
    item: Option<String>,
    tags: Vec<String>,
    touched: bool,
    report: Value,
}

impl Run {
    pub fn new() -> Result<Self> {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let directory = root
            .join("target/real-tests/mailboxes")
            .join(uuid::Uuid::new_v4().to_string());
        fs::create_dir_all(&directory)?;
        let home = directory.join("home");
        fs::create_dir(&home)?;
        let fleet_home =
            PathBuf::from(std::env::var("SKRZYNKA_FLEET_HOME").or_else(|_| std::env::var("HOME"))?);
        let keyring = std::env::var_os("GNUPGHOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| fleet_home.join(".gnupg"));
        let source = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(&root)
            .output()?;
        ensure!(
            source.status.success(),
            "git could not identify the source revision"
        );
        let revision = String::from_utf8(source.stdout)?.trim().to_owned();
        let diff = Command::new("git")
            .args(["diff", "HEAD", "--", "src", "Cargo.toml"])
            .current_dir(&root)
            .output()?;
        ensure!(diff.status.success(), "git could not identify source changes");
        let source_diff = String::from_utf8(diff.stdout)?;
        let organization = format!("mailbox-test-{}", uuid::Uuid::new_v4());
        Ok(Self {
            organization: organization.clone(),
            revision: revision.clone(),
            directory,
            home,
            fleet_home,
            keyring,
            vault: None,
            skarbiec: std::env::var("SKRZYNKA_TEST_SKARBIEC_BIN")
                .unwrap_or_else(|_| "skarbiec".into()),
            item: None,
            tags: Vec::new(),
            touched: false,
            report: json!({"source_revision": revision, "source_diff": source_diff,
                "test_sha256": format!("{:x}", Sha256::digest(include_str!("main.rs").as_bytes())),
                "support_sha256": format!("{:x}", Sha256::digest(include_str!("support.rs").as_bytes())),
                "organization": organization, "started_at": chrono::Utc::now().to_rfc3339(),
                "commands": [], "result": "not_run"}),
        })
    }

    pub fn prepare(&mut self, item: &str) -> Result<()> {
        ensure!(
            self.report["source_diff"] == "",
            "commit the product source before qualifying its exact revision"
        );
        let target = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("target")
            .canonicalize()?;
        let vault = PathBuf::from(required("SKARBIEC_VAULT_FILE")?).canonicalize()?;
        ensure!(
            vault.starts_with(&target),
            "the dedicated fixture vault must be inside this checkout's ignored target directory"
        );
        ensure!(
            vault.is_file(),
            "SKARBIEC_VAULT_FILE must name the prepared fixture vault"
        );
        self.vault = Some(vault);
        let inventory = self.vault(&["list"])?;
        let entries = inventory
            .as_array()
            .context("Skarbiec list did not return an array")?;
        ensure!(
            entries
                .iter()
                .all(|entry| !tags(entry).iter().any(|tag| tag == "skrzynka:mailbox")),
            "fixture vault already declares a mailbox; refusing to affect an existing declaration"
        );
        let fixture = entries
            .iter()
            .find(|entry| entry["id"] == item)
            .context("test item is absent from the fixture vault")?;
        self.tags = tags(fixture);
        ensure!(
            self.tags.iter().any(|tag| tag == "skrzynka:test"),
            "test item must carry skrzynka:test to identify a dedicated provider account"
        );
        self.item = Some(item.to_owned());
        Ok(())
    }

    pub fn record_fixture(&mut self, fixture: Value) {
        self.report["fixture"] = fixture;
    }

    fn execute(&mut self, binary: &str, args: &[&str]) -> Result<Output> {
        let mut command = Command::new(binary);
        command
            .args(args)
            .env("HOME", &self.home)
            .env("XDG_STATE_HOME", self.home.join(".local/state"))
            .env("SKRZYNKA_FLEET_HOME", &self.fleet_home)
            .env("GNUPGHOME", &self.keyring)
            .env(
                "SKARBIEC_AUDIT_FILE",
                self.directory.join("skarbiec-audit.jsonl"),
            );
        if let Some(vault) = &self.vault {
            command.env("SKARBIEC_VAULT_FILE", vault);
        }
        let output = command.output();
        match &output {
            Ok(value) => self.report["commands"].as_array_mut().unwrap().push(json!({
                "binary": binary, "args": args, "exit_status": value.status.code(),
                "stdout": String::from_utf8_lossy(&value.stdout), "stderr": String::from_utf8_lossy(&value.stderr),
            })),
            Err(error) => self.report["commands"].as_array_mut().unwrap().push(json!({
                "binary": binary, "args": args, "spawn_error": error.to_string(),
            })),
        }
        Ok(output?)
    }

    pub fn cli(&mut self, args: &[&str]) -> Result<Output> {
        let organization = self.organization.clone();
        let skarbiec = self.skarbiec.clone();
        let mut complete = vec!["--organization", &organization, "--skarbiec-bin", &skarbiec];
        complete.extend_from_slice(args);
        self.execute(env!("CARGO_BIN_EXE_skrzynka"), &complete)
    }

    pub fn ok(&mut self, args: &[&str]) -> Result<Value> {
        json_output(self.cli(args)?)
    }

    pub fn refusal(&mut self, args: &[&str], code: &str) -> Result<()> {
        let result = self.cli(args)?;
        ensure!(
            result.status.code() == Some(1),
            "expected application refusal, got {:?}: {}",
            result.status.code(),
            String::from_utf8_lossy(&result.stderr)
        );
        ensure!(
            result.stdout.is_empty(),
            "a refused command emitted success output"
        );
        let error: Value = serde_json::from_slice(&result.stderr)?;
        ensure!(
            error["error"]["code"] == code,
            "expected {code}, observed {error}"
        );
        Ok(())
    }

    pub fn vault(&mut self, args: &[&str]) -> Result<Value> {
        json_output(self.execute(&self.skarbiec.clone(), args)?)
    }

    pub fn declare(&mut self) -> Result<Value> {
        let item = self.item.clone().context("fixture has not been prepared")?;
        self.touched = true;
        self.ok(&["mailbox", "declare", "--skarbiec-item", &item])
    }

    pub fn clean(&mut self) -> Result<()> {
        if !self.touched {
            return Ok(());
        }
        let item = self
            .item
            .clone()
            .context("missing fixture identity during cleanup")?;
        self.vault(&["retag", &item, "--tags", &self.tags.join(",")])?;
        // Only this run's organization and isolated fixture vault are reconciled.
        let rows = self.ok(&["mailbox", "list"])?;
        for row in rows
            .as_array()
            .context("mailbox list did not return an array")?
        {
            ensure!(
                row["skarbiec_item_id"] == item,
                "unexpected mailbox in isolated test organization"
            );
            let id = row["id"].as_str().context("mailbox has no id")?;
            ensure!(
                row["enabled"] == false,
                "fixture mailbox stayed enabled after restoring its tags"
            );
            self.ok(&["mailbox", "remove", id, "--confirm"])?;
            self.refusal(&["mailbox", "show", id], "NOT_FOUND")?;
        }
        let inventory = self.vault(&["list"])?;
        let fixture = inventory
            .as_array()
            .context("invalid vault inventory")?
            .iter()
            .find(|entry| entry["id"] == item)
            .context("fixture item disappeared")?;
        let mut actual = tags(fixture);
        let mut original = self.tags.clone();
        actual.sort();
        original.sort();
        ensure!(actual == original, "fixture tags were not restored");
        self.touched = false;
        Ok(())
    }

    pub fn finish(&mut self, outcome: &Result<()>, cleanup: &Result<()>) -> Result<()> {
        self.report["finished_at"] = json!(chrono::Utc::now().to_rfc3339());
        self.report["result"] = json!(if outcome.is_ok() && cleanup.is_ok() {
            "passed"
        } else {
            "failed"
        });
        self.report["error"] = json!(outcome.as_ref().err().map(|error| format!("{error:#}")));
        self.report["cleanup_error"] =
            json!(cleanup.as_ref().err().map(|error| format!("{error:#}")));
        let path = self.directory.join("report.json");
        fs::write(&path, serde_json::to_vec_pretty(&self.report)?)?;
        eprintln!("mailbox qualification report: {}", path.display());
        fs::remove_dir_all(&self.home)?;
        Ok(())
    }
}

pub fn required(name: &str) -> Result<String> {
    let value = std::env::var(name)
        .with_context(|| format!("{name} is required for real mailbox qualification"))?;
    ensure!(!value.trim().is_empty(), "{name} must not be empty");
    Ok(value)
}

pub fn expected_body() -> Result<String> {
    let path = required("SKRZYNKA_TEST_BODY_FILE")?;
    Ok(fs::read_to_string(Path::new(&path))?)
}

fn tags(item: &Value) -> Vec<String> {
    item["tags"]
        .as_array()
        .map(|tags| {
            tags.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn json_output(result: Output) -> Result<Value> {
    if !result.status.success() {
        bail!(
            "command exited {:?}: {}",
            result.status.code(),
            String::from_utf8_lossy(&result.stderr)
        );
    }
    Ok(serde_json::from_slice(&result.stdout)?)
}
