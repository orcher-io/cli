//! `orcher logs`: a workflow's log, as the engine writes it while the
//! workflow runs.

use crate::client::Client;
use crate::error::Result;
use crate::render;
use crate::settings::GlobalArgs;
use clap::Args;
use console::style;
use jiff::{SignedDuration, Timestamp};
use orcher_proto::{ExecutionLogEntry, GetExecutionLogsRequest, WorkflowExecutionStatus};
use std::time::Duration;

/// How often `--follow` asks for new lines.
const FOLLOW_INTERVAL: Duration = Duration::from_secs(1);

/// The most lines read in one go before `--tail` picks the last of them.
const MAX_LINES: usize = 10_000;
const PAGE_SIZE: i32 = 500;

#[derive(Args)]
#[command(after_help = "\
Examples:
  orcher logs order-1001                  The last 100 lines
  orcher logs order-1001 --follow         Keep printing until the workflow ends
  orcher logs order-1001 --level error    Only errors
  orcher logs order-1001 --since 10m      Only the last ten minutes")]
pub struct LogsCommand {
    workflow_id: String,

    /// A specific run, rather than the latest
    #[arg(short, long)]
    execution_id: Option<String>,

    /// Keep printing new lines until the workflow ends
    #[arg(short, long)]
    follow: bool,

    /// How many of the most recent lines to show first (0 for all)
    #[arg(long, default_value_t = 100)]
    tail: usize,

    /// Only lines at this level: trace, debug, info, warn, error
    #[arg(long)]
    level: Option<String>,

    /// Only lines from this step or task
    #[arg(long)]
    step: Option<String>,

    /// Only lines since this time: RFC 3339, or a duration ago such as 10m, 2h, 1d
    #[arg(long, value_parser = parse_since)]
    since: Option<Timestamp>,
}

pub async fn run(cmd: LogsCommand, args: &GlobalArgs) -> Result<()> {
    let mut client = Client::connect(args).await?;
    let base = GetExecutionLogsRequest {
        workflow_id: cmd.workflow_id.clone(),
        execution_id: cmd.execution_id.clone().unwrap_or_default(),
        page_size: PAGE_SIZE,
        level: cmd.level.clone().unwrap_or_default(),
        step: cmd.step.clone().unwrap_or_default(),
        since: cmd.since.map(|t| prost_types::Timestamp {
            seconds: t.as_second(),
            nanos: t.subsec_nanosecond(),
        }),
        ..Default::default()
    };

    // Everything so far, then the last `--tail` of it.
    let mut lines = Vec::new();
    let mut page_token = Vec::new();
    loop {
        let response = client
            .get_execution_logs(GetExecutionLogsRequest {
                next_page_token: page_token,
                ..base.clone()
            })
            .await?;
        lines.extend(response.logs);
        page_token = response.next_page_token;
        if page_token.is_empty() || lines.len() >= MAX_LINES {
            break;
        }
    }
    lines.sort_by_key(|l| l.id);
    let mut last_id = lines.iter().map(|l| l.id).max().unwrap_or(0);
    let skip = if cmd.tail == 0 {
        0
    } else {
        lines.len().saturating_sub(cmd.tail)
    };

    let structured = args.is_structured();
    for line in &lines[skip..] {
        print_line(line, structured, args)?;
    }
    if !cmd.follow {
        if lines.is_empty() && !args.quiet && !structured {
            println!("No log lines for '{}'.", cmd.workflow_id);
        }
        return Ok(());
    }

    // Follow: print what is new each second until the workflow has ended and
    // nothing more has arrived.
    loop {
        let ended = has_ended(&mut client, &cmd.workflow_id, cmd.execution_id.as_deref()).await?;
        let response = client
            .get_execution_logs(GetExecutionLogsRequest {
                after_id: last_id,
                ..base.clone()
            })
            .await?;
        let mut new = response.logs;
        new.sort_by_key(|l| l.id);
        for line in &new {
            print_line(line, structured, args)?;
            last_id = last_id.max(line.id);
        }
        if ended && new.is_empty() {
            return Ok(());
        }
        tokio::time::sleep(FOLLOW_INTERVAL).await;
    }
}

async fn has_ended(
    client: &mut Client,
    workflow_id: &str,
    execution_id: Option<&str>,
) -> Result<bool> {
    let status = client
        .get_workflow_status(workflow_id, execution_id)
        .await?;
    Ok(!matches!(
        WorkflowExecutionStatus::try_from(status.status),
        Ok(WorkflowExecutionStatus::Running | WorkflowExecutionStatus::Pending)
    ))
}

fn print_line(line: &ExecutionLogEntry, structured: bool, args: &GlobalArgs) -> Result<()> {
    if structured {
        // One JSON object per line, so that a followed log can be piped.
        let value = serde_json::json!({
            "id": line.id,
            "timestamp": render::rfc3339(line.timestamp.as_ref()),
            "level": line.level,
            "step": line.step,
            "message": line.message,
            "source": line.source,
        });
        match args.output {
            crate::settings::Output::Yaml => print!("---\n{}", serde_yaml::to_string(&value)?),
            _ => println!("{}", serde_json::to_string(&value)?),
        }
        return Ok(());
    }
    let level = line.level.to_ascii_uppercase();
    let level_cell = match level.as_str() {
        "ERROR" | "FATAL" => style(format!("{level:<5}")).red(),
        "WARN" | "WARNING" => style(format!("{level:<5}")).yellow(),
        "DEBUG" | "TRACE" => style(format!("{level:<5}")).dim(),
        _ => style(format!("{level:<5}")),
    };
    let step = if line.step.is_empty() {
        String::new()
    } else {
        format!("{} ", style(format!("[{}]", line.step)).dim())
    };
    println!(
        "{} {} {}{}",
        style(render::absolute_time(line.timestamp.as_ref())).dim(),
        level_cell,
        step,
        line.message
    );
    Ok(())
}

/// An RFC 3339 time, or a duration ago such as `30s`, `10m`, `2h` or `1d`.
fn parse_since(value: &str) -> std::result::Result<Timestamp, String> {
    if let Ok(t) = value.parse::<Timestamp>() {
        return Ok(t);
    }
    let invalid = || format!("'{value}' is neither an RFC 3339 time nor a duration such as 10m");
    let split = value
        .find(|c: char| !c.is_ascii_digit())
        .ok_or_else(invalid)?;
    let (number, unit) = value.split_at(split);
    let number: i64 = number.parse().map_err(|_| invalid())?;
    let seconds = match unit {
        "s" => number,
        "m" => number * 60,
        "h" => number * 3600,
        "d" => number * 86_400,
        _ => return Err(invalid()),
    };
    Timestamp::now()
        .checked_sub(SignedDuration::from_secs(seconds))
        .map_err(|_| invalid())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn since_takes_a_time_or_a_duration() {
        assert_eq!(
            parse_since("2026-10-04T12:00:00Z").unwrap(),
            "2026-10-04T12:00:00Z".parse::<Timestamp>().unwrap()
        );
        let ten_minutes_ago = parse_since("10m").unwrap();
        let elapsed = Timestamp::now().as_second() - ten_minutes_ago.as_second();
        assert!((599..=601).contains(&elapsed), "{elapsed}");
        assert!(parse_since("1d").is_ok());
        assert!(parse_since("10").is_err());
        assert!(parse_since("10w").is_err());
        assert!(parse_since("yesterday").is_err());
    }
}
