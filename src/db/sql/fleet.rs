//! Where Skrzynka's state lives: the fleet database `skrzynka`. Stado names
//! the Skarbiec item that holds its address (`stado database resolve`), and
//! Skarbiec answers the pooler URL and the provider's root certificate to
//! the consumer `skrzynka-database-client`, whose bearer Stado keeps in
//! `~/.stado/skrzynka-database-client-skarbiec-token`. Every refusal names
//! the step that failed and what it answered.
//!
//! Stado and that bearer belong to the account running Skrzynka. A caller
//! that gives Skrzynka another HOME (the mailbox journeys isolate Skarbiec
//! and onboarding state that way) names the account's own home in
//! `SKRZYNKA_FLEET_HOME`.

use std::path::PathBuf;
use std::process::{Command, Stdio};

use postgres::config::SslMode;
use postgres::Client;
use serde::de::DeserializeOwned;
use serde::Deserialize;
use serde_json::Value;

use crate::error::AppError;

const DATABASE: &str = "skrzynka";
/// Who asks Stado's directory; the database lists `skrzynka` as consumer.
const DIRECTORY_CONSUMER: &str = "skrzynka";
/// Who reads the credential item; it may read exactly the two fields below.
const CREDENTIAL_CONSUMER: &str = "skrzynka-database-client";
const TOKEN_FILE: &str = "skrzynka-database-client-skarbiec-token";

#[derive(Deserialize)]
struct Resolution {
    credential_item: String,
}

#[derive(Deserialize)]
struct Route {
    url: String,
}

fn refused(detail: String) -> AppError {
    tracing::error!(detail = %detail, "fleet database connection failed");
    AppError::new(
        axum::http::StatusCode::SERVICE_UNAVAILABLE,
        "DATABASE_UNREACHABLE",
        format!("the fleet database {DATABASE} is unreachable: {detail}"),
        true,
    )
}

fn home() -> Result<PathBuf, AppError> {
    std::env::var_os("SKRZYNKA_FLEET_HOME")
        .or_else(|| std::env::var_os("HOME"))
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| {
            refused(
                "neither SKRZYNKA_FLEET_HOME nor HOME is set, so Stado cannot be found".to_owned(),
            )
        })
}

fn stado() -> Result<PathBuf, AppError> {
    let stado = home()?.join(".stado/bin/stado");
    if stado.is_file() {
        Ok(stado)
    } else {
        Err(refused(format!(
            "Stado is not installed at {}; Skrzynka finds its database through Stado",
            stado.display()
        )))
    }
}

/// `stado <arguments>`, with exactly `environment` when one is given, so no
/// ambient variable selects the identity a credential read runs under.
fn run(arguments: &[&str], environment: Option<&[(&str, String)]>) -> Result<String, AppError> {
    let operation = format!("stado {}", arguments.join(" "));
    let mut command = Command::new(stado()?);
    command.args(arguments).stdin(Stdio::null());
    if let Some(environment) = environment {
        command.env_clear();
        for (name, value) in environment {
            command.env(name, value);
        }
    }
    let output = command
        .output()
        .map_err(|error| refused(format!("{operation} could not start: {error}")))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        return Err(refused(format!(
            "{operation} exited {}: {}",
            output.status,
            detail.trim()
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn answer<T: DeserializeOwned>(arguments: &[&str]) -> Result<T, AppError> {
    let output = run(arguments, None)?;
    serde_json::from_str(&output).map_err(|error| {
        refused(format!(
            "stado {} answered unreadable JSON: {error}",
            arguments.join(" ")
        ))
    })
}

/// A value answered as a JSON string, as `{"value": …}`, or as text.
fn decoded(output: &str) -> Option<String> {
    let value = match serde_json::from_str::<Value>(output) {
        Ok(Value::String(value)) => value,
        Ok(Value::Object(object)) => object.get("value")?.as_str()?.to_owned(),
        Ok(_) => return None,
        Err(_) => output.to_owned(),
    };
    let value = value.trim().to_owned();
    (!value.is_empty()).then_some(value)
}

fn read_field(route: &str, item: &str, field: &str) -> Result<String, AppError> {
    let home = home()?;
    let environment = [
        ("HOME", home.display().to_string()),
        (
            "PATH",
            std::env::var("PATH").unwrap_or_else(|_| "/usr/local/bin:/usr/bin:/bin".into()),
        ),
        ("TMPDIR", std::env::temp_dir().display().to_string()),
        ("STADO_CREDENTIALS_ADMIN_URL", route.to_owned()),
        ("STADO_CREDENTIALS_ADMIN_CONSUMER", CREDENTIAL_CONSUMER.to_owned()),
        (
            "STADO_CREDENTIALS_ADMIN_TOKEN_FILE",
            home.join(".stado").join(TOKEN_FILE).display().to_string(),
        ),
    ];
    let output = run(&["secrets", "get", item, "--field", field], Some(&environment))?;
    decoded(&output).ok_or_else(|| {
        refused(format!(
            "stado secrets get {item} --field {field} as {CREDENTIAL_CONSUMER} answered an empty value"
        ))
    })
}

/// A client of the fleet database, over TLS verified against the provider
/// root certificate the credential item carries.
pub fn connect() -> Result<Client, AppError> {
    let resolution: Resolution = answer(&[
        "database",
        "resolve",
        DATABASE,
        "--consumer",
        DIRECTORY_CONSUMER,
        "--json",
    ])?;
    let route: Route = answer(&[
        "service",
        "directory",
        "connect",
        "skarbiec",
        "--consumer",
        DIRECTORY_CONSUMER,
        "--json",
    ])?;
    let item = resolution.credential_item;
    let url = read_field(&route.url, &item, "pooler_url")?;
    let certificate = read_field(&route.url, &item, "ca_certificate")?;
    let certificate = native_tls::Certificate::from_pem(certificate.as_bytes()).map_err(|error| {
        refused(format!("{item}#ca_certificate is not a PEM certificate: {error}"))
    })?;
    let tls = native_tls::TlsConnector::builder()
        .add_root_certificate(certificate)
        .build()
        .map_err(|error| refused(format!("the TLS connector could not be built: {error}")))?;
    let mut config: postgres::Config = url.parse().map_err(|error| {
        refused(format!("{item}#pooler_url is not a Postgres connection URL: {error}"))
    })?;
    config.ssl_mode(SslMode::Require);
    super::blocking(|| config.connect(postgres_native_tls::MakeTlsConnector::new(tls)))
        .map_err(|error| refused(format!("connecting through {item}#pooler_url failed: {error}")))
}
