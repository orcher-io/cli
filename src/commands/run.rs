//! `orcher run` — submit a workflow and poll until completion.

use crate::client::grpc_client::OrcherGrpcClient;
use crate::error::{CliError, Result};
use crate::time::Timestamptz;
use crate::utils::GlobalConfig;
use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use serde_json::Value;
use std::time::Duration;
use tracing::debug;

pub async fn execute(
    workflow: String,
    environment: Option<String>,
    params: Vec<String>,
    global_config: &GlobalConfig,
) -> Result<()> {
    if !global_config.quiet {
        println!("{}", style("Running workflow...").cyan().bold());
        println!();
    }

    // Parse parameters from key=value strings
    let parameters = parse_parameters(&params)?;

    // Get workflow name and task queue
    let (workflow_type, task_queue) = parse_workflow_identifier(&workflow)?;

    // Get namespace from environment or config
    let namespace = environment
        .or_else(|| global_config.get_namespace())
        .unwrap_or_else(|| "default".to_string());

    // Create gRPC client with smart detection
    let mut client = crate::client::connection::connect_grpc_in(global_config, &namespace).await?;

    // Submit workflow for execution
    let (workflow_id, execution_id) = submit_workflow(
        &mut client,
        &workflow_type,
        &task_queue,
        parameters,
        global_config,
    )
    .await?;

    if !global_config.quiet {
        println!(
            "{}  Workflow submitted: {}",
            style("✓").green(),
            style(&workflow_id).cyan()
        );
        if !execution_id.is_empty() {
            println!("   Execution ID: {}", style(&execution_id).dim());
        }
        println!();
    }

    // Wait for completion with progress indicator
    let result =
        wait_for_completion(&mut client, &workflow_id, &execution_id, global_config).await?;

    // Display final result
    display_result(&result, global_config)?;

    // Exit with appropriate status code
    if result.status == "FAILED" || result.status == "3" {
        std::process::exit(1);
    }

    Ok(())
}

/// Parse "key=value" strings into JSON bytes.
fn parse_parameters(params: &[String]) -> Result<Vec<u8>> {
    let mut map = serde_json::Map::new();

    for param in params {
        let parts: Vec<&str> = param.splitn(2, '=').collect();
        if parts.len() != 2 {
            return Err(CliError::InvalidInput {
                message: format!(
                    "Invalid parameter format: '{}'. Expected 'key=value'\nUse format: key=value\nExample: order_id=123\nFor JSON values: data='{{\"foo\":\"bar\"}}'",
                    param
                ),
                field: Some("parameters".to_string()),
                expected: Some("key=value".to_string()),
            });
        }

        let key = parts[0].to_string();
        let value = parts[1];

        // Try to parse as JSON first, otherwise treat as string
        let parsed_value = if value.starts_with('{') || value.starts_with('[') {
            serde_json::from_str(value).unwrap_or_else(|_| Value::String(value.to_string()))
        } else if let Ok(num) = value.parse::<i64>() {
            Value::Number(num.into())
        } else if let Ok(num) = value.parse::<f64>() {
            serde_json::Number::from_f64(num)
                .map(Value::Number)
                .unwrap_or_else(|| Value::String(value.to_string()))
        } else if value == "true" {
            Value::Bool(true)
        } else if value == "false" {
            Value::Bool(false)
        } else if value == "null" {
            Value::Null
        } else {
            Value::String(value.to_string())
        };

        map.insert(key, parsed_value);
    }

    // Serialize to JSON bytes
    let json = Value::Object(map);
    Ok(serde_json::to_vec(&json)?)
}

/// Parse "type[@queue]" — defaults queue to type if omitted.
fn parse_workflow_identifier(workflow: &str) -> Result<(String, String)> {
    // Format: workflow_type[@task_queue]
    // If no task queue specified, use workflow type as task queue
    if let Some((workflow_type, task_queue)) = workflow.split_once('@') {
        Ok((workflow_type.to_string(), task_queue.to_string()))
    } else {
        // Use workflow type as both workflow type and task queue name
        Ok((workflow.to_string(), workflow.to_string()))
    }
}

async fn submit_workflow(
    client: &mut OrcherGrpcClient,
    workflow_type: &str,
    task_queue: &str,
    input: Vec<u8>,
    global_config: &GlobalConfig,
) -> Result<(String, String)> {
    debug!(
        "Submitting workflow: {} to task queue: {}",
        workflow_type, task_queue
    );

    if !global_config.quiet {
        let spinner = ProgressBar::new_spinner();
        spinner.set_style(
            ProgressStyle::default_spinner()
                .template("{spinner:.cyan} {msg}")
                .unwrap()
                .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"),
        );
        spinner.set_message("Submitting workflow...");
        spinner.enable_steady_tick(Duration::from_millis(80));

        let response = client
            .start_workflow(
                workflow_type,
                None, // Let server generate workflow_id
                task_queue,
                Some(input),
            )
            .await?;

        spinner.finish_and_clear();

        Ok((response.workflow_id, response.execution_id))
    } else {
        // Quiet mode - no spinner
        let response = client
            .start_workflow(workflow_type, None, task_queue, Some(input))
            .await?;

        Ok((response.workflow_id, response.execution_id))
    }
}

async fn wait_for_completion(
    client: &mut OrcherGrpcClient,
    workflow_id: &str,
    execution_id: &str,
    global_config: &GlobalConfig,
) -> Result<ExecutionResult> {
    if !global_config.quiet {
        let spinner = ProgressBar::new_spinner();
        spinner.set_style(
            ProgressStyle::default_spinner()
                .template("{spinner:.cyan} {msg}")
                .unwrap()
                .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"),
        );
        spinner.enable_steady_tick(Duration::from_millis(80));

        let mut last_status = String::new();

        loop {
            // Get current status via gRPC
            let status = get_execution_status(client, workflow_id, execution_id).await?;

            // Update spinner message with current progress
            let message = format!("Running... (status: {})", status.status);

            if message != last_status {
                spinner.set_message(message.clone());
                last_status = message;
            }

            // Check if terminal state
            if is_terminal_status(&status.status) {
                spinner.finish_and_clear();
                return Ok(status);
            }

            // Wait before next poll (gRPC is faster, can poll more frequently)
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    } else {
        // Quiet mode - poll without spinner
        loop {
            let status = get_execution_status(client, workflow_id, execution_id).await?;

            if is_terminal_status(&status.status) {
                return Ok(status);
            }

            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
}

async fn get_execution_status(
    client: &mut OrcherGrpcClient,
    workflow_id: &str,
    execution_id: &str,
) -> Result<ExecutionResult> {
    let exec_id = if execution_id.is_empty() {
        None
    } else {
        Some(execution_id)
    };

    let response = client.get_workflow_status(workflow_id, exec_id).await?;

    // Map gRPC status enum to string
    let status_str = map_status_code(response.status);

    // Extract execution info
    let exec_id = if let Some(exec) = &response.execution {
        exec.execution_id.clone()
    } else {
        execution_id.to_string()
    };

    // Extract timestamps
    let started_at = response.started_at.as_ref().map(|ts| {
        Timestamptz::from_second(ts.seconds)
            .map(|dt| dt.to_rfc3339())
            .unwrap_or_default()
    });

    let completed_at = response.completed_at.as_ref().map(|ts| {
        Timestamptz::from_second(ts.seconds)
            .map(|dt| dt.to_rfc3339())
            .unwrap_or_default()
    });

    // Extract outcome (result or error)
    let (result, error) = match &response.outcome {
        Some(orcher_proto::get_workflow_status_response::Outcome::Result(bytes)) => {
            let parsed = serde_json::from_slice(bytes).ok();
            (parsed, None)
        }
        Some(orcher_proto::get_workflow_status_response::Outcome::Error(err)) => {
            (None, Some(err.clone()))
        }
        None => (None, None),
    };

    Ok(ExecutionResult {
        execution_id: exec_id,
        workflow_id: workflow_id.to_string(),
        status: status_str,
        started_at,
        completed_at,
        result,
        error,
    })
}

/// Map gRPC status code to string
fn map_status_code(status: i32) -> String {
    match status {
        0 => "UNSPECIFIED".to_string(),
        1 => "RUNNING".to_string(),
        2 => "COMPLETED".to_string(),
        3 => "FAILED".to_string(),
        4 => "CANCELED".to_string(),
        5 => "TERMINATED".to_string(),
        6 => "CONTINUED_AS_NEW".to_string(),
        7 => "TIMED_OUT".to_string(),
        _ => format!("UNKNOWN({})", status),
    }
}

/// Check if status is terminal
fn is_terminal_status(status: &str) -> bool {
    matches!(
        status,
        "COMPLETED"
            | "FAILED"
            | "CANCELED"
            | "CANCELLED"
            | "TERMINATED"
            | "TIMED_OUT"
            | "CONTINUED_AS_NEW"
    )
}

fn display_result(result: &ExecutionResult, global_config: &GlobalConfig) -> Result<()> {
    if global_config.quiet {
        // In quiet mode, just print the workflow ID
        println!("{}", result.workflow_id);
        return Ok(());
    }

    println!();
    println!("{}", style("Execution Complete").cyan().bold());
    println!("{}", style("═".repeat(50)).dim());
    println!();

    // Status with color coding
    let status_display = match result.status.as_str() {
        "COMPLETED" => style(&result.status).green().bold(),
        "FAILED" => style(&result.status).red().bold(),
        "CANCELED" | "CANCELLED" => style(&result.status).yellow().bold(),
        "TERMINATED" => style(&result.status).yellow().bold(),
        "TIMED_OUT" => style(&result.status).yellow().bold(),
        _ => style(&result.status).white(),
    };
    println!("  {}  {}", style("Status:").dim(), status_display);

    // Execution details
    println!(
        "  {}  {}",
        style("Workflow ID:").dim(),
        style(&result.workflow_id).cyan()
    );
    if !result.execution_id.is_empty() {
        println!(
            "  {}  {}",
            style("Execution ID:").dim(),
            style(&result.execution_id).dim()
        );
    }

    // Timing information
    if let Some(started) = &result.started_at {
        println!("  {}  {}", style("Started:").dim(), started);
    }
    if let Some(completed) = &result.completed_at {
        println!("  {}  {}", style("Completed:").dim(), completed);
    }

    println!();

    // Result or error
    match result.status.as_str() {
        "COMPLETED" => {
            if let Some(ref result_value) = result.result {
                println!("{}", style("Result:").cyan().bold());
                println!();
                let formatted = serde_json::to_string_pretty(result_value)?;
                for line in formatted.lines() {
                    println!("  {}", line);
                }
                println!();
            }
        }
        "FAILED" => {
            if let Some(ref error) = result.error {
                println!("{}", style("Error:").red().bold());
                println!();
                println!("  {}", style(error).red());
                println!();
            }
        }
        "CANCELED" | "CANCELLED" => {
            println!("{}", style("Workflow was cancelled").yellow());
            println!();
        }
        "TERMINATED" => {
            println!("{}", style("Workflow was terminated").yellow());
            println!();
        }
        "TIMED_OUT" => {
            println!("{}", style("Workflow execution timed out").yellow());
            println!();
        }
        _ => {}
    }

    // Show command to view logs
    println!("{}", style("View details:").dim());
    println!(
        "  {} workflow get {}",
        style("orcher").cyan(),
        result.workflow_id
    );
    println!();

    Ok(())
}

/// Execution result structure
#[derive(Debug, Clone)]
struct ExecutionResult {
    execution_id: String,
    workflow_id: String,
    status: String,
    started_at: Option<String>,
    completed_at: Option<String>,
    result: Option<Value>,
    error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_parameters() {
        let params = vec![
            "order_id=123".to_string(),
            "amount=99.99".to_string(),
            "email=test@example.com".to_string(),
            "verified=true".to_string(),
        ];

        let result = parse_parameters(&params).unwrap();
        let parsed: Value = serde_json::from_slice(&result).unwrap();
        assert_eq!(parsed["order_id"], 123);
        assert_eq!(parsed["amount"], 99.99);
        assert_eq!(parsed["email"], "test@example.com");
        assert_eq!(parsed["verified"], true);
    }

    #[test]
    fn test_parse_parameters_json() {
        let params = vec![
            "data={\"foo\":\"bar\"}".to_string(),
            "items=[1,2,3]".to_string(),
        ];

        let result = parse_parameters(&params).unwrap();
        let parsed: Value = serde_json::from_slice(&result).unwrap();
        assert!(parsed["data"].is_object());
        assert!(parsed["items"].is_array());
    }

    #[test]
    fn test_parse_workflow_identifier() {
        let (workflow_type, task_queue) =
            parse_workflow_identifier("my-workflow@my-queue").unwrap();
        assert_eq!(workflow_type, "my-workflow");
        assert_eq!(task_queue, "my-queue");

        let (workflow_type, task_queue) = parse_workflow_identifier("my-workflow").unwrap();
        assert_eq!(workflow_type, "my-workflow");
        assert_eq!(task_queue, "my-workflow");
    }

    #[test]
    fn test_parse_parameters_invalid() {
        let params = vec!["invalid_format".to_string()];
        assert!(parse_parameters(&params).is_err());
    }

    #[test]
    fn test_is_terminal_status() {
        assert!(is_terminal_status("COMPLETED"));
        assert!(is_terminal_status("FAILED"));
        assert!(is_terminal_status("CANCELED"));
        assert!(is_terminal_status("CANCELLED"));
        assert!(is_terminal_status("TERMINATED"));
        assert!(is_terminal_status("TIMED_OUT"));
        assert!(!is_terminal_status("RUNNING"));
        assert!(!is_terminal_status("UNSPECIFIED"));
    }

    #[test]
    fn test_map_status_code() {
        assert_eq!(map_status_code(1), "RUNNING");
        assert_eq!(map_status_code(2), "COMPLETED");
        assert_eq!(map_status_code(3), "FAILED");
        assert_eq!(map_status_code(4), "CANCELED");
        assert_eq!(map_status_code(99), "UNKNOWN(99)");
    }
}
