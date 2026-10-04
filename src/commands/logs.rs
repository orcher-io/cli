//! `orcher logs` — view workflow logs (text, journal, or task executions).
//! gRPC for one-shot retrieval; HTTP/SSE for `--follow` streaming.

use crate::client::connection::{ConnectionManager, ServerType};
use crate::client::grpc_client::OrcherGrpcClient;
use crate::error::{CliError, Result};
use crate::render;
use crate::time::Timestamptz;
use crate::utils::GlobalConfig;
use console::style;
use futures::StreamExt;
use reqwest_eventsource::{Event as SseEvent, EventSource};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::io::{self, Write};
use std::time::Duration;
use tracing::{debug, info, warn};

/// Type of logs to retrieve
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LogType {
    /// Human-readable text logs from workflow_execution_logs table (default)
    #[default]
    Text,
    /// Event-sourced journal from workflow_execution_journal table
    Journal,
    /// Task execution details from task_executions table
    Tasks,
}

/// Configuration for logs operation
#[derive(Debug, Clone)]
pub struct LogsConfig {
    pub follow: bool,
    pub tail: i32,
    pub since: Option<String>,
    pub until: Option<String>,
    pub grep: Option<String>,
    pub timestamps: bool,
    pub colors: bool,
    pub poll_interval: Duration,
    pub level: Option<String>,
    pub step: Option<String>,
}

impl Default for LogsConfig {
    fn default() -> Self {
        Self {
            follow: false,
            tail: 100,
            since: None,
            until: None,
            grep: None,
            timestamps: true,
            colors: true,
            poll_interval: Duration::from_secs(2),
            level: None,
            step: None,
        }
    }
}

/// Log entry structure (matches server's LogEntry model)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub timestamp: Timestamptz,
    pub level: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step: Option<String>,
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub metadata: std::collections::HashMap<String, String>,
}

/// Log stream for following logs
#[derive(Debug)]
#[allow(dead_code)]
pub struct LogStream {
    entries: VecDeque<LogEntry>,
    last_timestamp: Option<Timestamptz>,
    resource_type: String,
    resource_name: String,
}

impl LogStream {
    pub fn new(resource_type: String, resource_name: String) -> Self {
        Self {
            entries: VecDeque::new(),
            last_timestamp: None,
            resource_type,
            resource_name,
        }
    }

    pub fn add_entries(&mut self, mut entries: Vec<LogEntry>) {
        // Sort entries by timestamp
        entries.sort_by_key(|e| e.timestamp);

        for entry in entries {
            // Update last timestamp
            if self.last_timestamp.is_none() || entry.timestamp > self.last_timestamp.unwrap() {
                self.last_timestamp = Some(entry.timestamp);
            }

            self.entries.push_back(entry);
        }
    }

    pub fn get_entries(&self) -> &VecDeque<LogEntry> {
        &self.entries
    }

    pub fn last_timestamp(&self) -> Option<Timestamptz> {
        self.last_timestamp
    }
}

/// SSE stream complete event
#[derive(Debug, Deserialize)]
struct StreamCompleteEvent {
    #[serde(rename = "type")]
    event_type: String,
    execution_id: String,
}

/// Execute the logs command
pub async fn execute(
    resource: String,
    follow: bool,
    since: Option<String>,
    tail: i32,
    journal: bool,
    tasks: bool,
    global_config: &GlobalConfig,
) -> Result<()> {
    // Determine log type from flags
    let log_type = if journal {
        LogType::Journal
    } else if tasks {
        LogType::Tasks
    } else {
        LogType::Text // Default to human-readable text logs
    };

    let logs_config = LogsConfig {
        follow,
        tail,
        since,
        colors: global_config.use_colors(),
        ..Default::default()
    };

    let log_type_str = match log_type {
        LogType::Text => "text logs",
        LogType::Journal => "event journal",
        LogType::Tasks => "task executions",
    };

    info!(
        "Getting {} for resource '{}' (follow: {}, tail: {})",
        log_type_str, resource, logs_config.follow, logs_config.tail
    );

    // Parse resource identifier
    let (resource_type, resource_name, _namespace) = parse_resource_identifier(&resource)?;

    // Detect best server
    let manager = ConnectionManager::from_config(global_config);
    let server_type = manager.detect_best_server().await?;

    if logs_config.follow {
        // Following reads text logs only.
        if log_type != LogType::Text {
            return Err(CliError::invalid_input(
                "Follow mode (--follow) is only supported for text logs. Remove --journal or --tasks flag.",
            ));
        }
        // A gateway streams the log over SSE; an engine on its own is polled
        // over gRPC for lines after the last one seen.
        if manager.configured_http_addr().is_some() || server_type == ServerType::Http {
            follow_logs_sse(
                &manager,
                &resource_type,
                &resource_name,
                &logs_config,
                global_config,
            )
            .await
        } else {
            follow_logs_grpc(&manager, &resource_name, &logs_config, global_config).await
        }
    } else {
        // One-time log retrieval
        match server_type {
            ServerType::Grpc => {
                get_logs_grpc(
                    &manager,
                    &resource_type,
                    &resource_name,
                    &logs_config,
                    log_type,
                    global_config,
                )
                .await
            }
            ServerType::Http => {
                // Through a gateway only the text log is available
                if log_type != LogType::Text {
                    return Err(CliError::invalid_input(
                        "HTTP mode only supports text logs. Use gRPC for --journal or --tasks.",
                    ));
                }
                get_logs_http(
                    &manager,
                    &resource_type,
                    &resource_name,
                    &logs_config,
                    global_config,
                )
                .await
            }
        }
    }
}

async fn get_logs_grpc(
    manager: &ConnectionManager,
    resource_type: &str,
    resource_name: &str,
    logs_config: &LogsConfig,
    log_type: LogType,
    global_config: &GlobalConfig,
) -> Result<()> {
    let log_type_str = match log_type {
        LogType::Text => "text logs",
        LogType::Journal => "event journal",
        LogType::Tasks => "task executions",
    };
    debug!(
        "Getting {} via gRPC for {} '{}'",
        log_type_str, resource_type, resource_name
    );

    let client = manager.get_grpc_client().await?;
    let mut client = (*client).clone();

    match log_type {
        LogType::Tasks => {
            // Fetch task executions
            match resource_type.to_lowercase().as_str() {
                "workflow" | "workflows" | "wf" => {
                    fetch_task_executions_grpc(
                        &mut client,
                        resource_name,
                        logs_config,
                        global_config,
                    )
                    .await
                }
                _ => Err(CliError::invalid_input(format!(
                    "Task executions not available for resource type: {}. Use: workflow/<id>",
                    resource_type
                ))),
            }
        }
        LogType::Journal | LogType::Text => {
            // Fetch logs (journal or text)
            let logs = match resource_type.to_lowercase().as_str() {
                "workflow" | "workflows" | "wf" => {
                    fetch_workflow_logs_grpc(&mut client, resource_name, logs_config, log_type)
                        .await?
                }
                "execution" | "executions" | "exec" => {
                    fetch_execution_logs_grpc(&mut client, resource_name, logs_config, log_type)
                        .await?
                }
                _ => {
                    return Err(CliError::invalid_input(format!(
                        "Logs not available for resource type: {}. Supported types: workflow, execution",
                        resource_type
                    )));
                }
            };

            if logs.is_empty() {
                if !global_config.quiet {
                    println!(
                        "{}  No {} found for {} '{}'",
                        style("•").yellow(),
                        log_type_str,
                        resource_type,
                        resource_name
                    );
                }
                return Ok(());
            }

            // Filter logs if needed
            let filtered_logs = filter_logs(logs, logs_config)?;

            // Display logs
            display_logs(&filtered_logs, logs_config, global_config)?;

            if !global_config.quiet {
                println!(
                    "\n{}  Retrieved {} {} entries for {} '{}'",
                    style("✓").green(),
                    filtered_logs.len(),
                    log_type_str,
                    resource_type,
                    resource_name
                );
            }

            Ok(())
        }
    }
}

async fn fetch_workflow_logs_grpc(
    client: &mut OrcherGrpcClient,
    workflow_id: &str,
    logs_config: &LogsConfig,
    log_type: LogType,
) -> Result<Vec<LogEntry>> {
    match log_type {
        LogType::Text => {
            // Fetch human-readable text logs from workflow_execution_logs table
            let response = client
                .get_execution_logs(
                    workflow_id,
                    None,
                    logs_config.tail,
                    logs_config.level.as_deref(),
                    logs_config.step.as_deref(),
                )
                .await?;

            // The engine does not return lines in order; their ids are.
            let mut lines = response.logs;
            lines.sort_by_key(|l| l.id);
            let logs: Vec<LogEntry> = lines.iter().map(execution_log_to_log_entry).collect();

            Ok(logs)
        }
        LogType::Journal => {
            // Fetch event journal from workflow_execution_journal table
            let response = client
                .get_execution_journal(workflow_id, None, logs_config.tail, None)
                .await?;

            // Convert journal entries to log entries
            let logs: Vec<LogEntry> = response
                .journal
                .into_iter()
                .map(|entry| journal_entry_to_log(&entry))
                .collect();

            Ok(logs)
        }
        LogType::Tasks => Err(CliError::internal(
            "task logs are routed to fetch_task_executions_grpc; reaching here is a bug",
        )),
    }
}

async fn fetch_execution_logs_grpc(
    client: &mut OrcherGrpcClient,
    execution_id: &str,
    logs_config: &LogsConfig,
    log_type: LogType,
) -> Result<Vec<LogEntry>> {
    match log_type {
        LogType::Text => {
            // Fetch human-readable text logs
            let response = client
                .get_execution_logs(
                    execution_id,
                    None,
                    logs_config.tail,
                    logs_config.level.as_deref(),
                    logs_config.step.as_deref(),
                )
                .await?;

            // The engine does not return lines in order; their ids are.
            let mut lines = response.logs;
            lines.sort_by_key(|l| l.id);
            let logs: Vec<LogEntry> = lines.iter().map(execution_log_to_log_entry).collect();

            Ok(logs)
        }
        LogType::Journal => {
            // Fetch event journal
            let response = client
                .get_execution_journal(execution_id, None, logs_config.tail, None)
                .await?;

            let logs: Vec<LogEntry> = response
                .journal
                .into_iter()
                .map(|entry| journal_entry_to_log(&entry))
                .collect();

            Ok(logs)
        }
        LogType::Tasks => Err(CliError::internal(
            "task logs are routed to fetch_task_executions_grpc; reaching here is a bug",
        )),
    }
}

async fn fetch_task_executions_grpc(
    client: &mut OrcherGrpcClient,
    workflow_id: &str,
    logs_config: &LogsConfig,
    global_config: &GlobalConfig,
) -> Result<()> {
    let response = client
        .get_task_executions(workflow_id, None, logs_config.tail)
        .await?;

    if response.tasks.is_empty() {
        if !global_config.quiet {
            println!(
                "{}  No task executions found for workflow '{}'",
                style("•").yellow(),
                workflow_id
            );
        }
        return Ok(());
    }

    // Display task executions in a table format
    display_task_executions(&response.tasks, global_config)?;

    if !global_config.quiet && !global_config.is_structured_output() {
        println!(
            "\n{}  Retrieved {} task executions for workflow '{}'",
            style("✓").green(),
            response.tasks.len(),
            workflow_id
        );
    }

    Ok(())
}

pub(crate) fn display_task_executions(
    tasks: &[orcher_proto::TaskExecutionInfo],
    global_config: &GlobalConfig,
) -> Result<()> {
    // Structured output (-o json/yaml) for scripting.
    let items = serde_json::Value::Array(
        tasks
            .iter()
            .map(|task| {
                serde_json::json!({
                    "taskId": task.task_id,
                    "type": task.task_type,
                    "status": task.status,
                    "attempt": task.attempt,
                    "startedAt": task.started_at.as_ref().map(|t| t.seconds),
                    "completedAt": task.completed_at.as_ref().map(|t| t.seconds),
                    "error": task.error_message,
                })
            })
            .collect(),
    );
    if let Some(out) = render::structured(&global_config.output_format, &items) {
        println!("{}", out?);
        return Ok(());
    }

    let rows: Vec<Vec<String>> = tasks
        .iter()
        .map(|task| {
            let duration = match (task.started_at.as_ref(), task.completed_at.as_ref()) {
                (Some(start), Some(end)) => {
                    let start_ms = start.seconds * 1000 + (start.nanos / 1_000_000) as i64;
                    let end_ms = end.seconds * 1000 + (end.nanos / 1_000_000) as i64;
                    let duration_ms = end_ms - start_ms;
                    if duration_ms < 1000 {
                        format!("{}ms", duration_ms)
                    } else if duration_ms < 60000 {
                        format!("{:.1}s", duration_ms as f64 / 1000.0)
                    } else {
                        format!("{:.1}m", duration_ms as f64 / 60000.0)
                    }
                }
                _ => "—".to_string(),
            };
            vec![
                render::short_id(&task.task_id),
                render::or_dash(&task.task_type),
                render::status_cell_str(&task.status),
                task.attempt.to_string(),
                render::relative_time(task.started_at.as_ref().map(|t| t.seconds)),
                duration,
            ]
        })
        .collect();

    println!(
        "{}",
        render::table(
            &["TASK ID", "TYPE", "STATUS", "ATTEMPT", "STARTED", "DURATION"],
            rows
        )
    );

    // Error messages for failed tasks, listed under the table.
    for task in tasks {
        if !task.error_message.is_empty() {
            println!(
                "  {} {} {}",
                style(render::short_id(&task.task_id)).dim(),
                style("└─").red(),
                style(&task.error_message).dim()
            );
        }
    }

    Ok(())
}

/// Convert a gRPC ExecutionLogEntry to a LogEntry
fn execution_log_to_log_entry(entry: &orcher_proto::ExecutionLogEntry) -> LogEntry {
    let timestamp = entry
        .timestamp
        .as_ref()
        .map(|ts| Timestamptz::from_second(ts.seconds).unwrap_or_else(Timestamptz::now))
        .unwrap_or_else(Timestamptz::now);

    // Parse metadata from JSON string
    let metadata: std::collections::HashMap<String, String> = if entry.metadata.is_empty() {
        std::collections::HashMap::new()
    } else {
        serde_json::from_str(&entry.metadata).unwrap_or_default()
    };

    LogEntry {
        timestamp,
        level: entry.level.clone(),
        message: entry.message.clone(),
        step: if entry.step.is_empty() {
            None
        } else {
            Some(entry.step.clone())
        },
        source: entry.source.clone(),
        metadata,
    }
}

/// Convert a gRPC JournalEntry to a LogEntry, naming the entry by its type in
/// the protocol (an earlier hand-written table numbered several types wrong).
fn journal_entry_to_log(entry: &orcher_proto::JournalEntry) -> LogEntry {
    let timestamp = entry
        .timestamp
        .as_ref()
        .map(|ts| Timestamptz::from_second(ts.seconds).unwrap_or_else(Timestamptz::now))
        .unwrap_or_else(Timestamptz::now);

    let name = crate::commands::workflow::entry_type_name(entry.entry_type);
    let level = if name.ends_with("FAILED") || name.ends_with("TIMED_OUT") {
        "ERROR"
    } else if name.contains("CANCEL") || name.ends_with("TERMINATED") {
        "WARN"
    } else if name.ends_with("SCHEDULED") || name == "TIMER_STARTED" {
        "DEBUG"
    } else {
        "INFO"
    };
    let mut message = name.replace('_', " ").to_lowercase();
    if let Some(first) = message.get(..1) {
        message = first.to_uppercase() + &message[1..];
    }
    let step = if entry.task_id == 0 {
        None
    } else if name.starts_with("TASK_") {
        Some(format!("task-{}", entry.task_id))
    } else if name.contains("STEP_") {
        Some(format!("step-{}", entry.task_id))
    } else {
        None
    };

    LogEntry {
        timestamp,
        level: level.to_string(),
        message,
        step,
        source: "workflow-engine".to_string(),
        metadata: std::collections::HashMap::new(),
    }
}

/// Get logs via HTTP (fallback)
async fn get_logs_http(
    manager: &ConnectionManager,
    resource_type: &str,
    resource_name: &str,
    logs_config: &LogsConfig,
    global_config: &GlobalConfig,
) -> Result<()> {
    debug!(
        "Getting logs via HTTP for {} '{}'",
        resource_type, resource_name
    );

    let client = reqwest::Client::new();
    let base_url = manager.http_addr();

    let url = match resource_type.to_lowercase().as_str() {
        "workflow" | "workflows" | "wf" => {
            format!("{}/api/v1/workflows/{}/logs", base_url, resource_name)
        }
        "execution" | "executions" | "exec" => {
            format!("{}/api/v1/executions/{}/logs", base_url, resource_name)
        }
        _ => {
            return Err(CliError::invalid_input(format!(
                "Logs not available for resource type: {}. Supported types: workflow, execution",
                resource_type
            )));
        }
    };

    let mut request = crate::client::with_gateway_auth(client.get(&url), global_config);

    // Add query parameters
    if logs_config.tail > 0 {
        request = request.query(&[("limit", logs_config.tail.to_string())]);
    }

    if let Some(level) = &logs_config.level {
        request = request.query(&[("level", level.as_str())]);
    }

    if let Some(step) = &logs_config.step {
        request = request.query(&[("step", step.as_str())]);
    }

    let response = request.send().await.map_err(|e| CliError::Network {
        message: format!("Failed to fetch logs: {}", e),
        source: None,
    })?;

    if !response.status().is_success() {
        return Err(CliError::Api {
            status: response.status().as_u16(),
            message: format!("Failed to fetch logs: {}", response.status()),
            endpoint: Some(url),
        });
    }

    let logs: Vec<LogEntry> = response.json().await.map_err(|e| CliError::Network {
        message: format!("Failed to parse logs response: {}", e),
        source: None,
    })?;

    if logs.is_empty() {
        if !global_config.quiet {
            println!(
                "{}  No logs found for {} '{}'",
                style("•").yellow(),
                resource_type,
                resource_name
            );
        }
        return Ok(());
    }

    // Filter logs if needed
    let filtered_logs = filter_logs(logs, logs_config)?;

    // Display logs
    display_logs(&filtered_logs, logs_config, global_config)?;

    if !global_config.quiet {
        println!(
            "\n{}  Retrieved {} log entries for {} '{}'",
            style("✓").green(),
            filtered_logs.len(),
            resource_type,
            resource_name
        );
    }

    Ok(())
}

/// How often `--follow` asks an engine for new lines.
const FOLLOW_POLL_INTERVAL: Duration = Duration::from_secs(1);

/// Follow a workflow's log over gRPC: print what is there, then ask for lines
/// after the last one seen until the workflow has ended and nothing more
/// arrives.
async fn follow_logs_grpc(
    manager: &ConnectionManager,
    workflow_id: &str,
    logs_config: &LogsConfig,
    global_config: &GlobalConfig,
) -> Result<()> {
    let client = manager.get_grpc_client().await?;
    let mut client = (*client).clone();

    if !global_config.quiet && !global_config.is_structured_output() {
        eprintln!(
            "{}  Following logs for workflow '{}' until it ends (press Ctrl+C to stop)",
            style("→").cyan(),
            workflow_id
        );
    }

    let mut last_id = 0;
    let mut first = true;
    loop {
        let ended = workflow_has_ended(&mut client, workflow_id).await?;
        let mut lines = client
            .get_execution_logs_after(
                workflow_id,
                last_id,
                if first { logs_config.tail } else { 500 },
                logs_config.level.as_deref(),
                logs_config.step.as_deref(),
            )
            .await?
            .logs;
        lines.sort_by_key(|l| l.id);
        for line in &lines {
            last_id = last_id.max(line.id);
            let entry = execution_log_to_log_entry(line);
            display_log_entry(&entry, logs_config, global_config)?;
        }
        first = false;
        if ended && lines.is_empty() {
            return Ok(());
        }
        tokio::time::sleep(FOLLOW_POLL_INTERVAL).await;
    }
}

/// Whether the workflow has reached a final status.
async fn workflow_has_ended(client: &mut OrcherGrpcClient, workflow_id: &str) -> Result<bool> {
    let status = client.get_workflow_status(workflow_id, None).await?.status;
    // 1 is RUNNING and 9 PENDING; anything else is final.
    Ok(!matches!(status, 1 | 9))
}

/// Follow logs via SSE streaming
async fn follow_logs_sse(
    manager: &ConnectionManager,
    resource_type: &str,
    resource_name: &str,
    logs_config: &LogsConfig,
    global_config: &GlobalConfig,
) -> Result<()> {
    debug!(
        "Following logs via SSE for {} '{}'",
        resource_type, resource_name
    );

    let base_url = manager.http_addr();

    // Build stream URL based on resource type
    let mut url = match resource_type.to_lowercase().as_str() {
        "execution" | "executions" | "exec" => {
            format!(
                "{}/api/v1/executions/{}/logs/stream",
                base_url, resource_name
            )
        }
        "workflow" | "workflows" | "wf" => {
            // For workflows, we need the execution ID - try to get the latest execution
            format!(
                "{}/api/v1/executions/{}/logs/stream",
                base_url, resource_name
            )
        }
        _ => {
            return Err(CliError::invalid_input(format!(
                "Log streaming not available for resource type: {}. Supported types: execution, workflow",
                resource_type
            )));
        }
    };

    // Add query parameters for filtering
    let mut query_params = vec![];
    if let Some(level) = &logs_config.level {
        query_params.push(format!("level={}", level));
    }
    if let Some(step) = &logs_config.step {
        query_params.push(format!("step={}", step));
    }
    if logs_config.tail > 0 {
        query_params.push(format!("limit={}", logs_config.tail));
    }
    if !query_params.is_empty() {
        url = format!("{}?{}", url, query_params.join("&"));
    }

    if !global_config.quiet {
        println!(
            "{}  Following logs for {} '{}' (press Ctrl+C to stop)",
            style("→").cyan(),
            resource_type,
            resource_name
        );
        println!("{}", "─".repeat(60));
    }

    // Create SSE client
    let client = reqwest::Client::new();
    let request = crate::client::with_gateway_auth(client.get(&url), global_config);
    let mut event_source = EventSource::new(request).map_err(|e| CliError::Network {
        message: format!("Failed to create SSE connection: {}", e),
        source: None,
    })?;

    // Set up signal handler for graceful shutdown
    let (shutdown_tx, mut shutdown_rx) = tokio::sync::mpsc::channel::<()>(1);

    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        shutdown_tx.send(()).await.ok();
    });

    // Process SSE events
    loop {
        tokio::select! {
            event = event_source.next() => {
                match event {
                    Some(Ok(SseEvent::Open)) => {
                        debug!("SSE connection opened");
                    }
                    Some(Ok(SseEvent::Message(msg))) => {
                        // Parse log entry
                        match serde_json::from_str::<LogEntry>(&msg.data) {
                            Ok(log) => {
                                // Check for stream complete event
                                if let Ok(complete) = serde_json::from_str::<StreamCompleteEvent>(&msg.data) {
                                    if complete.event_type == "stream_complete" {
                                        if !global_config.quiet {
                                            println!(
                                                "\n{}  Log stream completed for execution '{}'",
                                                style("✓").green(),
                                                complete.execution_id
                                            );
                                        }
                                        break;
                                    }
                                }

                                // Apply grep filter if specified
                                if let Some(pattern) = &logs_config.grep {
                                    if !log.message.contains(pattern) && !log.level.contains(pattern) {
                                        continue;
                                    }
                                }

                                // Display log entry
                                display_log_entry(&log, logs_config, global_config)?;
                            }
                            Err(e) => {
                                // Try parsing as stream complete event
                                if let Ok(complete) = serde_json::from_str::<StreamCompleteEvent>(&msg.data) {
                                    if complete.event_type == "stream_complete" {
                                        if !global_config.quiet {
                                            println!(
                                                "\n{}  Log stream completed for execution '{}'",
                                                style("✓").green(),
                                                complete.execution_id
                                            );
                                        }
                                        break;
                                    }
                                }
                                debug!("Failed to parse SSE message: {} - data: {}", e, msg.data);
                            }
                        }
                    }
                    Some(Err(e)) => {
                        warn!("SSE error: {}", e);
                        // Continue on transient errors
                    }
                    None => {
                        // Stream ended
                        break;
                    }
                }
            }
            _ = shutdown_rx.recv() => {
                // Received shutdown signal
                if !global_config.quiet {
                    println!("\n{}  Log following stopped", style("•").yellow());
                }
                break;
            }
        }
    }

    Ok(())
}

/// Display a single log entry
fn display_log_entry(
    entry: &LogEntry,
    logs_config: &LogsConfig,
    global_config: &GlobalConfig,
) -> Result<()> {
    let line = format_log_entry(entry, logs_config)?;

    if logs_config.colors && global_config.use_colors() {
        println!("{}", colorize_log_line(&line, &entry.level));
    } else {
        println!("{}", line);
    }

    Ok(())
}

/// Filter logs based on criteria
fn filter_logs(logs: Vec<LogEntry>, logs_config: &LogsConfig) -> Result<Vec<LogEntry>> {
    let mut filtered = logs;

    // Filter by time range
    if let Some(since_str) = &logs_config.since {
        let since_time = parse_time_filter(since_str)?;
        filtered.retain(|entry| entry.timestamp >= since_time);
    }

    if let Some(until_str) = &logs_config.until {
        let until_time = parse_time_filter(until_str)?;
        filtered.retain(|entry| entry.timestamp <= until_time);
    }

    // Filter by log level
    if let Some(level) = &logs_config.level {
        let level_upper = level.to_uppercase();
        filtered.retain(|entry| entry.level.to_uppercase() == level_upper);
    }

    // Filter by step
    if let Some(step_filter) = &logs_config.step {
        filtered.retain(|entry| {
            entry
                .step
                .as_ref()
                .map(|s| s.contains(step_filter))
                .unwrap_or(false)
        });
    }

    // Filter by grep pattern
    if let Some(pattern) = &logs_config.grep {
        let regex = regex::Regex::new(pattern)
            .map_err(|e| CliError::invalid_input(format!("Invalid grep pattern: {}", e)))?;

        filtered.retain(|entry| {
            regex.is_match(&entry.message)
                || regex.is_match(&entry.level)
                || regex.is_match(&entry.source)
                || entry
                    .step
                    .as_ref()
                    .map(|s| regex.is_match(s))
                    .unwrap_or(false)
        });
    }

    // Apply tail limit
    if logs_config.tail > 0 && filtered.len() > logs_config.tail as usize {
        let start_idx = filtered.len() - logs_config.tail as usize;
        filtered = filtered[start_idx..].to_vec();
    }

    Ok(filtered)
}

/// Display logs to stdout
fn display_logs(
    logs: &[LogEntry],
    logs_config: &LogsConfig,
    global_config: &GlobalConfig,
) -> Result<()> {
    let stdout = io::stdout();
    let mut handle = stdout.lock();

    for entry in logs {
        let line = format_log_entry(entry, logs_config)?;

        if logs_config.colors && global_config.use_colors() {
            writeln!(handle, "{}", colorize_log_line(&line, &entry.level))?;
        } else {
            writeln!(handle, "{}", line)?;
        }
    }

    handle.flush()?;
    Ok(())
}

/// Format log entry for display
fn format_log_entry(entry: &LogEntry, logs_config: &LogsConfig) -> Result<String> {
    let mut parts = Vec::new();

    // Timestamp
    if logs_config.timestamps {
        parts.push(entry.timestamp.format("%Y-%m-%d %H:%M:%S UTC").to_string());
    }

    // Level
    parts.push(format!("[{}]", entry.level));

    // Step (if present)
    if let Some(step) = &entry.step {
        parts.push(format!("[{}]", step));
    }

    // Source (if different from default)
    if !entry.source.is_empty() && entry.source != "default" && entry.source != "unknown" {
        parts.push(format!("[{}]", entry.source));
    }

    // Message
    parts.push(entry.message.clone());

    Ok(parts.join(" "))
}

/// Colorize log line based on level
fn colorize_log_line(line: &str, level: &str) -> console::StyledObject<String> {
    match level.to_uppercase().as_str() {
        "ERROR" => style(line.to_string()).red(),
        "WARN" | "WARNING" => style(line.to_string()).yellow(),
        "INFO" => style(line.to_string()).white(),
        "DEBUG" => style(line.to_string()).dim(),
        "TRACE" => style(line.to_string()).dim().italic(),
        _ => style(line.to_string()).white(),
    }
}

/// Parse time filter (relative or absolute)
fn parse_time_filter(time_str: &str) -> Result<Timestamptz> {
    // Try absolute timestamp first
    if let Ok(timestamp) = Timestamptz::parse_rfc3339(time_str) {
        return Ok(timestamp);
    }

    // Try relative time (e.g., "5m", "1h", "2d")
    if let Ok(duration) = crate::utils::parse_duration(time_str) {
        let now = Timestamptz::now();
        let target_time = now
            - jiff::SignedDuration::try_from(duration)
                .map_err(|e| CliError::invalid_input(format!("Duration too large: {}", e)))?;
        return Ok(target_time);
    }

    Err(CliError::invalid_input(format!(
        "Invalid time format: {}. Use RFC3339 timestamp or relative duration (5m, 1h, 2d)",
        time_str
    )))
}

/// Parse resource identifier
fn parse_resource_identifier(resource: &str) -> Result<(String, String, Option<String>)> {
    if resource.contains('/') {
        // Format: type/name or namespace/type/name
        let parts: Vec<&str> = resource.split('/').collect();

        match parts.len() {
            2 => {
                // type/name
                Ok((parts[0].to_string(), parts[1].to_string(), None))
            }
            3 => {
                // namespace/type/name
                Ok((
                    parts[1].to_string(),
                    parts[2].to_string(),
                    Some(parts[0].to_string()),
                ))
            }
            _ => Err(CliError::invalid_input(
                "Invalid resource format. Use 'type/name' or 'namespace/type/name'",
            )),
        }
    } else if !resource.is_empty() {
        // A bare id names a workflow.
        Ok(("workflow".to_string(), resource.to_string(), None))
    } else {
        Err(CliError::invalid_input(
            "Invalid resource format. Use a workflow id, 'type/name' or 'namespace/type/name'",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// With a gateway, --follow reads its server-sent event stream, with the
    /// credential, until the stream says it is complete.
    #[tokio::test]
    async fn follow_reads_the_gateways_event_stream() {
        let mut server = mockito::Server::new_async().await;
        let body = concat!(
            "data: {\"timestamp\":\"2026-10-04T12:00:00Z\",\"level\":\"INFO\",\"message\":\"started\",\"source\":\"engine\"}\n\n",
            "data: {\"type\":\"stream_complete\",\"execution_id\":\"e1\"}\n\n",
        );
        let mock = server
            .mock("GET", "/api/v1/executions/e1/logs/stream")
            .match_query(mockito::Matcher::Any)
            .match_header("authorization", "Bearer token-1")
            .with_header("content-type", "text/event-stream")
            .with_body(body)
            .create_async()
            .await;
        let manager = ConnectionManager::new("http://localhost:1", &server.url(), "default");
        let global_config = GlobalConfig {
            quiet: true,
            ..Default::default()
        };
        std::env::set_var("ORCHER_TOKEN", "token-1");
        let result = tokio::time::timeout(
            Duration::from_secs(10),
            follow_logs_sse(
                &manager,
                "execution",
                "e1",
                &LogsConfig::default(),
                &global_config,
            ),
        )
        .await;
        std::env::remove_var("ORCHER_TOKEN");
        result.expect("the stream ends").unwrap();
        mock.assert_async().await;
    }

    #[test]
    fn test_parse_resource_identifier() {
        // Test type/name format
        let (rtype, name, ns) = parse_resource_identifier("workflow/my-workflow").unwrap();
        assert_eq!(rtype, "workflow");
        assert_eq!(name, "my-workflow");
        assert_eq!(ns, None);

        // Test namespace/type/name format
        let (rtype, name, ns) = parse_resource_identifier("default/workflow/my-workflow").unwrap();
        assert_eq!(rtype, "workflow");
        assert_eq!(name, "my-workflow");
        assert_eq!(ns, Some("default".to_string()));

        // A bare id names a workflow
        let (rtype, name, ns) = parse_resource_identifier("order-1001").unwrap();
        assert_eq!(rtype, "workflow");
        assert_eq!(name, "order-1001");
        assert_eq!(ns, None);

        // Test invalid format
        assert!(parse_resource_identifier("").is_err());
        assert!(parse_resource_identifier("a/b/c/d").is_err());
    }

    #[test]
    fn test_parse_time_filter() {
        // Test RFC3339 format
        let result = parse_time_filter("2023-12-01T10:00:00Z");
        assert!(result.is_ok());

        // Test relative format
        let result = parse_time_filter("5m");
        assert!(result.is_ok());

        let result = parse_time_filter("1h");
        assert!(result.is_ok());

        // Test invalid format
        let result = parse_time_filter("invalid");
        assert!(result.is_err());
    }

    #[test]
    fn test_logs_config_default() {
        let config = LogsConfig::default();
        assert!(!config.follow);
        assert_eq!(config.tail, 100);
        assert!(config.timestamps);
        assert!(config.colors);
        assert_eq!(config.poll_interval, Duration::from_secs(2));
    }

    #[test]
    fn test_format_log_entry() {
        let entry = LogEntry {
            timestamp: Timestamptz::now(),
            level: "INFO".to_string(),
            source: "test-source".to_string(),
            message: "Test message".to_string(),
            step: Some("step-1".to_string()),
            metadata: std::collections::HashMap::new(),
        };

        let config = LogsConfig {
            timestamps: true,
            ..Default::default()
        };

        let formatted = format_log_entry(&entry, &config).unwrap();
        assert!(formatted.contains("[INFO]"));
        assert!(formatted.contains("Test message"));
        assert!(formatted.contains("[step-1]"));
        assert!(formatted.contains("[test-source]"));
    }

    #[test]
    fn test_filter_logs() {
        let logs = vec![
            LogEntry {
                timestamp: Timestamptz::now() - jiff::SignedDuration::from_hours(2),
                level: "INFO".to_string(),
                source: "test".to_string(),
                message: "First message".to_string(),
                step: None,
                metadata: std::collections::HashMap::new(),
            },
            LogEntry {
                timestamp: Timestamptz::now() - jiff::SignedDuration::from_hours(1),
                level: "ERROR".to_string(),
                source: "test".to_string(),
                message: "Error message".to_string(),
                step: None,
                metadata: std::collections::HashMap::new(),
            },
            LogEntry {
                timestamp: Timestamptz::now(),
                level: "INFO".to_string(),
                source: "test".to_string(),
                message: "Last message".to_string(),
                step: None,
                metadata: std::collections::HashMap::new(),
            },
        ];

        // Test tail filtering
        let config = LogsConfig {
            tail: 2,
            ..Default::default()
        };

        let filtered = filter_logs(logs.clone(), &config).unwrap();
        assert_eq!(filtered.len(), 2);

        // Test grep filtering
        let config = LogsConfig {
            grep: Some("Error".to_string()),
            ..Default::default()
        };

        let filtered = filter_logs(logs.clone(), &config).unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].level, "ERROR");

        // Test level filtering
        let config = LogsConfig {
            level: Some("ERROR".to_string()),
            ..Default::default()
        };

        let filtered = filter_logs(logs, &config).unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].level, "ERROR");
    }
}
