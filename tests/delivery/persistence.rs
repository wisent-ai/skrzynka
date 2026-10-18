//! Real CLI and fleet database refusals; no SMTP server or vault is simulated.
//! Requires a dedicated database containing a real imported message.
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use stado_database::{params, sync::Client, FleetDatabase};
use std::{env, fs, path::PathBuf, process::Command};
use uuid::Uuid;

fn required(name: &str) -> Result<String> {
    let value = env::var(name).with_context(|| format!("{name} is required"))?;
    ensure!(!value.trim().is_empty(), "{name} must not be blank");
    Ok(value)
}

struct Journey {
    output: PathBuf,
    home: PathBuf,
    commands: Vec<Value>,
}
impl Journey {
    fn run(&mut self, program: &str, args: &[&str]) -> Result<std::process::Output> {
        let result = Command::new(program)
            .args(args)
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .env("SKRZYNKA_FLEET_HOME", &self.home)
            .output()
            .with_context(|| format!("start {program}"))?;
        let index = self.commands.len();
        fs::write(self.output.join(format!("{index}.stdout")), &result.stdout)?;
        fs::write(self.output.join(format!("{index}.stderr")), &result.stderr)?;
        self.commands.push(json!({"program": program, "args": args,
            "exit": result.status.code(), "status": result.status.to_string(),
            "stdout": format!("{index}.stdout"), "stderr": format!("{index}.stderr")}));
        Ok(result)
    }
    fn cli(&mut self, args: &[&str]) -> Result<std::process::Output> {
        self.run(env!("CARGO_BIN_EXE_skrzynka"), args)
    }
}

fn exercise(journey: &mut Journey) -> Result<()> {
    ensure!(
        env::var_os("SKRZYNKA_CREDENTIALS_FILE").is_none(),
        "credential-file overrides are refused; use the actual Skarbiec boundary"
    );
    ensure!(
        env::var_os("SKRZYNKA_DATABASE_URL").is_none(),
        "direct database overrides are refused; CLI and readback must use the declared fleet route"
    );
    let revision = required("SKRZYNKA_DELIVERY_TEST_REVISION")?;
    let source = journey.run(
        "git",
        &["rev-parse", "--verify", &format!("{revision}^{{commit}}")],
    )?;
    ensure!(
        source.status.success(),
        "source revision could not be resolved"
    );
    let identity = journey.cli(&["version"])?;
    ensure!(identity.status.success(), "candidate version failed");
    let identity: Value = serde_json::from_slice(&identity.stdout)?;
    ensure!(
        identity["source"].as_str() == Some(String::from_utf8_lossy(&source.stdout).trim()),
        "candidate does not identify the requested revision"
    );
    fs::write(
        journey.output.join("identity.json"),
        serde_json::to_vec_pretty(&identity)?,
    )?;
    let database = required("SKRZYNKA_DELIVERY_TEST_DATABASE")?;
    ensure!(database != "skrzynka", "production database is refused");
    ensure!(
        required("SKRZYNKA_DELIVERY_TEST_PURPOSE")? == "isolated-qualification",
        "declare isolated-qualification purpose"
    );
    let mut fleet = FleetDatabase::for_product("skrzynka", "SKRZYNKA_FLEET_HOME")?;
    fleet.home = journey.home.clone();
    let db = Client::connect(&fleet)?;
    let actual: String = db.query_row("SELECT current_database() AS name", params![], |row| {
        row.get("name")
    })?;
    ensure!(
        actual == database,
        "resolved database differs from the dedicated database declaration"
    );
    let organization = required("SKRZYNKA_DELIVERY_TEST_ORGANIZATION")?;
    let message = required("SKRZYNKA_DELIVERY_TEST_MESSAGE")?;
    let mailbox: String = db.query_row(
        "SELECT m.mailbox_id FROM messages m JOIN mailboxes b ON b.id=m.mailbox_id WHERE m.id=$1 AND b.organization_id=$2",
        params![message, organization], |row| row.get("mailbox_id"))?;
    let recipient = required("SKRZYNKA_DELIVERY_TEST_RECIPIENT")?;
    let body = journey.output.join("body.txt");
    fs::write(
        &body,
        "Delivery persistence qualification; SMTP must not be reached.",
    )?;
    let absent = journey.output.join("absent-skarbiec");
    ensure!(
        !absent.exists(),
        "qualification vault executable must be absent"
    );
    let body = body.to_str().context("body path is not UTF-8")?;
    let absent = absent.to_str().context("vault path is not UTF-8")?;
    for (table, kind) in [("reply_attempts", "reply"), ("outbound_messages", "send")] {
        let key = Uuid::new_v4().to_string();
        let constraint = format!("qualification_{}", Uuid::new_v4().simple());
        // A real PostgreSQL constraint refuses only the owned attempt's Failed write.
        db.execute_batch(&format!(
            "ALTER TABLE {table} ADD CONSTRAINT {constraint} CHECK (idempotency_key <> '{key}' OR status <> 'failed')"
        ))?;
        let result = (|| -> Result<()> {
            let mut args = vec![
                "--organization",
                &organization,
                "--skarbiec-bin",
                absent,
                "message",
                kind,
            ];
            if kind == "reply" {
                args.push(&message);
            } else {
                args.extend([
                    "--mailbox",
                    &mailbox,
                    "--to",
                    &recipient,
                    "--subject",
                    "Delivery persistence qualification",
                ]);
            }
            args.extend(["--body-file", body, "--idempotency-key", &key]);
            let refused = journey.cli(&args)?;
            ensure!(
                !refused.status.success(),
                "credential and persistence refusal returned success"
            );
            let stderr = String::from_utf8(refused.stderr)?;
            ensure!(
                stderr.contains(absent),
                "original vault operation cause was lost: {stderr}"
            );
            ensure!(
                stderr.contains(&constraint),
                "actual database constraint cause was lost: {stderr}"
            );
            ensure!(
                stderr.contains("last confirmed state was sending"),
                "state diagnostic claims an unobserved outcome: {stderr}"
            );
            let state: String = db.query_row(
                &format!("SELECT status FROM {table} WHERE idempotency_key=$1"),
                [&key],
                |row| row.get("status"),
            )?;
            ensure!(
                state == "sending",
                "refused terminal write unexpectedly persisted {state}"
            );
            fs::write(
                journey.output.join(format!("{kind}-refusal.json")),
                serde_json::to_vec_pretty(
                    &json!({"key": key, "constraint": constraint, "state": state}),
                )?,
            )?;
            Ok(())
        })();
        let cleanup = (|| -> Result<()> {
            db.execute_batch(&format!("ALTER TABLE {table} DROP CONSTRAINT {constraint}"))?;
            db.execute(
                &format!("DELETE FROM {table} WHERE idempotency_key=$1"),
                [&key],
            )?;
            Ok(())
        })();
        match (result, cleanup) {
            (Err(error), Err(cleanup)) => {
                anyhow::bail!("{error:#}; owned-state cleanup failed: {cleanup:#}")
            }
            (Err(error), Ok(())) => return Err(error),
            (Ok(()), Err(error)) => return Err(error.context("owned-state cleanup")),
            (Ok(()), Ok(())) => {}
        }
    }
    Ok(())
}

#[test]
#[ignore = "requires a dedicated quiescent fleet database with real imported mail"]
fn delivery_refusals_retain_original_and_persistence_causes() -> Result<()> {
    let output = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".build/real-tests/delivery")
        .join(Uuid::new_v4().to_string());
    fs::create_dir_all(&output)?;
    let mut journey = Journey {
        output,
        commands: Vec::new(),
        home: PathBuf::from(required("SKRZYNKA_DELIVERY_TEST_FLEET_HOME")?),
    };
    let result = exercise(&mut journey);
    fs::write(
        journey.output.join("report.json"),
        serde_json::to_vec_pretty(&json!({
            "passed": result.is_ok(), "error": result.as_ref().err().map(|error| format!("{error:#}")),
            "commands": journey.commands,
        }))?,
    )?;
    println!("delivery qualification: {}", journey.output.display());
    result
}
