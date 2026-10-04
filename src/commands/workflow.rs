//! `orcher workflow`: inspect and control workflow executions.

use crate::client::Client;
use crate::commands::confirm;
use crate::error::{Error, Result};
use crate::render;
use crate::settings::{GlobalArgs, Output};
use clap::{Args, Subcommand};
use console::style;
use orcher_proto::get_workflow_status_response::Outcome;
use orcher_proto::EntryType;

#[derive(Args)]
#[command(after_help = "\
Examples:
  orcher workflow list                          Recent executions
  orcher workflow list --status running         Only the running ones
  orcher workflow describe order-1001           Status, outcome and pending work
  orcher workflow history order-1001            Everything the engine recorded
  orcher workflow cancel order-1001 --yes       Ask it to stop")]
pub struct WorkflowCommand {
    #[command(subcommand)]
    action: Action,
}

#[derive(Subcommand)]
enum Action {
    /// List workflow executions
    #[command(alias = "ls")]
    List {
        /// Only workflows of this type
        #[arg(short = 't', long = "type")]
        workflow_type: Option<String>,

        /// Only workflows in this status: running, completed, failed,
        /// canceled, terminated, timed-out
        #[arg(short, long)]
        status: Option<String>,

        /// Search with a query instead of filters
        #[arg(long, conflicts_with_all = ["workflow_type", "status"])]
        query: Option<String>,

        /// How many to show
        #[arg(short, long, default_value_t = 50)]
        limit: i32,

        /// Show full ids and absolute times
        #[arg(long)]
        wide: bool,
    },

    /// Show a workflow's status, outcome and pending work
    #[command(alias = "get")]
    Describe {
        workflow_id: String,

        /// A specific run, rather than the latest
        #[arg(short, long)]
        execution_id: Option<String>,
    },

    /// Ask a workflow to stop; it can clean up before it ends
    Cancel {
        workflow_id: String,

        /// A specific run, rather than the latest
        #[arg(short, long)]
        execution_id: Option<String>,

        /// Why, for the record
        #[arg(short, long, default_value = "")]
        reason: String,

        /// Do not ask for confirmation
        #[arg(short = 'y', long)]
        yes: bool,
    },

    /// Stop a workflow at once, without letting it clean up
    Terminate {
        workflow_id: String,

        /// A specific run, rather than the latest
        #[arg(short, long)]
        execution_id: Option<String>,

        /// Why, for the record
        #[arg(short, long, default_value = "terminated from the CLI")]
        reason: String,

        /// Do not ask for confirmation
        #[arg(short = 'y', long)]
        yes: bool,
    },

    /// Send an event to a running workflow
    Event {
        workflow_id: String,

        /// The event name the workflow waits for
        event_name: String,

        /// The event's payload, as JSON
        #[arg(short, long)]
        payload: Option<String>,
    },

    /// Show the journal: every step the engine recorded
    #[command(alias = "events")]
    History {
        workflow_id: String,

        /// A specific run, rather than the latest
        #[arg(short, long)]
        execution_id: Option<String>,

        /// How many entries to show
        #[arg(short, long, default_value_t = 100)]
        limit: i32,
    },

    /// Show the tasks a workflow ran: status, attempts and time taken
    Tasks {
        workflow_id: String,

        /// A specific run, rather than the latest
        #[arg(short, long)]
        execution_id: Option<String>,

        /// How many tasks to show
        #[arg(short, long, default_value_t = 100)]
        limit: i32,
    },
}

pub async fn run(cmd: WorkflowCommand, args: &GlobalArgs) -> Result<()> {
    match cmd.action {
        Action::List {
            workflow_type,
            status,
            query,
            limit,
            wide,
        } => list(workflow_type, status, query, limit, wide, args).await,
        Action::Describe {
            workflow_id,
            execution_id,
        } => describe(&workflow_id, execution_id.as_deref(), args).await,
        Action::Cancel {
            workflow_id,
            execution_id,
            reason,
            yes,
        } => cancel(&workflow_id, execution_id.as_deref(), &reason, yes, args).await,
        Action::Terminate {
            workflow_id,
            execution_id,
            reason,
            yes,
        } => terminate(&workflow_id, execution_id.as_deref(), &reason, yes, args).await,
        Action::Event {
            workflow_id,
            event_name,
            payload,
        } => event(&workflow_id, &event_name, payload, args).await,
        Action::History {
            workflow_id,
            execution_id,
            limit,
        } => history(&workflow_id, execution_id.as_deref(), limit, args).await,
        Action::Tasks {
            workflow_id,
            execution_id,
            limit,
        } => tasks(&workflow_id, execution_id.as_deref(), limit, args).await,
    }
}

async fn list(
    workflow_type: Option<String>,
    status: Option<String>,
    query: Option<String>,
    limit: i32,
    wide: bool,
    args: &GlobalArgs,
) -> Result<()> {
    let status_filter = match &status {
        Some(s) => Some(render::parse_status(s).ok_or_else(|| {
            Error::invalid_input(format!(
                "unknown status '{s}': use running, completed, failed, canceled, terminated or timed-out"
            ))
        })?),
        None => None,
    };

    let mut client = Client::connect(args).await?;
    let (workflows, more) = match &query {
        Some(q) => {
            let r = client.search_workflows(q, limit).await?;
            (r.executions, !r.next_page_token.is_empty())
        }
        None => {
            let r = client
                .list_workflows(workflow_type.as_deref(), status_filter, limit)
                .await?;
            (r.executions, !r.next_page_token.is_empty())
        }
    };

    let items: Vec<serde_json::Value> = workflows
        .iter()
        .map(|wf| {
            serde_json::json!({
                "workflowId": wf.workflow_id,
                "executionId": wf.execution_id,
                "type": wf.workflow_type,
                "taskQueue": wf.task_queue,
                "status": render::status_name(wf.status),
                "startedAt": render::rfc3339(wf.start_time.as_ref()),
                "closedAt": render::rfc3339(wf.close_time.as_ref()),
            })
        })
        .collect();
    if render::structured(args, &serde_json::Value::Array(items))? {
        return Ok(());
    }
    if args.output == Output::Name {
        for wf in &workflows {
            println!("{}", wf.workflow_id);
        }
        return Ok(());
    }

    if workflows.is_empty() {
        if !args.quiet {
            println!("No workflows found in namespace '{}'.", client.namespace());
        }
        return Ok(());
    }

    let rows = workflows
        .iter()
        .map(|wf| {
            let (id, started) = if wide {
                (
                    render::or_dash(&wf.workflow_id),
                    render::absolute_time(wf.start_time.as_ref()),
                )
            } else {
                (
                    truncate(&wf.workflow_id, 36),
                    render::relative_time(wf.start_time.as_ref()),
                )
            };
            vec![
                id,
                render::or_dash(&wf.workflow_type),
                render::status_cell(wf.status),
                started,
            ]
        })
        .collect();
    println!(
        "{}",
        render::table(&["WORKFLOW ID", "TYPE", "STATUS", "STARTED"], rows)
    );
    if !args.quiet {
        let mut summary = render::plural(workflows.len(), "workflow", "workflows");
        if more {
            summary.push_str(", more with a higher --limit");
        }
        println!("{}", style(summary).dim());
    }
    Ok(())
}

/// Shortens a long id for the table, keeping its start.
fn truncate(id: &str, max: usize) -> String {
    if id.chars().count() <= max {
        render::or_dash(id)
    } else {
        let head: String = id.chars().take(max - 1).collect();
        format!("{head}…")
    }
}

async fn describe(workflow_id: &str, execution_id: Option<&str>, args: &GlobalArgs) -> Result<()> {
    let mut client = Client::connect(args).await?;
    let status = client
        .get_workflow_status(workflow_id, execution_id)
        .await?;
    let details = client.describe_workflow(workflow_id, execution_id).await?;
    let info = details.execution_info.as_ref();

    let execution = status.execution.as_ref();
    let shown_id = execution
        .map(|e| e.workflow_id.as_str())
        .filter(|id| !id.is_empty())
        .unwrap_or(workflow_id);

    let mut obj = serde_json::json!({
        "workflowId": shown_id,
        "executionId": execution.map(|e| e.execution_id.as_str()),
        "type": info.map(|i| i.workflow_type.as_str()),
        "taskQueue": info.map(|i| i.task_queue.as_str()),
        "namespace": info.map(|i| i.namespace.as_str()),
        "status": render::status_name(status.status),
        "attempt": info.map(|i| i.attempt),
        "startedAt": render::rfc3339(status.started_at.as_ref()),
        "completedAt": render::rfc3339(status.completed_at.as_ref()),
        "pendingTasks": details.pending_tasks,
        "pendingTimers": details.pending_timers,
        "pendingEvents": details.pending_events,
    });
    match &status.outcome {
        Some(Outcome::Result(bytes)) => obj["result"] = render::payload(bytes),
        Some(Outcome::Error(e)) => obj["error"] = serde_json::json!(e),
        None => {}
    }
    if let Some(cron) = info.map(|i| &i.cron_schedule).filter(|c| !c.is_empty()) {
        obj["cronSchedule"] = serde_json::json!(cron);
    }
    if render::structured(args, &obj)? {
        return Ok(());
    }
    if args.output == Output::Name {
        println!("{shown_id}");
        return Ok(());
    }

    let field = |name: &str, value: String| println!("  {:<15}{}", style(name).dim(), value);
    println!("{}", style(shown_id).bold());
    if let Some(e) = execution {
        field("Execution", render::or_dash(&e.execution_id));
    }
    field("Status", render::status_cell(status.status));
    if let Some(i) = info {
        field("Type", render::or_dash(&i.workflow_type));
        field("Task queue", render::or_dash(&i.task_queue));
        field("Namespace", render::or_dash(&i.namespace));
        field("Attempt", i.attempt.to_string());
        if !i.cron_schedule.is_empty() {
            field("Cron", i.cron_schedule.clone());
        }
    }
    field("Started", render::absolute_time(status.started_at.as_ref()));
    if status.completed_at.is_some() {
        field(
            "Completed",
            render::absolute_time(status.completed_at.as_ref()),
        );
    }
    field(
        "Pending",
        format!(
            "{} tasks, {} timers, {} events",
            details.pending_tasks, details.pending_timers, details.pending_events
        ),
    );
    match &status.outcome {
        Some(Outcome::Result(bytes)) => {
            println!();
            println!("{}", style("Result").bold());
            println!("{}", serde_json::to_string_pretty(&render::payload(bytes))?);
        }
        Some(Outcome::Error(e)) => {
            println!();
            println!("{}", style("Error").red().bold());
            println!("{e}");
        }
        None => {}
    }
    Ok(())
}

async fn cancel(
    workflow_id: &str,
    execution_id: Option<&str>,
    reason: &str,
    yes: bool,
    args: &GlobalArgs,
) -> Result<()> {
    if !confirm(&format!("Cancel workflow '{workflow_id}'?"), yes)? {
        return Err(Error::invalid_input("not confirmed; nothing was cancelled"));
    }
    let mut client = Client::connect(args).await?;
    if !client
        .cancel_workflow(workflow_id, execution_id, reason)
        .await?
    {
        return Err(Error::local(format!(
            "the engine did not accept the cancellation of '{workflow_id}'"
        )));
    }
    if !args.quiet {
        println!("Cancellation of '{workflow_id}' requested.");
    }
    Ok(())
}

async fn terminate(
    workflow_id: &str,
    execution_id: Option<&str>,
    reason: &str,
    yes: bool,
    args: &GlobalArgs,
) -> Result<()> {
    if !confirm(
        &format!("Terminate workflow '{workflow_id}' now, without cleanup?"),
        yes,
    )? {
        return Err(Error::invalid_input(
            "not confirmed; nothing was terminated",
        ));
    }
    let mut client = Client::connect(args).await?;
    if !client
        .terminate_workflow(workflow_id, execution_id, reason)
        .await?
    {
        return Err(Error::local(format!(
            "the engine did not terminate '{workflow_id}'"
        )));
    }
    if !args.quiet {
        println!("Workflow '{workflow_id}' terminated.");
    }
    Ok(())
}

async fn event(
    workflow_id: &str,
    event_name: &str,
    payload: Option<String>,
    args: &GlobalArgs,
) -> Result<()> {
    let payload = match payload {
        Some(p) => json_bytes(&p, "--payload")?,
        None => Vec::new(),
    };
    let mut client = Client::connect(args).await?;
    if !client.send_event(workflow_id, event_name, payload).await? {
        return Err(Error::local(format!(
            "the engine did not record event '{event_name}' for '{workflow_id}'"
        )));
    }
    if !args.quiet {
        println!("Event '{event_name}' sent to '{workflow_id}'.");
    }
    Ok(())
}

/// Checks that `text` is JSON and returns it as bytes, unchanged.
pub fn json_bytes(text: &str, flag: &str) -> Result<Vec<u8>> {
    serde_json::from_str::<serde_json::Value>(text)
        .map_err(|e| Error::invalid_input(format!("{flag} is not valid JSON: {e}")))?;
    Ok(text.as_bytes().to_vec())
}

/// "TASK_COMPLETED" for `EntryType::TaskCompleted`.
pub fn entry_type_name(entry_type: i32) -> String {
    match EntryType::try_from(entry_type) {
        Ok(t) => t
            .as_str_name()
            .trim_start_matches("ENTRY_TYPE_")
            .to_string(),
        Err(_) => format!("ENTRY_TYPE_{entry_type}"),
    }
}

async fn history(
    workflow_id: &str,
    execution_id: Option<&str>,
    limit: i32,
    args: &GlobalArgs,
) -> Result<()> {
    let mut client = Client::connect(args).await?;
    let response = client
        .get_execution_journal(workflow_id, execution_id, limit)
        .await?;
    let entries = response.journal;

    let items: Vec<serde_json::Value> = entries
        .iter()
        .map(|e| {
            serde_json::json!({
                "id": e.entry_id,
                "type": entry_type_name(e.entry_type),
                "taskId": e.task_id,
                "timestamp": render::rfc3339(e.timestamp.as_ref()),
            })
        })
        .collect();
    if render::structured(args, &serde_json::Value::Array(items))? {
        return Ok(());
    }

    if entries.is_empty() {
        if !args.quiet {
            println!("No journal entries for '{workflow_id}'.");
        }
        return Ok(());
    }
    let rows = entries
        .iter()
        .map(|e| {
            let name = entry_type_name(e.entry_type);
            let colored = if name.ends_with("FAILED") || name.ends_with("TIMED_OUT") {
                style(name).red().to_string()
            } else if name.ends_with("COMPLETED") {
                style(name).green().to_string()
            } else {
                name
            };
            vec![
                e.entry_id.to_string(),
                render::absolute_time(e.timestamp.as_ref()),
                colored,
                if e.task_id == 0 {
                    "—".to_string()
                } else {
                    e.task_id.to_string()
                },
            ]
        })
        .collect();
    println!("{}", render::table(&["ID", "TIME", "ENTRY", "TASK"], rows));
    if !args.quiet {
        let mut summary = render::plural(entries.len(), "entry", "entries");
        if !response.next_page_token.is_empty() {
            summary.push_str(", more with a higher --limit");
        }
        println!("{}", style(summary).dim());
    }
    Ok(())
}

async fn tasks(
    workflow_id: &str,
    execution_id: Option<&str>,
    limit: i32,
    args: &GlobalArgs,
) -> Result<()> {
    let mut client = Client::connect(args).await?;
    let tasks = client
        .get_task_executions(workflow_id, execution_id, limit)
        .await?
        .tasks;

    let items: Vec<serde_json::Value> = tasks
        .iter()
        .map(|t| {
            serde_json::json!({
                "taskId": t.task_id,
                "type": t.task_type,
                "status": t.status,
                "attempt": t.attempt,
                "startedAt": render::rfc3339(t.started_at.as_ref()),
                "completedAt": render::rfc3339(t.completed_at.as_ref()),
                "error": if t.error_message.is_empty() { None } else { Some(&t.error_message) },
            })
        })
        .collect();
    if render::structured(args, &serde_json::Value::Array(items))? {
        return Ok(());
    }
    if args.output == Output::Name {
        for t in &tasks {
            println!("{}", t.task_id);
        }
        return Ok(());
    }
    if tasks.is_empty() {
        if !args.quiet {
            println!("No tasks have run for '{workflow_id}'.");
        }
        return Ok(());
    }

    let rows = tasks
        .iter()
        .map(|t| {
            vec![
                render::or_dash(&t.task_type),
                render::status_cell_str(&t.status),
                t.attempt.to_string(),
                render::relative_time(t.started_at.as_ref()),
                duration(t.started_at.as_ref(), t.completed_at.as_ref()),
            ]
        })
        .collect();
    println!(
        "{}",
        render::table(&["TASK", "STATUS", "ATTEMPT", "STARTED", "TOOK"], rows)
    );
    for t in tasks.iter().filter(|t| !t.error_message.is_empty()) {
        println!(
            "{} {}",
            style(format!("{}:", render::or_dash(&t.task_type))).red(),
            t.error_message
        );
    }
    Ok(())
}

/// "350ms", "4.2s", "3.1m", or a dash for a task that has not finished.
fn duration(
    start: Option<&prost_types::Timestamp>,
    end: Option<&prost_types::Timestamp>,
) -> String {
    let (Some(start), Some(end)) = (render::timestamp(start), render::timestamp(end)) else {
        return "—".to_string();
    };
    let ms = end.as_millisecond() - start.as_millisecond();
    if ms < 1000 {
        format!("{ms}ms")
    } else if ms < 60_000 {
        format!("{:.1}s", ms as f64 / 1000.0)
    } else {
        format!("{:.1}m", ms as f64 / 60_000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn journal_entry_names_drop_the_prefix() {
        assert_eq!(
            entry_type_name(EntryType::TaskCompleted as i32),
            "TASK_COMPLETED"
        );
        assert_eq!(
            entry_type_name(EntryType::WorkflowExecutionStarted as i32),
            "WORKFLOW_EXECUTION_STARTED"
        );
        assert_eq!(entry_type_name(9999), "ENTRY_TYPE_9999");
    }

    #[test]
    fn payloads_must_be_json() {
        assert_eq!(
            json_bytes(r#"{"ok":true}"#, "--payload").unwrap(),
            br#"{"ok":true}"#
        );
        assert!(json_bytes("not json", "--payload").is_err());
    }

    #[test]
    fn task_durations() {
        let at = |ms: i64| prost_types::Timestamp {
            seconds: ms / 1000,
            nanos: ((ms % 1000) * 1_000_000) as i32,
        };
        assert_eq!(duration(Some(&at(0)), Some(&at(350))), "350ms");
        assert_eq!(duration(Some(&at(0)), Some(&at(4200))), "4.2s");
        assert_eq!(duration(Some(&at(0)), Some(&at(186_000))), "3.1m");
        assert_eq!(duration(Some(&at(0)), None), "—");
    }

    #[test]
    fn long_ids_are_shortened() {
        assert_eq!(truncate("short", 36), "short");
        let long = "a".repeat(40);
        assert_eq!(truncate(&long, 10).chars().count(), 10);
    }
}
