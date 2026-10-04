//! `orcher status` — dashboard view and per-resource status.

use crate::client::connection::ConnectionManager;
use crate::client::grpc_client::{status_to_string, OrcherGrpcClient};
use crate::config::Config;
use crate::error::{CliError, Result};
use crate::render;
use crate::utils::GlobalConfig;
use console::style;
use std::collections::HashMap;
use tracing::debug;

pub async fn execute(
    resource_type: Option<String>,
    all: bool,
    global_config: &GlobalConfig,
) -> Result<()> {
    // If no resource type specified, show dashboard
    if resource_type.is_none() {
        return show_dashboard(global_config).await;
    }

    // Decorative header would corrupt json/yaml piping, so skip it there.
    if !global_config.quiet && !global_config.is_structured_output() {
        println!("{}", style("Resource Status").cyan().bold());
        println!();
    }

    // Create gRPC client with smart detection
    let mut client = crate::client::connection::connect_grpc(global_config).await?;

    // Determine what to show
    match resource_type.as_deref() {
        Some("workflows") | Some("workflow") | Some("wf") => {
            show_workflow_status(&mut client, all, global_config).await?;
        }
        Some("executions") | Some("execution") | Some("exec") => {
            show_execution_status(&mut client, all, global_config).await?;
        }
        Some(unknown) => {
            return Err(CliError::InvalidInput {
                message: format!(
                    "Unknown resource type: {}\nValid types:\n  - workflows - Show workflow status\n  - executions - Show execution status",
                    unknown
                ),
                field: Some("resource_type".to_string()),
                expected: Some("workflows or executions".to_string()),
            });
        }
        None => {
            // Show executions by default
            show_execution_status(&mut client, all, global_config).await?;
        }
    }

    Ok(())
}

async fn show_dashboard(global_config: &GlobalConfig) -> Result<()> {
    let config = Config::load().ok();

    if !global_config.quiet {
        println!();
        println!(
            "{}",
            style("╔════════════════════════════════════════════════════════════════╗").cyan()
        );
        println!(
            "{}",
            style("║                    ORCHER Status Dashboard                     ║").cyan()
        );
        println!(
            "{}",
            style("╚════════════════════════════════════════════════════════════════╝").cyan()
        );
        println!();
    }

    // Configuration Status
    if !global_config.quiet {
        println!("{}", style("Configuration").bold());
        println!("{}", style("─".repeat(50)).dim());

        // The server and namespace actually in use: a flag or the
        // environment overrides the context's.
        let resolved = ConnectionManager::from_config(global_config);
        match &config {
            Some(cfg) => println!("  Context:   {}", style(&cfg.current_context).green()),
            None => println!("  Context:   {}", style("(not configured)").yellow()),
        }
        println!("  Server:    {}", style(resolved.grpc_addr()).cyan());
        println!("  Namespace: {}", style(resolved.namespace()).dim());
        println!();
    }

    // Server Status - check both gRPC and HTTP
    let manager = ConnectionManager::from_config(global_config);
    let (grpc_status, http_status) = manager.detect_servers().await;

    if !global_config.quiet {
        println!("{}", style("Server Status").bold());
        println!("{}", style("─".repeat(50)).dim());

        // gRPC Orchestrator
        if grpc_status.available {
            println!("  Orchestrator (gRPC):  {} Connected", style("●").green());
            println!("    Address: {}", style(&grpc_status.address).cyan());
            if let Some(latency) = grpc_status.latency_ms {
                println!("    Latency: {}ms", latency);
            }
        } else {
            println!("  Orchestrator (gRPC):  {} Disconnected", style("●").red());
            println!("    Address: {}", style(&grpc_status.address).dim());
            if let Some(err) = &grpc_status.error {
                println!("    Error: {}", style(err).dim());
            }
        }

        // HTTP Gateway: only an engine with a gateway in front of it (a cloud
        // deployment) has one, so an unconfigured one is not "down".
        if manager.configured_http_addr().is_none() && !http_status.available {
            println!(
                "  Gateway (HTTP):       {} not configured",
                style("○").dim()
            );
        } else if http_status.available {
            println!("  Gateway (HTTP):       {} Connected", style("●").green());
            println!("    Address: {}", style(&http_status.address).cyan());
        } else {
            println!("  Gateway (HTTP):       {} Disconnected", style("●").red());
            println!("    Address: {}", style(&http_status.address).dim());
        }

        println!();

        // Overall status
        if grpc_status.available || http_status.available {
            println!("  Overall: {}", style("Services available").green());
        } else {
            println!("  Overall: {}", style("All services offline").red());
            println!();
            println!(
                "  {}",
                style("Start services with: orcher server start").yellow()
            );
        }
        println!();
    }

    // Quick Stats (only if gRPC connected)
    if grpc_status.available {
        if let Ok(mut client) = crate::client::connection::connect_grpc(global_config).await {
            if !global_config.quiet {
                println!("{}", style("Quick Stats").bold());
                println!("{}", style("─".repeat(50)).dim());

                // Try to get workflow counts
                match client.count_workflows("").await {
                    Ok(total) => {
                        println!("  Total Workflows: {}", style(total).cyan());

                        // Get running count
                        if let Ok(running) = client
                            .count_workflows("WorkflowExecutionStatus = 'Running'")
                            .await
                        {
                            println!("  Running:         {}", style(running).green());
                        }

                        // Get failed count
                        if let Ok(failed) = client
                            .count_workflows("WorkflowExecutionStatus = 'Failed'")
                            .await
                        {
                            if failed > 0 {
                                println!("  Failed:          {}", style(failed).red());
                            }
                        }
                    }
                    Err(_) => {
                        println!("  (Unable to fetch statistics)");
                    }
                }
                println!();
            }
        }
    }

    // Available Commands
    if !global_config.quiet {
        println!("{}", style("Quick Commands").bold());
        println!("{}", style("─".repeat(50)).dim());
        println!(
            "  {} workflow list          List workflow executions",
            style("orcher").cyan()
        );
        println!(
            "  {} queue list             List task queues",
            style("orcher").cyan()
        );
        println!(
            "  {} server status          Check server health",
            style("orcher").cyan()
        );
        println!(
            "  {} config view            View configuration",
            style("orcher").cyan()
        );
        println!();

        println!("{}", style("Getting Started").bold());
        println!("{}", style("─".repeat(50)).dim());
        println!(
            "  {} new project myapp      Create a new project",
            style("orcher").cyan()
        );
        println!(
            "  {} run my-workflow        Execute a workflow",
            style("orcher").cyan()
        );
        println!(
            "  {} --help                 Show all commands",
            style("orcher").cyan()
        );
        println!();
    }

    Ok(())
}

/// Show workflow execution status via gRPC
async fn show_execution_status(
    client: &mut OrcherGrpcClient,
    _all: bool,
    global_config: &GlobalConfig,
) -> Result<()> {
    debug!("Fetching execution status via gRPC");

    // Use list_workflows to get recent executions
    let response = client.list_workflows(None, None, 50, None).await?;

    let executions = response.executions;

    if executions.is_empty() {
        if !global_config.quiet {
            println!("  {}  No executions found", style("•").yellow());
            println!();
        }
        return Ok(());
    }

    // Structured output (-o json/yaml) for scripting.
    let items = serde_json::Value::Array(
        executions
            .iter()
            .map(|e| {
                serde_json::json!({
                    "workflowId": e.workflow_id,
                    "type": e.workflow_type,
                    "status": status_to_string(e.status),
                    "startedAt": e.start_time.as_ref().map(|t| t.seconds),
                })
            })
            .collect(),
    );
    if let Some(out) = render::structured(&global_config.output_format, &items) {
        println!("{}", out?);
        return Ok(());
    }

    // Display executions
    if !global_config.quiet {
        println!(
            "{}",
            style(format!("Recent Executions ({})", executions.len())).bold()
        );
        println!();

        let rows: Vec<Vec<String>> = executions
            .iter()
            .map(|e| {
                vec![
                    render::short_id(&e.workflow_id),
                    render::or_dash(&e.workflow_type),
                    render::status_cell(e.status),
                    render::relative_time(e.start_time.as_ref().map(|t| t.seconds)),
                ]
            })
            .collect();

        println!(
            "{}",
            render::table(&["WORKFLOW ID", "TYPE", "STATUS", "STARTED"], rows)
        );
        println!();

        // Summary
        let mut status_counts: HashMap<String, usize> = HashMap::new();
        for execution in &executions {
            let status = status_to_string(execution.status);
            *status_counts.entry(status.to_string()).or_insert(0) += 1;
        }

        println!();
        println!("{}", style("Summary:").bold());
        for (status, count) in status_counts.iter() {
            let status_display = match status.as_str() {
                "COMPLETED" => style(status).green(),
                "FAILED" => style(status).red(),
                "RUNNING" => style(status).cyan(),
                "CANCELED" => style(status).yellow(),
                _ => style(status).white(),
            };
            println!("  {} {}: {}", style("•").dim(), status_display, count);
        }
        println!();

        // Show commands
        println!("{}", style("Commands:").dim());
        println!(
            "  {} workflow list              # List all workflows",
            style("orcher").cyan()
        );
        println!(
            "  {} workflow get <id>          # View workflow details",
            style("orcher").cyan()
        );
        println!(
            "  {} workflow history <id>      # View execution history",
            style("orcher").cyan()
        );
        println!();
    } else {
        // Quiet mode - just output workflow IDs
        for execution in &executions {
            println!("{}", execution.workflow_id);
        }
    }

    Ok(())
}

/// Show workflow status via gRPC (registered workflow types)
async fn show_workflow_status(
    client: &mut OrcherGrpcClient,
    _all: bool,
    global_config: &GlobalConfig,
) -> Result<()> {
    debug!("Fetching workflow status via gRPC");

    // Get workflow executions grouped by type
    let response = client.list_workflows(None, None, 100, None).await?;

    // Group by workflow type
    let mut workflow_types: HashMap<String, Vec<_>> = HashMap::new();
    for execution in response.executions {
        workflow_types
            .entry(execution.workflow_type.clone())
            .or_default()
            .push(execution);
    }

    if workflow_types.is_empty() {
        if !global_config.quiet {
            println!("  {}  No workflows found", style("•").yellow());
            println!();
        }
        return Ok(());
    }

    // Structured output (-o json/yaml) for scripting.
    if global_config.is_structured_output() {
        let items = serde_json::Value::Array(
            workflow_types
                .iter()
                .map(|(t, execs)| {
                    serde_json::json!({
                        "type": t,
                        "executions": execs.len(),
                        "running": execs.iter().filter(|e| e.status == 1).count(),
                        "failed": execs.iter().filter(|e| e.status == 3).count(),
                    })
                })
                .collect(),
        );
        if let Some(out) = render::structured(&global_config.output_format, &items) {
            println!("{}", out?);
            return Ok(());
        }
    }

    if !global_config.quiet {
        println!(
            "{}",
            style(format!("Workflow Types ({})", workflow_types.len())).bold()
        );
        println!("{}", style("─".repeat(80)).dim());
        println!();

        // Header
        println!(
            "{:<30} {:<15} {:<15} {:<15}",
            style("WORKFLOW TYPE").bold(),
            style("EXECUTIONS").bold(),
            style("RUNNING").bold(),
            style("FAILED").bold()
        );
        println!("{}", style("─".repeat(80)).dim());

        // Rows
        for (workflow_type, executions) in &workflow_types {
            let total = executions.len();
            let running = executions.iter().filter(|e| e.status == 1).count();
            let failed = executions.iter().filter(|e| e.status == 3).count();

            let display_type = if workflow_type.len() > 28 {
                format!("{}...", &workflow_type[..28])
            } else {
                workflow_type.clone()
            };

            let running_display = if running > 0 {
                style(format!("{:<15}", running)).cyan()
            } else {
                style(format!("{:<15}", running)).dim()
            };

            let failed_display = if failed > 0 {
                style(format!("{:<15}", failed)).red()
            } else {
                style(format!("{:<15}", failed)).dim()
            };

            println!(
                "{:<30} {:<15} {} {}",
                style(&display_type).cyan(),
                total,
                running_display,
                failed_display
            );
        }

        println!();
        println!("{}", style("─".repeat(80)).dim());
        println!();

        println!("{}", style("Commands:").dim());
        println!(
            "  {} run <workflow-type>       # Execute workflow",
            style("orcher").cyan()
        );
        println!(
            "  {} workflow list -t <type>   # List by type",
            style("orcher").cyan()
        );
        println!();
    } else {
        // Quiet mode - output workflow types
        for workflow_type in workflow_types.keys() {
            println!("{}", workflow_type);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_resource_type_parsing() {
        // Valid resource types
        assert!(matches!("workflows", "workflows" | "workflow" | "wf"));
        assert!(matches!("executions", "executions" | "execution" | "exec"));
        assert!(matches!("tasks", "tasks" | "task"));
    }
}
