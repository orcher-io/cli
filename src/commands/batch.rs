//! Batch operations CLI commands.
//!
//! Perform bulk administrative actions on workflows matching a filter.
//!
//! # Examples
//!
//! ```bash
//! # Terminate all running workflows of a type
//! orcher batch start --operation terminate --namespace production \
//!   --status running --workflow-type payment_workflow --reason "outage"
//!
//! # Cancel matching workflows at 100 ops/sec
//! orcher batch start --operation cancel --namespace default \
//!   --status running --rate 100
//!
//! # List batch operations
//! orcher batch list
//!
//! # Check progress of a batch operation
//! orcher batch describe <job-id>
//!
//! # Stop a running batch operation
//! orcher batch terminate <job-id> --reason "wrong query"
//! ```

use crate::client::connection::ConnectionManager;
use crate::client::with_gateway_auth;
use crate::error::{CliError, Result};
use crate::render;
use crate::utils::GlobalConfig;
use clap::Subcommand;

#[derive(clap::Parser, Debug)]
pub struct BatchCommand {
    #[command(subcommand)]
    pub action: BatchAction,
}

#[derive(Subcommand, Debug)]
pub enum BatchAction {
    /// Start a new batch operation on matching workflows
    Start {
        /// Operation type: terminate, cancel, send_event, reset. (No `-o`:
        /// that is the global --output.)
        #[arg(long)]
        operation: String,

        /// Namespace to target
        #[arg(long, default_value = "default")]
        namespace: String,

        /// Filter by workflow status (e.g., running, failed, completed)
        #[arg(long)]
        status: Option<String>,

        /// Filter by workflow type
        #[arg(long, alias = "type")]
        workflow_type: Option<String>,

        /// Rate limit: operations per second (default: 50, max: 1000)
        #[arg(long, default_value = "50")]
        rate: i32,

        /// Reason for the batch operation (required for audit)
        #[arg(long, short = 'r')]
        reason: Option<String>,

        /// Event name (required for send_event operation)
        #[arg(long)]
        event_name: Option<String>,

        /// Event payload as JSON string (for send_event operation)
        #[arg(long)]
        event_payload: Option<String>,

        /// Reset to event ID (required for reset operation)
        #[arg(long)]
        reset_event_id: Option<i64>,

        /// Skip confirmation prompt
        #[arg(long, short = 'f')]
        force: bool,
    },

    /// List batch operations
    List {
        /// Filter by status (pending, processing, completed, failed, terminated)
        #[arg(long)]
        status: Option<String>,

        /// Maximum number of results
        #[arg(long, default_value = "20")]
        limit: i32,
    },

    /// Show details of a batch operation
    Describe {
        /// Batch operation job ID
        job_id: String,
    },

    /// Stop a running batch operation
    Terminate {
        /// Batch operation job ID
        job_id: String,

        /// Reason for termination
        #[arg(long, short = 'r')]
        reason: Option<String>,
    },
}

pub async fn execute(cmd: BatchCommand, global_config: &GlobalConfig) -> Result<()> {
    let gateway_url = resolve_gateway_url(global_config);

    match cmd.action {
        BatchAction::Start {
            operation,
            namespace,
            status,
            workflow_type,
            rate,
            reason,
            event_name,
            event_payload,
            reset_event_id,
            force,
        } => {
            // Validate operation type
            if !["terminate", "cancel", "send_event", "reset"].contains(&operation.as_str()) {
                return Err(CliError::invalid_input(format!(
                    "Invalid operation type: '{}'. Must be one of: terminate, cancel, send_event, reset",
                    operation
                )));
            }

            // Build filter description for confirmation
            let filter_desc = format!(
                "namespace={}, status={}, workflow_type={}",
                namespace,
                status.as_deref().unwrap_or("any"),
                workflow_type.as_deref().unwrap_or("any"),
            );

            // Confirm with user
            if !force {
                let confirmed = crate::commands::common::confirm_action(
                    &format!(
                        "This will {} all workflows matching [{}] at {} ops/sec. Continue?",
                        operation, filter_desc, rate
                    ),
                    false,
                )?;
                if !confirmed {
                    println!("Aborted.");
                    return Ok(());
                }
            }

            // Build request body
            let mut body = serde_json::json!({
                "operationType": operation,
                "namespace": namespace,
                "rateLimitPerSecond": rate,
            });

            if let Some(s) = &status {
                body["filterStatus"] = serde_json::Value::String(s.clone());
            }
            if let Some(wt) = &workflow_type {
                body["filterWorkflowType"] = serde_json::Value::String(wt.clone());
            }
            if let Some(en) = &event_name {
                body["eventName"] = serde_json::Value::String(en.clone());
            }
            if let Some(ep) = &event_payload {
                body["eventPayload"] =
                    serde_json::from_str(ep).unwrap_or(serde_json::Value::String(ep.clone()));
            }
            if let Some(re) = reset_event_id {
                body["resetEventId"] = serde_json::Value::Number(re.into());
            }
            if let Some(r) = &reason {
                body["resetReason"] = serde_json::Value::String(r.clone());
            }

            let client = reqwest::Client::new();
            let resp = with_gateway_auth(
                client.post(format!("{}/api/v1/batch-operations", gateway_url)),
                global_config,
            )
            .json(&body)
            .send()
            .await
            .map_err(|e| CliError::internal(format!("Failed to create batch operation: {}", e)))?;

            if !resp.status().is_success() {
                let text = resp.text().await.unwrap_or_default();
                return Err(CliError::internal(format!("Gateway error: {}", text)));
            }

            let result: serde_json::Value = resp
                .json()
                .await
                .map_err(|e| CliError::internal(format!("Failed to parse response: {}", e)))?;

            let job_id = result["id"].as_str().unwrap_or("unknown");
            println!("Batch operation created: {}", job_id);
            println!("  Type:      {}", operation);
            println!("  Namespace: {}", namespace);
            println!("  Filter:    {}", filter_desc);
            println!("  Rate:      {} ops/sec", rate);
            println!();
            println!("Track progress: orcher batch describe {}", job_id);
            println!("Stop operation: orcher batch terminate {}", job_id);

            Ok(())
        }

        BatchAction::List { status, limit } => {
            let mut url = format!("{}/api/v1/batch-operations?limit={}", gateway_url, limit);
            if let Some(s) = &status {
                url.push_str(&format!("&status={}", s));
            }

            let client = reqwest::Client::new();
            let resp = with_gateway_auth(client.get(&url), global_config)
                .send()
                .await
                .map_err(|e| {
                    CliError::internal(format!("Failed to list batch operations: {}", e))
                })?;

            if !resp.status().is_success() {
                let text = resp.text().await.unwrap_or_default();
                return Err(CliError::internal(format!("Gateway error: {}", text)));
            }

            let result: serde_json::Value = resp
                .json()
                .await
                .map_err(|e| CliError::internal(format!("Failed to parse response: {}", e)))?;

            let ops = result["batchOperations"].as_array();
            if let Some(ops) = ops {
                if ops.is_empty() {
                    println!("No batch operations found.");
                    return Ok(());
                }

                // Structured output (-o json/yaml) for scripting.
                let items = serde_json::Value::Array(ops.clone());
                if let Some(out) = render::structured(&global_config.output_format, &items) {
                    println!("{}", out?);
                    return Ok(());
                }

                let rows: Vec<Vec<String>> = ops
                    .iter()
                    .map(|op| {
                        let processed = op["processedCount"].as_i64().unwrap_or(0);
                        let total = op["totalCount"].as_i64().unwrap_or(0);
                        let progress = if total > 0 {
                            format!("{}/{} ({}%)", processed, total, processed * 100 / total)
                        } else {
                            format!("{}/—", processed)
                        };
                        vec![
                            render::short_id(op["id"].as_str().unwrap_or("")),
                            render::or_dash(op["operationType"].as_str().unwrap_or("")),
                            render::status_cell_str(op["status"].as_str().unwrap_or("")),
                            progress,
                            render::relative_time_rfc3339(op["createdAt"].as_str().unwrap_or("")),
                        ]
                    })
                    .collect();

                println!(
                    "{}",
                    render::table(&["ID", "TYPE", "STATUS", "PROGRESS", "CREATED"], rows)
                );
            }

            Ok(())
        }

        BatchAction::Describe { job_id } => {
            let client = reqwest::Client::new();
            let resp = with_gateway_auth(
                client.get(format!(
                    "{}/api/v1/batch-operations/{}",
                    gateway_url, job_id
                )),
                global_config,
            )
            .send()
            .await
            .map_err(|e| CliError::internal(format!("Failed to get batch operation: {}", e)))?;

            if !resp.status().is_success() {
                let text = resp.text().await.unwrap_or_default();
                return Err(CliError::internal(format!("Gateway error: {}", text)));
            }

            let op: serde_json::Value = resp
                .json()
                .await
                .map_err(|e| CliError::internal(format!("Failed to parse response: {}", e)))?;

            println!("Batch Operation: {}", op["id"].as_str().unwrap_or("—"));
            println!(
                "  Type:           {}",
                op["operationType"].as_str().unwrap_or("—")
            );
            println!("  Status:         {}", op["status"].as_str().unwrap_or("—"));
            println!(
                "  Namespace:      {}",
                op["namespace"].as_str().unwrap_or("—")
            );
            if let Some(fs) = op["filterStatus"].as_str() {
                println!("  Filter Status:  {}", fs);
            }
            if let Some(ft) = op["filterWorkflowType"].as_str() {
                println!("  Filter Type:    {}", ft);
            }
            println!(
                "  Rate Limit:     {} ops/sec",
                op["rateLimitPerSecond"].as_i64().unwrap_or(50)
            );
            println!();
            println!("  Progress:");
            println!("    Total:     {}", op["totalCount"].as_i64().unwrap_or(0));
            println!(
                "    Processed: {}",
                op["processedCount"].as_i64().unwrap_or(0)
            );
            println!(
                "    Success:   {}",
                op["successCount"].as_i64().unwrap_or(0)
            );
            println!(
                "    Failure:   {}",
                op["failureCount"].as_i64().unwrap_or(0)
            );
            println!();
            println!("  Created:    {}", op["createdAt"].as_str().unwrap_or("—"));
            if let Some(sa) = op["startedAt"].as_str() {
                println!("  Started:    {}", sa);
            }
            if let Some(ca) = op["completedAt"].as_str() {
                println!("  Completed:  {}", ca);
            }
            if let Some(err) = op["error"].as_str() {
                println!("  Error:      {}", err);
            }

            Ok(())
        }

        BatchAction::Terminate { job_id, reason } => {
            let client = reqwest::Client::new();
            let resp = with_gateway_auth(
                client.post(format!(
                    "{}/api/v1/batch-operations/{}/terminate",
                    gateway_url, job_id
                )),
                global_config,
            )
            .send()
            .await
            .map_err(|e| {
                CliError::internal(format!("Failed to terminate batch operation: {}", e))
            })?;

            if !resp.status().is_success() {
                let text = resp.text().await.unwrap_or_default();
                return Err(CliError::internal(format!("Gateway error: {}", text)));
            }

            println!("Batch operation {} terminated.", job_id);
            if let Some(r) = &reason {
                println!("  Reason: {}", r);
            }

            Ok(())
        }
    }
}

fn resolve_gateway_url(config: &GlobalConfig) -> String {
    // Batch operations hit the gateway's HTTP API. `config.server` is the gRPC
    // address, so derive the gateway HTTP address the same way the rest of the
    // CLI does rather than talking HTTP to the gRPC port.
    ConnectionManager::from_config(config)
        .http_addr()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gateway(url: &str) -> GlobalConfig {
        GlobalConfig {
            api_url: Some(url.to_string()),
            output_format: "json".to_string(),
            ..Default::default()
        }
    }

    /// Batch operations go to the configured gateway, with the credential.
    #[tokio::test]
    async fn batch_list_calls_the_gateway_with_the_credential() {
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/api/v1/batch-operations")
            .match_query(mockito::Matcher::UrlEncoded("limit".into(), "20".into()))
            .match_header("authorization", "Bearer token-1")
            .with_header("content-type", "application/json")
            .with_body(r#"{"batchOperations":[{"id":"b1","operationType":"cancel","status":"completed","processedCount":2,"totalCount":2}]}"#)
            .create_async()
            .await;
        std::env::set_var("ORCHER_TOKEN", "token-1");
        let result = execute(
            BatchCommand {
                action: BatchAction::List {
                    status: None,
                    limit: 20,
                },
            },
            &gateway(&server.url()),
        )
        .await;
        std::env::remove_var("ORCHER_TOKEN");
        result.unwrap();
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn a_gateway_error_is_reported() {
        let mut server = mockito::Server::new_async().await;
        server
            .mock("GET", "/api/v1/batch-operations/b9")
            .with_status(404)
            .with_body("no such batch operation")
            .create_async()
            .await;
        let err = execute(
            BatchCommand {
                action: BatchAction::Describe {
                    job_id: "b9".into(),
                },
            },
            &gateway(&server.url()),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("no such batch operation"), "{err}");
    }
}
