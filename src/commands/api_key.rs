//! `orcher api-key` — issue, list, show and revoke the engine's API keys.
//!
//! Keys are managed over the engine's REST API (`/api/v1/api-keys`), for the
//! organisation the CLI's own credential acts for, and need the
//! `api-keys:manage` scope. A deployment's first key is issued on the engine's
//! side (`orchestrator api-key create`), since this needs a key to run.

use crate::client::connection::ConnectionManager;
use crate::client::with_gateway_auth;
use crate::error::{CliError, Result};
use crate::render;
use crate::utils::GlobalConfig;
use clap::{Args, Subcommand};
use serde_json::{json, Value};

/// API key management command
#[derive(Args)]
pub struct ApiKeyCommand {
    #[command(subcommand)]
    pub action: ApiKeyAction,
}

#[derive(Subcommand)]
pub enum ApiKeyAction {
    /// Issue a key. The key is printed once, on stdout; it is not stored
    Create {
        /// What the key is for, shown in listings
        name: String,

        /// A scope the key grants: workflows:read, workflows:write,
        /// workers:run, namespaces:manage, api-keys:manage, or * for all.
        /// Repeat or comma-separate for several. The key may be given only
        /// scopes your own credential holds.
        #[arg(short = 's', long = "scope", required = true, value_delimiter = ',')]
        scopes: Vec<String>,

        /// Days until the key expires (never, when omitted)
        #[arg(long = "expires-in-days")]
        expires_in_days: Option<i64>,

        /// More about what the key is for
        #[arg(short = 'd', long = "description")]
        description: Option<String>,
    },

    /// List keys, revoked ones included
    #[command(aliases = ["ls"])]
    List,

    /// Show one key
    #[command(aliases = ["describe"])]
    Get {
        /// The key's id
        id: String,
    },

    /// Revoke a key; every engine refuses it at once
    Revoke {
        /// The key's id
        id: String,

        /// Revoke without asking
        #[arg(short = 'f', long = "force")]
        force: bool,
    },
}

pub async fn execute(cmd: ApiKeyCommand, global_config: &GlobalConfig) -> Result<()> {
    let base = format!(
        "{}/api/v1/api-keys",
        ConnectionManager::from_config(global_config).http_addr()
    );
    let client = reqwest::Client::new();

    match cmd.action {
        ApiKeyAction::Create {
            name,
            scopes,
            expires_in_days,
            description,
        } => {
            let mut body = json!({ "name": name, "scopes": scopes });
            if let Some(days) = expires_in_days {
                body["expiresInDays"] = json!(days);
            }
            if let Some(description) = description {
                body["description"] = json!(description);
            }
            let key =
                send(with_gateway_auth(client.post(&base), global_config).json(&body)).await?;
            if let Some(rendered) = render::structured(&global_config.output_format, &key) {
                println!("{}", rendered?);
                return Ok(());
            }
            eprintln!(
                "Issued API key '{}' ({}), scopes {}{}. It is shown once and not stored:",
                text(&key, "name"),
                text(&key, "id"),
                scopes_of(&key),
                key.get("expiresAt")
                    .and_then(Value::as_str)
                    .map_or(String::new(), |at| format!(", expires {at}")),
            );
            println!("{}", text(&key, "key"));
        }
        ApiKeyAction::List => {
            let listing = send(with_gateway_auth(client.get(&base), global_config)).await?;
            if let Some(rendered) = render::structured(&global_config.output_format, &listing) {
                println!("{}", rendered?);
                return Ok(());
            }
            let keys = listing
                .get("keys")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            if keys.is_empty() {
                println!("No API keys.");
                return Ok(());
            }
            let rows = keys
                .iter()
                .map(|key| {
                    vec![
                        text(key, "id"),
                        text(key, "keyPrefix"),
                        text(key, "name"),
                        state_of(key).to_string(),
                        scopes_of(key),
                        key.get("lastUsedAt")
                            .and_then(Value::as_str)
                            .map_or_else(|| "never".to_string(), render::relative_time_rfc3339),
                    ]
                })
                .collect();
            println!(
                "{}",
                render::table(
                    &["ID", "PREFIX", "NAME", "STATE", "SCOPES", "LAST USED"],
                    rows
                )
            );
        }
        ApiKeyAction::Get { id } => {
            let key = send(with_gateway_auth(
                client.get(format!("{base}/{id}")),
                global_config,
            ))
            .await?;
            if let Some(rendered) = render::structured(&global_config.output_format, &key) {
                println!("{}", rendered?);
                return Ok(());
            }
            for (label, value) in [
                ("ID", text(&key, "id")),
                ("Name", text(&key, "name")),
                ("Prefix", text(&key, "keyPrefix")),
                ("State", state_of(&key).to_string()),
                ("Scopes", scopes_of(&key)),
                ("Description", text(&key, "description")),
                ("Created", text(&key, "createdAt")),
                ("Created by", text(&key, "createdBy")),
                ("Expires", text(&key, "expiresAt")),
                ("Last used", text(&key, "lastUsedAt")),
                ("Revoked", text(&key, "revokedAt")),
                ("Revoked by", text(&key, "revokedBy")),
            ] {
                println!("{label:<12} {}", render::or_dash(&value));
            }
        }
        ApiKeyAction::Revoke { id, force } => {
            if !crate::commands::common::confirm_action(
                &format!("Revoke API key {id}? Anything using it stops working at once."),
                force,
            )? {
                println!("Aborted.");
                return Ok(());
            }
            let key = send(with_gateway_auth(
                client.delete(format!("{base}/{id}")),
                global_config,
            ))
            .await?;
            if let Some(rendered) = render::structured(&global_config.output_format, &key) {
                println!("{}", rendered?);
                return Ok(());
            }
            println!(
                "Revoked API key '{}' ({}).",
                text(&key, "name"),
                text(&key, "id")
            );
        }
    }
    Ok(())
}

/// Send `request` and read the JSON answer, or say why the engine refused.
async fn send(request: reqwest::RequestBuilder) -> Result<Value> {
    let response = request
        .send()
        .await
        .map_err(|e| CliError::internal(format!("Could not reach the engine's REST API: {e}")))?;
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(Value::Null);
    if status.is_success() {
        return Ok(body);
    }
    // The engine's error envelope: {"error": {"message": ...}}.
    let message = body
        .pointer("/error/message")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| status.to_string());
    let message = message.trim_end_matches('.').to_string();
    Err(match status.as_u16() {
        400 | 422 => CliError::invalid_input(message),
        401 => CliError::auth(format!(
            "{message}. Log in, or set ORCHER_API_KEY to a key with the api-keys:manage scope"
        )),
        403 => CliError::Permission {
            message,
            path: None,
        },
        404 => CliError::internal(format!(
            "{message}. If this is a cloud deployment, manage its keys in the console"
        )),
        _ => CliError::internal(message),
    })
}

fn text(value: &Value, field: &str) -> String {
    value
        .get(field)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn scopes_of(key: &Value) -> String {
    key.get("scopes")
        .and_then(Value::as_array)
        .map(|scopes| {
            scopes
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(",")
        })
        .unwrap_or_default()
}

fn state_of(key: &Value) -> &'static str {
    if key.get("revokedAt").is_some_and(|v| !v.is_null()) {
        "revoked"
    } else if key.get("isActive").and_then(Value::as_bool) == Some(false) {
        "expired"
    } else {
        "active"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine(url: &str, output: &str) -> GlobalConfig {
        GlobalConfig {
            api_url: Some(url.to_string()),
            output_format: output.to_string(),
            ..Default::default()
        }
    }

    /// A key is issued with the scopes named, split on commas, and the
    /// credential the CLI holds.
    #[tokio::test]
    async fn create_sends_the_scopes_and_the_credential() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("POST", "/api/v1/api-keys")
            .match_header("authorization", "Bearer token-k")
            .match_body(mockito::Matcher::Json(json!({
                "name": "ci",
                "scopes": ["workflows:read", "workers:run"],
                "expiresInDays": 30
            })))
            .with_status(201)
            .with_header("content-type", "application/json")
            .with_body(r#"{"id":"k1","name":"ci","keyPrefix":"orch_abcdefg","scopes":["workflows:read","workers:run"],"isActive":true,"key":"orch_secret"}"#)
            .create_async()
            .await;
        let _env = crate::client::CREDENTIAL_ENV.lock().await;
        std::env::set_var("ORCHER_TOKEN", "token-k");
        let result = execute(
            ApiKeyCommand {
                action: ApiKeyAction::Create {
                    name: "ci".into(),
                    scopes: vec!["workflows:read".into(), "workers:run".into()],
                    expires_in_days: Some(30),
                    description: None,
                },
            },
            &engine(&server.url(), "table"),
        )
        .await;
        std::env::remove_var("ORCHER_TOKEN");
        result.unwrap();
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn a_refusal_carries_the_engines_reason() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/api/v1/api-keys")
            .with_status(403)
            .with_header("content-type", "application/json")
            .with_body(r#"{"error":{"code":"FORBIDDEN","status":403,"message":"Forbidden: The credential does not grant the 'api-keys:manage' scope this call needs"}}"#)
            .create_async()
            .await;
        let err = execute(
            ApiKeyCommand {
                action: ApiKeyAction::List,
            },
            &engine(&server.url(), "json"),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("api-keys:manage"), "{err}");
    }

    #[tokio::test]
    async fn a_request_the_engine_finds_invalid_is_reported_as_such() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("POST", "/api/v1/api-keys")
            .with_status(400)
            .with_header("content-type", "application/json")
            .with_body(r#"{"error":{"code":"VALIDATION_ERROR","status":400,"message":"Validation error: Unknown scopes [\"teleport\"]"}}"#)
            .create_async()
            .await;
        let err = execute(
            ApiKeyCommand {
                action: ApiKeyAction::Create {
                    name: "x".into(),
                    scopes: vec!["teleport".into()],
                    expires_in_days: None,
                    description: None,
                },
            },
            &engine(&server.url(), "json"),
        )
        .await
        .unwrap_err();
        assert!(matches!(err, CliError::InvalidInput { .. }), "{err}");
    }

    #[test]
    fn a_key_reads_as_revoked_expired_or_active() {
        assert_eq!(
            state_of(&json!({"revokedAt": "2026-10-26T00:00:00Z"})),
            "revoked"
        );
        assert_eq!(
            state_of(&json!({"revokedAt": null, "isActive": false})),
            "expired"
        );
        assert_eq!(
            state_of(&json!({"revokedAt": null, "isActive": true})),
            "active"
        );
    }
}
