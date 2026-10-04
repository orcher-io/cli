//! `orcher workflow` — list, get, cancel, terminate, event, and history.

use crate::client::grpc_client::{status_to_string, string_to_status, DEFAULT_GRPC_PORT};
use crate::config::Config;
use crate::error::{CliError, Result};
use crate::render;
use crate::time::Timestamptz;
use crate::utils::GlobalConfig;
use clap::{Args, Subcommand};
use console::style;
use std::time::Duration;
use tracing::info;

/// Workflow management command
#[derive(Args)]
#[command(after_help = "\
EXAMPLES:
  orcher workflow list                        List recent executions
  orcher workflow list -s running --limit 20  Filter by status
  orcher workflow list -o json                Machine-readable output
  orcher workflow get <workflow-id> --full    Full details incl. pending work
  orcher workflow cancel <workflow-id>        Request cancellation
  orcher workflow history <workflow-id>       Show the execution journal")]
pub struct WorkflowCommand {
    #[command(subcommand)]
    pub action: WorkflowCommands,
}

#[derive(Subcommand)]
pub enum WorkflowCommands {
    /// Start a workflow
    Start {
        /// The workflow type, as the worker registered it
        workflow_type: String,

        /// The task queue its worker polls
        #[arg(long = "task-queue", default_value = "default")]
        task_queue: String,

        /// The workflow id; the engine makes one up when it is omitted
        #[arg(long = "id")]
        workflow_id: Option<String>,

        /// The workflow's input, as JSON
        #[arg(short = 'i', long = "input")]
        input: Option<String>,

        /// Read the input JSON from a file, or from stdin with -
        #[arg(long = "input-file", conflicts_with = "input")]
        input_file: Option<std::path::PathBuf>,

        /// Wait for the workflow to finish and print its result
        #[arg(short = 'w', long = "wait")]
        wait: bool,

        /// With --wait, how long to wait: seconds, or a duration such as 5m
        #[arg(long, default_value = "5m", value_parser = parse_wait, requires = "wait")]
        timeout: Duration,
    },

    /// Wait for a workflow to finish and print its result
    Result {
        /// Workflow ID
        workflow_id: String,

        /// Specific execution/run ID
        #[arg(short = 'e', long = "execution-id")]
        execution_id: Option<String>,

        /// How long to wait: seconds, or a duration such as 5m
        #[arg(long, default_value = "60s", value_parser = parse_wait)]
        timeout: Duration,
    },

    /// Show the tasks a workflow ran: status, attempts and time taken
    Tasks {
        /// Workflow ID
        workflow_id: String,

        /// Specific execution/run ID
        #[arg(short = 'e', long = "execution-id")]
        execution_id: Option<String>,

        /// Maximum number of tasks to show
        #[arg(short = 'l', long = "limit", default_value = "100")]
        limit: i32,
    },

    /// List workflow executions
    #[command(aliases = ["ls"])]
    List {
        /// Filter by workflow type/name
        #[arg(short = 't', long = "type")]
        workflow_type: Option<String>,

        /// Filter by status (running, completed, failed, cancelled)
        #[arg(short = 's', long = "status")]
        status: Option<String>,

        /// Maximum number of results to return
        #[arg(short = 'l', long = "limit", default_value = "50")]
        limit: i32,

        /// Show all namespaces
        #[arg(short = 'A', long = "all-namespaces")]
        all_namespaces: bool,

        /// Advanced query filter (SQL-like syntax)
        #[arg(long = "query")]
        query: Option<String>,

        /// Show full IDs and absolute timestamps
        #[arg(long = "wide")]
        wide: bool,
    },

    /// Get detailed information about a workflow execution
    #[command(aliases = ["describe", "info"])]
    Get {
        /// Workflow execution ID or workflow_id
        workflow_id: String,

        /// Specific execution/run ID
        #[arg(short = 'e', long = "execution-id")]
        execution_id: Option<String>,

        /// Show full execution details including parameters
        #[arg(long = "full")]
        full: bool,
    },

    /// Cancel a running workflow execution
    Cancel {
        /// Workflow execution ID to cancel
        workflow_id: String,

        /// Specific execution/run ID
        #[arg(short = 'e', long = "execution-id")]
        execution_id: Option<String>,

        /// Reason for cancellation
        #[arg(short = 'r', long = "reason")]
        reason: Option<String>,

        /// Skip confirmation prompt
        #[arg(short = 'f', long = "force", visible_alias = "yes", short_alias = 'y')]
        force: bool,
    },

    /// View execution history/journal for a workflow
    #[command(aliases = ["journal", "events"])]
    History {
        /// Workflow execution ID
        workflow_id: String,

        /// Specific execution/run ID
        #[arg(short = 'e', long = "execution-id")]
        execution_id: Option<String>,

        /// Maximum number of events to show
        #[arg(short = 'l', long = "limit", default_value = "100")]
        limit: i32,

        /// Output in compact format
        #[arg(long = "compact")]
        compact: bool,
    },

    /// Terminate a workflow execution immediately
    Terminate {
        /// Workflow execution ID to terminate
        workflow_id: String,

        /// Specific execution/run ID
        #[arg(short = 'e', long = "execution-id")]
        execution_id: Option<String>,

        /// Reason for termination
        #[arg(short = 'r', long = "reason", default_value = "Terminated via CLI")]
        reason: String,

        /// Skip confirmation prompt
        #[arg(short = 'f', long = "force", visible_alias = "yes", short_alias = 'y')]
        force: bool,
    },

    /// Send an event to a running workflow
    Event {
        /// Workflow execution ID
        workflow_id: String,

        /// Event name to send. `-n` would clash with the global --namespace,
        /// so the name is positional (`--name` still works).
        #[arg(value_name = "EVENT", required_unless_present = "event_name_flag")]
        event_name: Option<String>,

        /// Event name to send (same as the positional EVENT)
        #[arg(long = "name", conflicts_with = "event_name", hide = true)]
        event_name_flag: Option<String>,

        /// Event payload as JSON string
        #[arg(short = 'p', long = "payload")]
        payload: Option<String>,
    },
}

/// Execute workflow management command
pub async fn execute(cmd: WorkflowCommand, global_config: &GlobalConfig) -> Result<()> {
    match cmd.action {
        WorkflowCommands::Start {
            workflow_type,
            task_queue,
            workflow_id,
            input,
            input_file,
            wait,
            timeout,
        } => {
            let input = match (input, input_file) {
                (Some(text), _) => Some(json_bytes(&text, "--input")?),
                (None, Some(path)) => Some(json_bytes(&read_input(&path)?, "--input-file")?),
                (None, None) => None,
            };
            start_workflow(
                &workflow_type,
                workflow_id.as_deref(),
                &task_queue,
                input,
                wait.then_some(timeout),
                global_config,
            )
            .await
        }
        WorkflowCommands::Result {
            workflow_id,
            execution_id,
            timeout,
        } => {
            let mut client = crate::client::connection::connect_grpc(global_config).await?;
            workflow_result(
                &mut client,
                &workflow_id,
                execution_id.as_deref(),
                timeout,
                global_config,
            )
            .await
        }
        WorkflowCommands::Tasks {
            workflow_id,
            execution_id,
            limit,
        } => {
            let mut client = crate::client::connection::connect_grpc(global_config).await?;
            let tasks = client
                .get_task_executions(&workflow_id, execution_id.as_deref(), limit)
                .await?
                .tasks;
            crate::commands::logs::display_task_executions(&tasks, global_config)
        }
        WorkflowCommands::List {
            workflow_type,
            status,
            limit,
            all_namespaces,
            query,
            wide,
        } => {
            list_workflows(
                workflow_type,
                status,
                limit,
                all_namespaces,
                query,
                wide,
                global_config,
            )
            .await
        }
        WorkflowCommands::Get {
            workflow_id,
            execution_id,
            full,
        } => get_workflow(workflow_id, execution_id, full, global_config).await,
        WorkflowCommands::Cancel {
            workflow_id,
            execution_id,
            reason,
            force,
        } => cancel_workflow(workflow_id, execution_id, reason, force, global_config).await,
        WorkflowCommands::History {
            workflow_id,
            execution_id,
            limit,
            compact,
        } => get_workflow_history(workflow_id, execution_id, limit, compact, global_config).await,
        WorkflowCommands::Terminate {
            workflow_id,
            execution_id,
            reason,
            force,
        } => terminate_workflow(workflow_id, execution_id, reason, force, global_config).await,
        WorkflowCommands::Event {
            workflow_id,
            event_name,
            event_name_flag,
            payload,
        } => {
            let event_name = event_name.or(event_name_flag).unwrap_or_default();
            send_event_to_workflow(workflow_id, event_name, payload, global_config).await
        }
    }
}

fn get_grpc_address(global_config: &GlobalConfig) -> Result<String> {
    if let Ok(config) = Config::load() {
        let context_name = global_config
            .profile
            .as_deref()
            .unwrap_or(&config.current_context);

        if let Some(ctx) = config.contexts.get(context_name) {
            // Parse the server URL and extract host, use gRPC port
            if let Ok(url) = url::Url::parse(&ctx.server) {
                let host = url.host_str().unwrap_or("localhost");
                return Ok(format!("http://{}:{}", host, DEFAULT_GRPC_PORT));
            }
        }
    }

    // Default to localhost
    Ok(format!("http://localhost:{}", DEFAULT_GRPC_PORT))
}

fn get_namespace(global_config: &GlobalConfig) -> String {
    global_config
        .namespace
        .clone()
        .unwrap_or_else(|| "default".to_string())
}

/// List workflow executions
async fn list_workflows(
    workflow_type: Option<String>,
    status: Option<String>,
    limit: i32,
    _all_namespaces: bool,
    query: Option<String>,
    wide: bool,
    global_config: &GlobalConfig,
) -> Result<()> {
    info!("Listing workflow executions");

    // Try to connect to gRPC server
    let mut client = crate::client::connection::connect_grpc(global_config).await?;

    // Convert status string to enum value
    // An unknown status is an error rather than silently no filter at all.
    let status_filter = match &status {
        Some(s) => Some(string_to_status(s).ok_or_else(|| {
            CliError::invalid_input(format!(
                "Unknown status '{}'. Use running, completed, failed, canceled, terminated, timed_out or pending",
                s
            ))
        })?),
        None => None,
    };

    // Use search if query is provided, otherwise list
    let (workflows, has_more) = if let Some(q) = query {
        let response = client.search_workflows(&q, limit, None).await?;
        (response.executions, !response.next_page_token.is_empty())
    } else {
        let response = client
            .list_workflows(workflow_type.as_deref(), status_filter, limit, None)
            .await?;
        (response.executions, !response.next_page_token.is_empty())
    };

    if workflows.is_empty() {
        println!("{}", style("No workflows found").yellow());
        if let Some(wt) = &workflow_type {
            println!("  Filter: type={}", wt);
        }
        if let Some(s) = &status {
            println!("  Filter: status={}", s);
        }
        return Ok(());
    }

    // Structured output (-o json/yaml) for scripting.
    let items = serde_json::Value::Array(
        workflows
            .iter()
            .map(|wf| {
                serde_json::json!({
                    "workflowId": wf.workflow_id,
                    "type": wf.workflow_type,
                    "status": status_to_string(wf.status),
                    "startedAt": wf.start_time.as_ref().map(|t| t.seconds),
                })
            })
            .collect(),
    );
    if let Some(out) = render::structured(&global_config.output_format, &items) {
        println!("{}", out?);
        return Ok(());
    }

    // `-o name` and quiet mode print bare IDs for piping.
    if global_config.output_format == "name" || global_config.quiet {
        for wf in &workflows {
            println!("{}", wf.workflow_id);
        }
        return Ok(());
    }

    // Table view. `--wide` (or `-o wide`) shows full IDs and absolute timestamps.
    let wide = wide || global_config.output_format == "wide";
    let rows: Vec<Vec<String>> = workflows
        .iter()
        .map(|wf| {
            let id = if wide {
                render::or_dash(&wf.workflow_id)
            } else {
                render::short_id(&wf.workflow_id)
            };
            let started = if wide {
                render::absolute_time(wf.start_time.as_ref().map(|t| t.seconds))
            } else {
                render::relative_time(wf.start_time.as_ref().map(|t| t.seconds))
            };
            vec![
                id,
                render::or_dash(&wf.workflow_type),
                render::status_cell(wf.status),
                started,
            ]
        })
        .collect();

    println!();
    println!(
        "{}",
        render::table(&["WORKFLOW ID", "TYPE", "STATUS", "STARTED"], rows)
    );
    println!();

    let count = format!(
        "{} workflow{}",
        workflows.len(),
        if workflows.len() == 1 { "" } else { "s" }
    );
    let hint = if wide {
        ""
    } else {
        "  ·  --wide for full IDs"
    };
    println!("  {}{}", style(count).dim(), style(hint).dim());
    if has_more {
        println!("  {}", style("more available — raise --limit").dim());
    }

    Ok(())
}

/// Get detailed workflow information
async fn get_workflow(
    workflow_id: String,
    execution_id: Option<String>,
    full: bool,
    global_config: &GlobalConfig,
) -> Result<()> {
    info!("Getting workflow details: {}", workflow_id);

    let mut client = crate::client::connection::connect_grpc(global_config).await?;

    // Get workflow status first
    let status_response = client
        .get_workflow_status(&workflow_id, execution_id.as_deref())
        .await?;

    // Describe by the execution the status names: engines up to at least
    // 0.5.5 match a bare workflow id against the workflow type instead, and
    // find nothing.
    let resolved_execution_id = status_response
        .execution
        .as_ref()
        .map(|e| e.execution_id.clone())
        .filter(|id| !id.is_empty())
        .or_else(|| execution_id.clone());

    // Structured output (-o json/yaml) for scripting.
    if global_config.is_structured_output() {
        let exec = status_response.execution.as_ref();
        let mut obj = serde_json::json!({
            "workflowId": exec.map(|e| e.workflow_id.as_str()).unwrap_or(&workflow_id),
            "executionId": exec.map(|e| e.execution_id.as_str()),
            "status": status_to_string(status_response.status),
            "startedAt": status_response.started_at.as_ref().map(|t| t.seconds),
            "completedAt": status_response.completed_at.as_ref().map(|t| t.seconds),
        });
        match &status_response.outcome {
            Some(orcher_proto::get_workflow_status_response::Outcome::Result(r)) => {
                obj["result"] = serde_json::from_slice(r).unwrap_or(serde_json::Value::Null);
            }
            Some(orcher_proto::get_workflow_status_response::Outcome::Error(e)) => {
                obj["error"] = serde_json::json!(e);
            }
            None => {}
        }
        if full {
            if let Ok(d) = client
                .describe_workflow(&workflow_id, resolved_execution_id.as_deref())
                .await
            {
                if let Some(info) = &d.execution_info {
                    obj["taskQueue"] = serde_json::json!(info.task_queue);
                    obj["namespace"] = serde_json::json!(info.namespace);
                    obj["attempt"] = serde_json::json!(info.attempt);
                    if !info.cron_schedule.is_empty() {
                        obj["cronSchedule"] = serde_json::json!(info.cron_schedule);
                    }
                }
                obj["pendingTasks"] = serde_json::json!(d.pending_tasks);
                obj["pendingTimers"] = serde_json::json!(d.pending_timers);
                obj["pendingEvents"] = serde_json::json!(d.pending_events);
            }
        }
        if let Some(out) = render::structured(&global_config.output_format, &obj) {
            println!("{}", out?);
            return Ok(());
        }
    }

    println!();
    println!("{}", style("Workflow Details").bold());
    println!("{}", style("-".repeat(50)).dim());

    // Basic info
    if let Some(exec) = &status_response.execution {
        println!("  Workflow ID:  {}", style(&exec.workflow_id).cyan());
        println!("  Execution ID: {}", style(&exec.execution_id).dim());
    } else {
        println!("  Workflow ID:  {}", style(&workflow_id).cyan());
    }

    let status_str = status_to_string(status_response.status);
    let status_styled = match status_str {
        "RUNNING" => style(status_str).cyan().bold(),
        "COMPLETED" => style(status_str).green().bold(),
        "FAILED" => style(status_str).red().bold(),
        _ => style(status_str).yellow().bold(),
    };
    println!("  Status:       {}", status_styled);

    if let Some(started) = &status_response.started_at {
        let dt = Timestamptz::from_second(started.seconds)
            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
            .unwrap_or_else(|| "-".to_string());
        println!("  Started:      {}", dt);
    }

    if let Some(completed) = &status_response.completed_at {
        let dt = Timestamptz::from_second(completed.seconds)
            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
            .unwrap_or_else(|| "-".to_string());
        println!("  Completed:    {}", dt);
    }

    // Show outcome if available
    if let Some(outcome) = &status_response.outcome {
        println!();
        match outcome {
            orcher_proto::get_workflow_status_response::Outcome::Result(result) => {
                println!("{}", style("Result:").bold());
                if let Ok(json) = serde_json::from_slice::<serde_json::Value>(result) {
                    println!(
                        "  {}",
                        serde_json::to_string_pretty(&json).unwrap_or_else(|_| "-".to_string())
                    );
                } else {
                    println!("  (binary data, {} bytes)", result.len());
                }
            }
            orcher_proto::get_workflow_status_response::Outcome::Error(error) => {
                println!("{}", style("Error:").red().bold());
                println!("  {}", error);
            }
        }
    }

    // If full details requested, get describe info
    if full {
        if let Ok(describe) = client
            .describe_workflow(&workflow_id, resolved_execution_id.as_deref())
            .await
        {
            println!();
            println!("{}", style("Execution Details").bold());
            println!("{}", style("-".repeat(50)).dim());

            if let Some(info) = &describe.execution_info {
                println!("  Task Queue:   {}", info.task_queue);
                println!("  Namespace:    {}", info.namespace);
                println!("  Attempt:      {}", info.attempt);

                if !info.cron_schedule.is_empty() {
                    println!("  Cron:         {}", info.cron_schedule);
                }
            }

            println!("  Pending Tasks:  {}", describe.pending_tasks);
            println!("  Pending Timers: {}", describe.pending_timers);
            println!("  Pending Events: {}", describe.pending_events);
        }
    }

    println!();
    Ok(())
}

/// Cancel a running workflow
async fn cancel_workflow(
    workflow_id: String,
    execution_id: Option<String>,
    reason: Option<String>,
    force: bool,
    global_config: &GlobalConfig,
) -> Result<()> {
    info!("Cancelling workflow: {}", workflow_id);

    // Confirm action unless forced
    if !force {
        let confirmed =
            super::common::confirm_action(&format!("Cancel workflow '{}'?", workflow_id), force)?;
        if !confirmed {
            println!("Cancellation aborted.");
            return Ok(());
        }
    }

    let mut client = crate::client::connection::connect_grpc(global_config).await?;

    let result = client
        .cancel_workflow(&workflow_id, execution_id.as_deref(), reason.as_deref())
        .await?;

    if result {
        println!(
            "{} Workflow '{}' cancellation requested",
            style("✓").green(),
            workflow_id
        );
    } else {
        println!(
            "{} Failed to cancel workflow '{}'",
            style("✗").red(),
            workflow_id
        );
    }

    Ok(())
}

/// Get workflow execution journal
async fn get_workflow_history(
    workflow_id: String,
    execution_id: Option<String>,
    limit: i32,
    compact: bool,
    global_config: &GlobalConfig,
) -> Result<()> {
    info!("Getting execution journal: {}", workflow_id);

    let mut client = crate::client::connection::connect_grpc(global_config).await?;

    let response = client
        .get_execution_journal(&workflow_id, execution_id.as_deref(), limit, None)
        .await?;

    let entries = response.journal;

    // Structured output (-o json/yaml) for scripting.
    let items = serde_json::Value::Array(
        entries
            .iter()
            .map(|entry| {
                serde_json::json!({
                    "id": entry.entry_id,
                    "type": entry_type_name(entry.entry_type),
                    "taskId": entry.task_id,
                    "timestamp": entry.timestamp.as_ref().map(|t| t.seconds),
                })
            })
            .collect(),
    );
    if let Some(out) = render::structured(&global_config.output_format, &items) {
        println!("{}", out?);
        return Ok(());
    }

    if entries.is_empty() {
        println!("{}", style("No history entries found").yellow());
        return Ok(());
    }

    println!();
    println!("{} ({})", style("Execution Journal").bold(), workflow_id);
    println!("{}", style("-".repeat(100)).dim());

    if compact {
        // Compact format
        println!(
            "{:<6}  {:<40}  {:<25}",
            style("ID").bold(),
            style("ENTRY TYPE").bold(),
            style("TIMESTAMP").bold()
        );
        println!("{}", style("-".repeat(75)).dim());

        for entry in &entries {
            let timestamp = entry
                .timestamp
                .as_ref()
                .map(|t| {
                    Timestamptz::from_second(t.seconds)
                        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                        .unwrap_or_else(|| "-".to_string())
                })
                .unwrap_or_else(|| "-".to_string());

            let entry_type = entry_type_name(entry.entry_type);
            println!(
                "{:<6}  {:<40}  {:<25}",
                entry.entry_id, entry_type, timestamp,
            );
        }
    } else {
        // Full format with details
        for entry in &entries {
            let timestamp = entry
                .timestamp
                .as_ref()
                .map(|t| {
                    Timestamptz::from_second(t.seconds)
                        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                        .unwrap_or_else(|| "-".to_string())
                })
                .unwrap_or_else(|| "-".to_string());

            let entry_type = entry_type_name(entry.entry_type);

            // Color-code entry types
            let entry_styled = if entry_type.contains("COMPLETED") {
                style(&entry_type).green()
            } else if entry_type.contains("FAILED") || entry_type.contains("TIMED_OUT") {
                style(&entry_type).red()
            } else if entry_type.contains("STARTED") {
                style(&entry_type).cyan()
            } else {
                style(&entry_type).white()
            };

            println!(
                "{} {} {}",
                style(format!("[{}]", entry.entry_id)).dim(),
                entry_styled,
                style(timestamp).dim(),
            );
        }
    }

    println!();
    println!("{} journal entries", entries.len());

    if !response.next_page_token.is_empty() {
        println!(
            "{}",
            style("More entries available. Use --limit to increase page size.").dim()
        );
    }

    Ok(())
}

/// Terminate a workflow immediately
async fn terminate_workflow(
    workflow_id: String,
    execution_id: Option<String>,
    reason: String,
    force: bool,
    global_config: &GlobalConfig,
) -> Result<()> {
    info!("Terminating workflow: {}", workflow_id);

    // Confirm action unless forced
    if !force {
        let confirmed = super::common::confirm_action(
            &format!(
                "Terminate workflow '{}'? This will immediately stop the workflow.",
                workflow_id
            ),
            force,
        )?;
        if !confirmed {
            println!("Termination aborted.");
            return Ok(());
        }
    }

    let mut client = crate::client::connection::connect_grpc(global_config).await?;

    let result = client
        .terminate_workflow(&workflow_id, execution_id.as_deref(), &reason)
        .await?;

    if result {
        println!(
            "{} Workflow '{}' terminated",
            style("✓").green(),
            workflow_id
        );
    } else {
        println!(
            "{} Failed to terminate workflow '{}'",
            style("✗").red(),
            workflow_id
        );
    }

    Ok(())
}

/// Send an event to a workflow
async fn send_event_to_workflow(
    workflow_id: String,
    event_name: String,
    payload: Option<String>,
    global_config: &GlobalConfig,
) -> Result<()> {
    info!(
        "Sending event '{}' to workflow: {}",
        event_name, workflow_id
    );

    // Parse payload if provided
    let payload_bytes = if let Some(p) = &payload {
        // Validate it's valid JSON
        let _: serde_json::Value = serde_json::from_str(p)
            .map_err(|e| CliError::invalid_input(format!("Invalid JSON payload: {}", e)))?;
        Some(p.as_bytes().to_vec())
    } else {
        None
    };

    let mut client = crate::client::connection::connect_grpc(global_config).await?;

    let result = client
        .send_event(&workflow_id, &event_name, payload_bytes)
        .await?;

    if result {
        println!(
            "{} Event '{}' sent to workflow '{}'",
            style("✓").green(),
            event_name,
            workflow_id
        );
    } else {
        println!(
            "{} Failed to send event to workflow '{}'",
            style("✗").red(),
            workflow_id
        );
    }

    Ok(())
}

/// Start a workflow, and with `wait`, wait for its result.
async fn start_workflow(
    workflow_type: &str,
    workflow_id: Option<&str>,
    task_queue: &str,
    input: Option<Vec<u8>>,
    wait: Option<Duration>,
    global_config: &GlobalConfig,
) -> Result<()> {
    let mut client = crate::client::connection::connect_grpc(global_config).await?;
    let started = client
        .start_workflow(workflow_type, workflow_id, task_queue, input)
        .await?;

    if let Some(timeout) = wait {
        if !global_config.quiet && !global_config.is_structured_output() {
            eprintln!(
                "Started {} (execution {}); waiting for it to finish...",
                started.workflow_id, started.execution_id
            );
        }
        return workflow_result(
            &mut client,
            &started.workflow_id,
            Some(&started.execution_id),
            timeout,
            global_config,
        )
        .await;
    }

    let value = serde_json::json!({
        "workflowId": started.workflow_id,
        "executionId": started.execution_id,
        "startedAt": started.started_at.as_ref().map(|t| t.seconds),
    });
    if let Some(out) = render::structured(&global_config.output_format, &value) {
        println!("{}", out?);
        return Ok(());
    }
    if global_config.output_format == "name" || global_config.quiet {
        println!("{}", started.workflow_id);
        return Ok(());
    }
    println!(
        "{} Started {} (execution {})",
        style("✓").green(),
        style(&started.workflow_id).cyan(),
        started.execution_id
    );
    println!(
        "{}",
        style(format!(
            "Wait for its result with: orcher workflow result {}",
            started.workflow_id
        ))
        .dim()
    );
    Ok(())
}

/// Wait for a workflow and print its result. A workflow that did not
/// complete is an error, so that the exit status says how it ended.
async fn workflow_result(
    client: &mut crate::client::grpc_client::OrcherGrpcClient,
    workflow_id: &str,
    execution_id: Option<&str>,
    timeout: Duration,
    global_config: &GlobalConfig,
) -> Result<()> {
    use orcher_proto::get_workflow_result_response::Outcome as ResultOutcome;

    let response = client
        .get_workflow_result(workflow_id, execution_id, timeout)
        .await?;
    let shown_id = response
        .execution
        .as_ref()
        .map(|e| e.workflow_id.clone())
        .filter(|id| !id.is_empty())
        .unwrap_or_else(|| workflow_id.to_string());
    let status = status_to_string(response.status).to_string();
    let (value, error) = match response.outcome {
        Some(ResultOutcome::Result(bytes)) => (payload(&bytes), None),
        Some(ResultOutcome::Error(e)) => (serde_json::Value::Null, Some(e)),
        None => (serde_json::Value::Null, None),
    };

    let summary = serde_json::json!({
        "workflowId": shown_id,
        "executionId": response.execution.as_ref().map(|e| e.execution_id.as_str()),
        "status": status,
        "result": value,
        "error": error,
    });
    if let Some(out) = render::structured(&global_config.output_format, &summary) {
        println!("{}", out?);
    } else if global_config.output_format == "name" {
        println!("{}", shown_id);
    } else if status == "COMPLETED" {
        // The result alone, so that it can be piped.
        println!("{}", serde_json::to_string_pretty(&value)?);
    }

    if status == "COMPLETED" {
        Ok(())
    } else {
        let detail = error
            .filter(|e| !e.is_empty())
            .map(|e| format!(": {}", e))
            .unwrap_or_default();
        Err(CliError::WorkflowFailed {
            workflow_id: shown_id,
            error_message: format!("ended {}{}", status, detail),
        })
    }
}

/// "TASK_COMPLETED" for a journal entry of type `ENTRY_TYPE_TASK_COMPLETED`.
pub(crate) fn entry_type_name(entry_type: i32) -> String {
    match orcher_proto::EntryType::try_from(entry_type) {
        Ok(t) => t
            .as_str_name()
            .trim_start_matches("ENTRY_TYPE_")
            .to_string(),
        Err(_) => format!("ENTRY_TYPE_{}", entry_type),
    }
}

/// Bytes the engine stored as a payload: JSON when they are JSON, otherwise
/// text, otherwise a note of their size.
fn payload(bytes: &[u8]) -> serde_json::Value {
    if bytes.is_empty() {
        return serde_json::Value::Null;
    }
    serde_json::from_slice(bytes).unwrap_or_else(|_| match std::str::from_utf8(bytes) {
        Ok(text) => serde_json::Value::String(text.to_string()),
        Err(_) => serde_json::Value::String(format!("<{} bytes of binary data>", bytes.len())),
    })
}

/// Check that `text` is JSON and return it as bytes, unchanged.
fn json_bytes(text: &str, flag: &str) -> Result<Vec<u8>> {
    serde_json::from_str::<serde_json::Value>(text)
        .map_err(|e| CliError::invalid_input(format!("{} is not valid JSON: {}", flag, e)))?;
    Ok(text.as_bytes().to_vec())
}

/// Read input from a file, or from stdin when the path is `-`.
fn read_input(path: &std::path::Path) -> Result<String> {
    if path.as_os_str() == "-" {
        let mut text = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut text)?;
        Ok(text)
    } else {
        std::fs::read_to_string(path).map_err(|e| {
            CliError::io_with_path("Failed to read input file", path.display().to_string(), e)
        })
    }
}

/// Seconds (`90`), or a duration such as `30s`, `5m` or `1h`.
fn parse_wait(value: &str) -> std::result::Result<Duration, String> {
    let invalid = || format!("'{}' is not a duration such as 90, 30s, 5m or 1h", value);
    let split = value
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(value.len());
    let (number, unit) = value.split_at(split);
    let number: u64 = number.parse().map_err(|_| invalid())?;
    let seconds = match unit {
        "" | "s" => number,
        "m" => number * 60,
        "h" => number * 3600,
        _ => return Err(invalid()),
    };
    Ok(Duration::from_secs(seconds))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waits_take_seconds_or_durations() {
        assert_eq!(parse_wait("90"), Ok(Duration::from_secs(90)));
        assert_eq!(parse_wait("30s"), Ok(Duration::from_secs(30)));
        assert_eq!(parse_wait("5m"), Ok(Duration::from_secs(300)));
        assert_eq!(parse_wait("1h"), Ok(Duration::from_secs(3600)));
        assert!(parse_wait("5x").is_err());
        assert!(parse_wait("m").is_err());
    }

    #[test]
    fn journal_entry_names_come_from_the_protocol() {
        use orcher_proto::EntryType;
        assert_eq!(
            entry_type_name(EntryType::TaskCompleted as i32),
            "TASK_COMPLETED"
        );
        assert_eq!(
            entry_type_name(EntryType::WorkflowExecutionCancelRequested as i32),
            "WORKFLOW_EXECUTION_CANCEL_REQUESTED"
        );
        assert_eq!(entry_type_name(9999), "ENTRY_TYPE_9999");
    }

    #[test]
    fn inputs_must_be_json() {
        assert_eq!(
            json_bytes(r#"{"ok":true}"#, "--input").unwrap(),
            br#"{"ok":true}"#
        );
        assert!(json_bytes("not json", "--input").is_err());
    }

    #[test]
    fn payloads_render_as_json_text_or_size() {
        assert_eq!(payload(br#"{"a":1}"#), serde_json::json!({"a": 1}));
        assert_eq!(payload(b"plain"), serde_json::json!("plain"));
        assert_eq!(
            payload(&[0xff, 0xfe]),
            serde_json::json!("<2 bytes of binary data>")
        );
        assert_eq!(payload(b""), serde_json::Value::Null);
    }

    #[test]
    fn test_get_grpc_address_default() {
        let global_config = GlobalConfig::default();
        let address = get_grpc_address(&global_config).unwrap();
        assert_eq!(address, "http://localhost:50051");
    }

    #[test]
    fn test_get_namespace_default() {
        let global_config = GlobalConfig::default();
        let ns = get_namespace(&global_config);
        assert_eq!(ns, "default");
    }
}
