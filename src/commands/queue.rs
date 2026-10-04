//! `orcher queue` — task queue visibility (list, stats).
//!
//! The engine has no task-queue filter: ListWorkflows ignores one, and a
//! search for `TaskQueue = '…'` matches a label of that name instead. So these
//! commands page through ListWorkflows, filtering by type on the engine where
//! asked, and pick the task queue out here.

use crate::client::grpc_client::{OrcherGrpcClient, DEFAULT_GRPC_PORT};
use crate::config::Config;
use crate::error::Result;
use crate::render;
use crate::utils::GlobalConfig;
use clap::{Args, Subcommand};
use console::style;
use orcher_proto::WorkflowExecutionInfo;
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

/// The most executions read while looking for a queue's workflows.
const MAX_SCANNED: usize = 10_000;
const PAGE_SIZE: i32 = 500;

/// A queue's workflows, newest first, up to `limit` (all when `None`), and
/// whether every execution was looked at.
async fn workflows_in_queue(
    client: &mut OrcherGrpcClient,
    queue: Option<&str>,
    workflow_type: Option<&str>,
    limit: Option<usize>,
) -> Result<(Vec<WorkflowExecutionInfo>, bool)> {
    let mut found = Vec::new();
    let mut scanned = 0;
    let mut page_token: Option<Vec<u8>> = None;
    loop {
        let page = client
            .list_workflows(workflow_type, None, PAGE_SIZE, page_token.take())
            .await?;
        scanned += page.executions.len();
        found.extend(in_queue(page.executions, queue));
        if limit.is_some_and(|l| found.len() >= l) {
            found.truncate(limit.unwrap_or(usize::MAX));
            return Ok((found, false));
        }
        if page.next_page_token.is_empty() {
            return Ok((found, true));
        }
        if scanned >= MAX_SCANNED {
            return Ok((found, false));
        }
        page_token = Some(page.next_page_token);
    }
}

/// The executions on `queue`, or all of them when no queue is named.
fn in_queue(
    executions: Vec<WorkflowExecutionInfo>,
    queue: Option<&str>,
) -> Vec<WorkflowExecutionInfo> {
    match queue {
        Some(q) => executions
            .into_iter()
            .filter(|e| e.task_queue == q)
            .collect(),
        None => executions,
    }
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

    let (workflows, _) = workflows_in_queue(
        &mut client,
        queue_name.as_deref(),
        None,
        Some(limit.max(1) as usize),
    )
    .await?;

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

    if workflows.is_empty() {
        println!("{}", style("No workflows found").yellow());
        if let Some(q) = &queue_name {
            println!("  Task queue: {}", q);
        }
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

    let (workflows, complete) = workflows_in_queue(
        &mut client,
        Some(&queue_name),
        workflow_type.as_deref(),
        None,
    )
    .await?;
    let count = workflows.len();

    // Structured output (-o json/yaml) for scripting.
    let counts = count_by_status(&workflows);
    let value = serde_json::json!({
        "queue": queue_name,
        "namespace": get_namespace(global_config),
        "workflowType": workflow_type,
        "total": count,
        "running": counts.running,
        "completed": counts.completed,
        "failed": counts.failed,
        "other": counts.other,
        "complete": complete,
    });
    if let Some(out) = render::structured(&global_config.output_format, &value) {
        println!("{}", out?);
        return Ok(());
    }

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

    let StatusCounts {
        running,
        completed,
        failed,
        other,
    } = counts;

    println!("{}", style("Summary").bold());
    let total = if complete {
        count.to_string()
    } else {
        format!("{} (of the latest {} executions)", count, MAX_SCANNED)
    };
    println!("  Total Workflows:  {}", total);
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
        "  {} queue list --queue {}",
        style("orcher").cyan(),
        queue_name
    );

    Ok(())
}

#[derive(Debug, Default, PartialEq)]
struct StatusCounts {
    running: usize,
    completed: usize,
    failed: usize,
    other: usize,
}

fn count_by_status(workflows: &[WorkflowExecutionInfo]) -> StatusCounts {
    let mut counts = StatusCounts::default();
    for wf in workflows {
        match crate::client::grpc_client::status_to_string(wf.status) {
            "RUNNING" => counts.running += 1,
            "COMPLETED" => counts.completed += 1,
            "FAILED" => counts.failed += 1,
            _ => counts.other += 1,
        }
    }
    counts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn execution(id: &str, queue: &str, status: i32) -> WorkflowExecutionInfo {
        WorkflowExecutionInfo {
            workflow_id: id.to_string(),
            task_queue: queue.to_string(),
            status,
            ..Default::default()
        }
    }

    #[test]
    fn only_the_named_queue_is_kept() {
        let all = vec![
            execution("a", "orders", 1),
            execution("b", "emails", 2),
            execution("c", "orders", 3),
        ];
        let ids: Vec<_> = in_queue(all.clone(), Some("orders"))
            .into_iter()
            .map(|e| e.workflow_id)
            .collect();
        assert_eq!(ids, ["a", "c"]);
        assert_eq!(in_queue(all, None).len(), 3);
    }

    #[test]
    fn statuses_are_counted() {
        let counts = count_by_status(&[
            execution("a", "q", 1),
            execution("b", "q", 2),
            execution("c", "q", 2),
            execution("d", "q", 3),
            execution("e", "q", 4),
        ]);
        assert_eq!(
            counts,
            StatusCounts {
                running: 1,
                completed: 2,
                failed: 1,
                other: 1
            }
        );
    }

    #[test]
    fn test_get_grpc_address_default() {
        let global_config = GlobalConfig::default();
        let address = get_grpc_address(&global_config).unwrap();
        assert_eq!(address, "http://localhost:50051");
    }
}
