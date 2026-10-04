//! `orcher queue` — task queue visibility (list, stats).
//! Uses QueryService.

use crate::client::grpc_client::DEFAULT_GRPC_PORT;
use crate::config::Config;
use crate::error::Result;
use crate::render;
use crate::utils::GlobalConfig;
use clap::{Args, Subcommand};
use console::style;
use tracing::info;

/// Queue management command
#[derive(Args)]
pub struct QueueCommand {
    #[command(subcommand)]
    pub action: QueueCommands,
}

#[derive(Subcommand)]
pub enum QueueCommands {
    /// List workflows by task queue
    #[command(aliases = ["ls"])]
    List {
        /// Filter by specific task queue
        #[arg(short = 'Q', long = "queue")]
        queue_name: Option<String>,

        /// Show all namespaces
        #[arg(short = 'A', long = "all-namespaces")]
        all_namespaces: bool,

        /// Maximum number of results
        #[arg(short = 'l', long = "limit", default_value = "50")]
        limit: i32,
    },

    /// Show queue statistics via workflow execution stats
    Stats {
        /// Task queue name
        queue_name: String,

        /// Filter by workflow type
        #[arg(short = 't', long = "type")]
        workflow_type: Option<String>,
    },
}

/// Execute queue management command
pub async fn execute(cmd: QueueCommand, global_config: &GlobalConfig) -> Result<()> {
    match cmd.action {
        QueueCommands::List {
            queue_name,
            all_namespaces,
            limit,
        } => list_by_queue(queue_name, all_namespaces, limit, global_config).await,
        QueueCommands::Stats {
            queue_name,
            workflow_type,
        } => show_queue_stats(queue_name, workflow_type, global_config).await,
    }
}

fn get_grpc_address(global_config: &GlobalConfig) -> Result<String> {
    if let Ok(config) = Config::load() {
        let context_name = global_config
            .profile
            .as_deref()
            .unwrap_or(&config.current_context);

        if let Some(ctx) = config.contexts.get(context_name) {
            if let Ok(url) = url::Url::parse(&ctx.server) {
                let host = url.host_str().unwrap_or("localhost");
                return Ok(format!("http://{}:{}", host, DEFAULT_GRPC_PORT));
            }
        }
    }
    Ok(format!("http://localhost:{}", DEFAULT_GRPC_PORT))
}

fn get_namespace(global_config: &GlobalConfig) -> String {
    global_config
        .namespace
        .clone()
        .unwrap_or_else(|| "default".to_string())
}

/// List workflows filtered by task queue
async fn list_by_queue(
    queue_name: Option<String>,
    _all_namespaces: bool,
    limit: i32,
    global_config: &GlobalConfig,
) -> Result<()> {
    info!("Listing workflows by task queue");

    let mut client = crate::client::connection::connect_grpc(global_config).await?;

    // Build query for task queue filter
    let query = if let Some(ref queue) = queue_name {
        format!("TaskQueue = '{}'", queue)
    } else {
        String::new()
    };

    let workflows = if query.is_empty() {
        client
            .list_workflows(None, None, limit, None)
            .await?
            .executions
    } else {
        client
            .search_workflows(&query, limit, None)
            .await?
            .executions
    };

    if workflows.is_empty() {
        println!("{}", style("No workflows found").yellow());
        if let Some(q) = &queue_name {
            println!("  Task queue: {}", q);
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
                    "status": crate::client::grpc_client::status_to_string(wf.status),
                    "taskQueue": wf.task_queue,
                    "startedAt": wf.start_time.as_ref().map(|t| t.seconds),
                })
            })
            .collect(),
    );
    if let Some(out) = render::structured(&global_config.output_format, &items) {
        println!("{}", out?);
        return Ok(());
    }

    // Group workflows by task queue
    let mut by_queue: std::collections::HashMap<String, Vec<_>> = std::collections::HashMap::new();
    for wf in &workflows {
        by_queue.entry(wf.task_queue.clone()).or_default().push(wf);
    }

    if !global_config.quiet {
        println!();

        for (queue, wfs) in &by_queue {
            println!(
                "{} {}",
                style(format!("Task Queue: {}", queue)).bold(),
                style(format!("({})", wfs.len())).dim()
            );

            let rows: Vec<Vec<String>> = wfs
                .iter()
                .map(|wf| {
                    vec![
                        render::short_id(&wf.workflow_id),
                        render::or_dash(&wf.workflow_type),
                        render::status_cell(wf.status),
                        render::relative_time(wf.start_time.as_ref().map(|t| t.seconds)),
                    ]
                })
                .collect();

            println!(
                "{}",
                render::table(&["WORKFLOW ID", "TYPE", "STATUS", "STARTED"], rows)
            );
            println!();
        }

        println!(
            "{} workflow(s) across {} queue(s)",
            workflows.len(),
            by_queue.len()
        );
    } else {
        // Quiet mode - just list queue names
        for queue in by_queue.keys() {
            println!("{}", queue);
        }
    }

    Ok(())
}

/// Show queue statistics using workflow execution stats
async fn show_queue_stats(
    queue_name: String,
    workflow_type: Option<String>,
    global_config: &GlobalConfig,
) -> Result<()> {
    info!("Showing statistics for queue: {}", queue_name);

    let mut client = crate::client::connection::connect_grpc(global_config).await?;

    // Build query for task queue
    let mut query_parts = vec![format!("TaskQueue = '{}'", queue_name)];
    if let Some(wf_type) = &workflow_type {
        query_parts.push(format!("WorkflowType = '{}'", wf_type));
    }
    let query = query_parts.join(" AND ");

    // Get workflow count
    let count = client.count_workflows(&query).await?;

    // Get some recent workflows to compute stats
    let response = client.search_workflows(&query, 100, None).await?;
    let workflows = &response.executions;

    println!();
    println!(
        "{}",
        style(format!("Queue Statistics: {}", queue_name)).bold()
    );
    println!("{}", style("-".repeat(50)).dim());

    if let Some(wf_type) = &workflow_type {
        println!("  Workflow Type:  {}", wf_type);
    }
    println!("  Namespace:      {}", get_namespace(global_config));
    println!();

    // Count by status
    let mut running = 0;
    let mut completed = 0;
    let mut failed = 0;
    let mut other = 0;

    for wf in workflows {
        match crate::client::grpc_client::status_to_string(wf.status) {
            "RUNNING" => running += 1,
            "COMPLETED" => completed += 1,
            "FAILED" => failed += 1,
            _ => other += 1,
        }
    }

    println!("{}", style("Summary").bold());
    println!("  Total Workflows:  {}", count);
    println!("  Running:          {}", style(running).cyan());
    println!("  Completed:        {}", style(completed).green());
    println!("  Failed:           {}", style(failed).red());
    if other > 0 {
        println!("  Other:            {}", other);
    }
    println!();

    // Calculate completion rate
    let total_finished = completed + failed;
    if total_finished > 0 {
        let success_rate = (completed as f64 / total_finished as f64) * 100.0;
        println!("{}", style("Performance").bold());
        println!("  Success Rate:     {:.1}%", success_rate);
    }

    println!();
    println!("{}", style("Commands").dim());
    println!(
        "  {} workflow list --query \"TaskQueue = '{}'\"",
        style("orcher").cyan(),
        queue_name
    );
    println!("  {} queue pollers {}", style("orcher").cyan(), queue_name);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_grpc_address_default() {
        let global_config = GlobalConfig::default();
        let address = get_grpc_address(&global_config).unwrap();
        assert_eq!(address, "http://localhost:50051");
    }
}
