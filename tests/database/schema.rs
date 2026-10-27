//! Real CLI admission against a dedicated, quiescent fleet PostgreSQL database.
//! Requires real synced mail; never point this journey at operator data.
use anyhow::{ensure, Context, Result};
use serde_json::{json, Value};
use stado_database::{params, sync::Client, FleetDatabase};
use std::{env, fs, path::PathBuf, process::Command};

struct Journey {
    output: PathBuf,
    commands: Vec<Value>,
    home: PathBuf,
}
impl Journey {
    fn command(&mut self, program: &str, args: &[&str]) -> Result<std::process::Output> {
        let result = Command::new(program)
            .args(args)
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .env("SKRZYNKA_FLEET_HOME", &self.home)
            .output()
            .with_context(|| format!("start {program} {args:?}"))?;
        let index = self.commands.len();
        fs::write(self.output.join(format!("{index}.stdout")), &result.stdout)?;
        fs::write(self.output.join(format!("{index}.stderr")), &result.stderr)?;
        self.commands
            .push(json!({"argv": ([vec![program], args.to_vec()].concat()),
            "exit_status": result.status.code(), "status": result.status.to_string(),
            "stdout": format!("{index}.stdout"), "stderr": format!("{index}.stderr")}));
        Ok(result)
    }
    fn cli(&mut self, args: &[&str]) -> Result<std::process::Output> {
        self.command(env!("CARGO_BIN_EXE_skrzynka"), args)
    }
}
fn required(name: &str) -> Result<String> {
    let value = env::var(name).with_context(|| format!("{name} is required"))?;
    ensure!(!value.trim().is_empty(), "{name} must not be blank");
    Ok(value)
}
fn text(db: &Client, query: &str) -> Result<String> {
    Ok(db.query_row(query, params![], |row| row.get::<_, String>("value"))?)
}
fn snapshot(db: &Client) -> Result<Value> {
    let tables: Vec<String> = db.prepare(
        "SELECT tablename FROM pg_tables WHERE schemaname = current_schema() ORDER BY tablename"
    )?.query_map(params![], |row| row.get::<_, String>("tablename"))?
        .collect::<std::result::Result<_, _>>()?;
    let mut rows = serde_json::Map::new();
    for table in tables {
        let quoted = table.replace('"', "\"\"");
        let values = text(db, &format!(
            "SELECT COALESCE(jsonb_agg(value ORDER BY value), '[]'::jsonb)::text AS value FROM (SELECT to_jsonb(t) AS value FROM \"{quoted}\" t) records"
        ))?;
        rows.insert(table, serde_json::from_str(&values)?);
    }
    let columns = text(db,
        "SELECT COALESCE(jsonb_agg(to_jsonb(c) ORDER BY table_name, ordinal_position), '[]'::jsonb)::text AS value FROM information_schema.columns c WHERE table_schema = current_schema()"
    )?;
    Ok(json!({"rows": rows, "columns": serde_json::from_str::<Value>(&columns)?}))
}
fn exercise(journey: &mut Journey) -> Result<()> {
    let revision = required("SKRZYNKA_SCHEMA_TEST_REVISION")?;
    let source = journey.command(
        "git",
        &["rev-parse", "--verify", &format!("{revision}^{{commit}}")],
    )?;
    ensure!(
        source.status.success(),
        "cannot resolve candidate revision: {}",
        String::from_utf8_lossy(&source.stderr)
    );
    let identity = journey.cli(&["version"])?;
    ensure!(
        identity.status.success(),
        "candidate version failed: {}",
        String::from_utf8_lossy(&identity.stderr)
    );
    let identity: Value = serde_json::from_slice(&identity.stdout)?;
    ensure!(
        identity["source"].as_str() == Some(String::from_utf8_lossy(&source.stdout).trim()),
        "candidate source does not match requested commit"
    );
    fs::write(
        journey.output.join("identity.json"),
        serde_json::to_vec_pretty(&identity)?,
    )?;
    let expected_database = required("SKRZYNKA_SCHEMA_TEST_DATABASE")?;
    ensure!(
        expected_database != "skrzynka",
        "the production database name is refused"
    );
    ensure!(
        required("SKRZYNKA_SCHEMA_TEST_PURPOSE")? == "isolated-qualification",
        "declare isolated-qualification purpose"
    );
    let mut fleet = FleetDatabase::for_product("skrzynka", "SKRZYNKA_FLEET_HOME")?;
    fleet.home = journey.home.clone();
    let db = Client::connect(&fleet)?;
    ensure!(
        text(&db, "SELECT current_database() AS value")? == expected_database,
        "resolved database is not the declared qualification database"
    );
    let organization = required("SKRZYNKA_SCHEMA_TEST_ORGANIZATION")?;
    let message_args = ["--organization", organization.as_str(), "message", "list"];
    let initial = journey.cli(&message_args)?;
    ensure!(
        initial.status.success(),
        "initial message read failed: {}",
        String::from_utf8_lossy(&initial.stderr)
    );
    let messages: Value = serde_json::from_slice(&initial.stdout)?;
    ensure!(
        messages.as_array().is_some_and(|rows| !rows.is_empty()),
        "sync real mail into the isolated database before qualification"
    );
    let version = db.query_row(
        "SELECT value FROM schema_meta WHERE key='schema_version'",
        params![],
        |row| row.get::<_, i64>("value"),
    )?;
    let newer: i64 = required("SKRZYNKA_SCHEMA_TEST_NEWER_VERSION")?
        .parse()
        .context("SKRZYNKA_SCHEMA_TEST_NEWER_VERSION must be an integer")?;
    ensure!(
        newer > version,
        "SKRZYNKA_SCHEMA_TEST_NEWER_VERSION must exceed the candidate schema version {version}"
    );
    let original = snapshot(&db)?;
    fs::write(
        journey.output.join("original.json"),
        serde_json::to_vec_pretty(&original)?,
    )?;
    let outcome = (|| -> Result<()> {
        for case in ["newer", "missing", "malformed"] {
            match case {
                "newer" => {
                    db.execute(
                        "UPDATE schema_meta SET value=$1 WHERE key='schema_version'",
                        [newer],
                    )?;
                }
                "missing" => {
                    db.execute(
                        "DELETE FROM schema_meta WHERE key='schema_version'",
                        params![],
                    )?;
                }
                "malformed" => {
                    db.execute_batch(
                        "ALTER TABLE schema_meta ALTER COLUMN value TYPE TEXT USING value::text",
                    )?;
                    db.execute(
                        "UPDATE schema_meta SET value=$1 WHERE key='schema_version'",
                        ["unreadable"],
                    )?;
                }
                _ => unreachable!(),
            }
            let before = snapshot(&db)?;
            let refused = journey.cli(&message_args)?;
            let after = snapshot(&db)?;
            fs::write(
                journey.output.join(format!("{case}.json")),
                serde_json::to_vec_pretty(&json!({"before": before, "after": after}))?,
            )?;
            ensure!(!refused.status.success(), "{case} schema was accepted");
            let stderr = std::str::from_utf8(&refused.stderr)?;
            let terminal = stderr
                .lines()
                .last()
                .context("CLI omitted its terminal error")?;
            let error: Value = serde_json::from_str(terminal).with_context(|| {
                format!("{case}: CLI did not return its terminal JSON error: {stderr}")
            })?;
            if case == "newer" {
                ensure!(
                    error["error"]["code"] == "DATABASE_SCHEMA_UNSUPPORTED",
                    "newer schema returned the wrong refusal: {error}"
                );
            } else {
                ensure!(
                    error["error"]["code"] == "INTERNAL_ERROR",
                    "{case} metadata returned an unrelated refusal: {error}"
                );
            }
            ensure!(
                before == after,
                "{case}: refused open changed persisted rows or schema"
            );
            restore(&db, version)?;
        }
        let recovered = journey.cli(&message_args)?;
        ensure!(
            recovered.status.success(),
            "read after restoring metadata failed: {}",
            String::from_utf8_lossy(&recovered.stderr)
        );
        ensure!(
            serde_json::from_slice::<Value>(&recovered.stdout)? == messages,
            "recovery changed the real mailbox messages"
        );
        ensure!(
            snapshot(&db)? == original,
            "recovery changed persisted database state"
        );
        Ok(())
    })();
    let cleanup = restore(&db, version);
    match (outcome, cleanup) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) => Err(error),
        (Ok(()), Err(error)) => Err(error.context("restore schema metadata")),
        (Err(error), Err(cleanup)) => {
            anyhow::bail!("{error:#}; metadata restoration also failed: {cleanup:#}")
        }
    }
}
fn restore(db: &Client, version: i64) -> Result<()> {
    let transaction = db.transaction()?;
    transaction.execute(
        "DELETE FROM schema_meta WHERE key='schema_version'",
        params![],
    )?;
    transaction.execute(
        "ALTER TABLE schema_meta ALTER COLUMN value TYPE BIGINT USING value::bigint",
        params![],
    )?;
    transaction.execute(
        "INSERT INTO schema_meta(key,value) VALUES ('schema_version',$1)",
        [version],
    )?;
    transaction.commit()?;
    Ok(())
}
#[test]
#[ignore = "requires a dedicated quiescent fleet database containing real synced mail"]
fn schema_refusals_preserve_persisted_mail() -> Result<()> {
    let output = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(".build/real-tests/schema")
        .join(uuid::Uuid::new_v4().to_string());
    fs::create_dir_all(&output)?;
    let mut journey = Journey {
        output,
        commands: Vec::new(),
        home: PathBuf::from(required("SKRZYNKA_SCHEMA_TEST_FLEET_HOME")?),
    };
    let result = exercise(&mut journey);
    fs::write(
        journey.output.join("report.json"),
        serde_json::to_vec_pretty(&json!({
            "passed": result.is_ok(), "error": result.as_ref().err().map(|error| format!("{error:#}")), "commands": journey.commands,
        }))?,
    )?;
    println!("schema qualification: {}", journey.output.display());
    result
}
