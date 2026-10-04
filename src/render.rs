//! How things look on the terminal: tables, status cells, timestamps, and
//! `-o json|yaml` for scripts.
//!
//! Color carries meaning only (a status), so everything else is plain or dim.
//! Cells may contain color escapes; tables measure widths without them.

use crate::error::Result;
use crate::settings::{GlobalArgs, Output};
use console::style;
use jiff::Timestamp;
use orcher_proto::WorkflowExecutionStatus;
use tabled::builder::Builder;
use tabled::settings::Style;

/// Prints `value` as JSON or YAML when that output was asked for, and says
/// whether it did; when it returns `false` the caller prints for people.
pub fn structured(args: &GlobalArgs, value: &serde_json::Value) -> Result<bool> {
    match args.output {
        Output::Json => println!("{}", serde_json::to_string_pretty(value)?),
        Output::Yaml => print!("{}", serde_yaml::to_string(value)?),
        Output::Table | Output::Name => return Ok(false),
    }
    Ok(true)
}

/// The value, or a dash when it is empty, so that no cell is blank.
pub fn or_dash(s: &str) -> String {
    if s.is_empty() {
        "—".to_string()
    } else {
        s.to_string()
    }
}

/// The name of a workflow execution status, as in `RUNNING`.
pub fn status_name(status: i32) -> &'static str {
    match WorkflowExecutionStatus::try_from(status) {
        Ok(WorkflowExecutionStatus::Running) => "RUNNING",
        Ok(WorkflowExecutionStatus::Completed) => "COMPLETED",
        Ok(WorkflowExecutionStatus::Failed) => "FAILED",
        Ok(WorkflowExecutionStatus::Canceled) => "CANCELED",
        Ok(WorkflowExecutionStatus::Terminated) => "TERMINATED",
        Ok(WorkflowExecutionStatus::RestartedFresh) => "RESTARTED_FRESH",
        Ok(WorkflowExecutionStatus::TimedOut) => "TIMED_OUT",
        Ok(WorkflowExecutionStatus::Reset) => "RESET",
        Ok(WorkflowExecutionStatus::Pending) => "PENDING",
        Ok(WorkflowExecutionStatus::Unspecified) | Err(_) => "UNKNOWN",
    }
}

/// Parses a status as typed on the command line (`running`, `cancelled`).
pub fn parse_status(status: &str) -> Option<i32> {
    let status = match status.to_ascii_uppercase().replace('-', "_").as_str() {
        "RUNNING" => WorkflowExecutionStatus::Running,
        "COMPLETED" => WorkflowExecutionStatus::Completed,
        "FAILED" => WorkflowExecutionStatus::Failed,
        "CANCELED" | "CANCELLED" => WorkflowExecutionStatus::Canceled,
        "TERMINATED" => WorkflowExecutionStatus::Terminated,
        "RESTARTED_FRESH" => WorkflowExecutionStatus::RestartedFresh,
        "TIMED_OUT" => WorkflowExecutionStatus::TimedOut,
        "RESET" => WorkflowExecutionStatus::Reset,
        "PENDING" => WorkflowExecutionStatus::Pending,
        _ => return None,
    };
    Some(status as i32)
}

/// A colored status cell for a workflow execution status.
pub fn status_cell(status: i32) -> String {
    status_cell_str(status_name(status))
}

/// A colored status cell for a status word. Colors mean the same for every
/// resource: green finished well, cyan in flight, red failed, yellow stopped.
pub fn status_cell_str(status: &str) -> String {
    let label = label(status);
    match status.to_ascii_uppercase().as_str() {
        "COMPLETED" | "SUCCEEDED" | "ACTIVE" | "READY" => {
            format!("{} {}", style("●").green(), style(label).green())
        }
        "RUNNING" | "PENDING" | "SCHEDULED" | "STARTED" => {
            format!("{} {}", style("◐").cyan(), style(label).cyan())
        }
        "FAILED" | "ERROR" => format!("{} {}", style("✗").red(), style(label).red()),
        "CANCELED" | "CANCELLED" | "TERMINATED" | "TIMED_OUT" | "DEPRECATED" | "DELETED" => {
            format!("{} {}", style("⊘").yellow(), style(label).yellow())
        }
        _ => format!("{} {}", style("○").dim(), style(label).dim()),
    }
}

/// "TIMED_OUT" becomes "Timed out".
fn label(status: &str) -> String {
    match status.to_ascii_uppercase().as_str() {
        "" => "Unknown".to_string(),
        "RESTARTED_FRESH" => "Restarted".to_string(),
        other => {
            let lower = other.replace('_', " ").to_ascii_lowercase();
            let mut chars = lower.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        }
    }
}

/// A protobuf timestamp as a jiff one.
pub fn timestamp(ts: Option<&prost_types::Timestamp>) -> Option<Timestamp> {
    let ts = ts?;
    Timestamp::new(ts.seconds, ts.nanos).ok()
}

/// "5m ago", or a dash when there is no time.
pub fn relative_time(ts: Option<&prost_types::Timestamp>) -> String {
    match timestamp(ts) {
        Some(then) => relative_from(then, Timestamp::now()),
        None => "—".to_string(),
    }
}

fn relative_from(then: Timestamp, now: Timestamp) -> String {
    let elapsed = now.as_second() - then.as_second();
    if elapsed < 60 {
        "just now".to_string()
    } else if elapsed < 3600 {
        format!("{}m ago", elapsed / 60)
    } else if elapsed < 86_400 {
        format!("{}h ago", elapsed / 3600)
    } else {
        format!("{}d ago", elapsed / 86_400)
    }
}

/// A UTC time such as `2026-10-04 12:21:53`, or a dash.
pub fn absolute_time(ts: Option<&prost_types::Timestamp>) -> String {
    match timestamp(ts) {
        Some(t) => t.strftime("%Y-%m-%d %H:%M:%S").to_string(),
        None => "—".to_string(),
    }
}

/// An RFC 3339 time for machine-readable output, or null.
pub fn rfc3339(ts: Option<&prost_types::Timestamp>) -> serde_json::Value {
    match timestamp(ts) {
        Some(t) => serde_json::Value::String(t.to_string()),
        None => serde_json::Value::Null,
    }
}

/// A table with a dim header and no outer border.
pub fn table(headers: &[&str], rows: Vec<Vec<String>>) -> String {
    let mut builder = Builder::default();
    builder.push_record(headers.iter().map(|h| style(h).dim().bold().to_string()));
    for row in rows {
        builder.push_record(row);
    }
    let mut table = builder.build();
    table.with(Style::psql());
    table.to_string()
}

/// Bytes the engine stored as a payload: JSON when they are JSON, otherwise
/// a note of how many bytes there are.
pub fn payload(bytes: &[u8]) -> serde_json::Value {
    if bytes.is_empty() {
        return serde_json::Value::Null;
    }
    serde_json::from_slice(bytes).unwrap_or_else(|_| match std::str::from_utf8(bytes) {
        Ok(text) => serde_json::Value::String(text.to_string()),
        Err(_) => serde_json::Value::String(format!("<{} bytes of binary data>", bytes.len())),
    })
}

/// "1 workflow", "2 workflows".
pub fn plural(count: usize, one: &str, many: &str) -> String {
    if count == 1 {
        format!("1 {one}")
    } else {
        format!("{count} {many}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn statuses_round_trip() {
        for name in ["running", "completed", "failed", "terminated", "timed-out"] {
            let code = parse_status(name).expect(name);
            assert_eq!(
                status_name(code),
                name.to_ascii_uppercase().replace('-', "_")
            );
        }
        assert_eq!(parse_status("cancelled"), parse_status("canceled"));
        assert_eq!(parse_status("bogus"), None);
        assert_eq!(status_name(99), "UNKNOWN");
    }

    #[test]
    fn relative_times() {
        let now = Timestamp::from_second(1_700_000_000).unwrap();
        let ago =
            |secs: i64| relative_from(Timestamp::from_second(1_700_000_000 - secs).unwrap(), now);
        assert_eq!(ago(5), "just now");
        assert_eq!(ago(300), "5m ago");
        assert_eq!(ago(7200), "2h ago");
        assert_eq!(ago(3 * 86_400), "3d ago");
        assert_eq!(relative_time(None), "—");
    }

    #[test]
    fn absolute_times_are_utc() {
        let ts = prost_types::Timestamp {
            seconds: 1_700_000_000,
            nanos: 0,
        };
        assert_eq!(absolute_time(Some(&ts)), "2023-11-14 22:13:20");
        assert_eq!(rfc3339(Some(&ts)), "2023-11-14T22:13:20Z");
    }

    #[test]
    fn payloads() {
        assert_eq!(payload(br#"{"a":1}"#), serde_json::json!({"a": 1}));
        assert_eq!(payload(b"plain"), serde_json::json!("plain"));
        assert_eq!(
            payload(&[0xff, 0xfe]),
            serde_json::json!("<2 bytes of binary data>")
        );
        assert_eq!(payload(b""), serde_json::Value::Null);
    }

    #[test]
    fn tables_have_a_header_and_rows() {
        console::set_colors_enabled(false);
        let out = table(&["ID", "STATUS"], vec![vec!["abc".into(), status_cell(2)]]);
        assert!(out.contains("ID"));
        assert!(out.contains("abc"));
        assert!(out.contains("Completed"));
    }
}
