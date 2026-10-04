//! Shared rendering primitives for list/table CLI output.
//!
//! Modern, restrained look: a borderless-ish `psql` table with a dim header,
//! short git-style IDs, relative timestamps, and colored status cells. Color is
//! carried only by status (meaning), everything else is weight/dim — in line
//! with the black-and-white brand.
//!
//! Status/header cells embed ANSI via `console`; the table is rendered with
//! tabled's `ansi` feature so those escapes don't skew column widths.

use crate::error::{CliError, Result};
use crate::time::Timestamptz;
use console::style;
use tabled::builder::Builder;
use tabled::settings::Style;

/// If `format` is a structured format (`json`/`yaml`), serialize `value` and
/// return `Some(rendered)` — the caller prints it and returns early. `None`
/// means the caller should fall through to the table renderer.
///
/// Gives every list/get command uniform `-o json|yaml` support for scripting.
pub fn structured(format: &str, value: &serde_json::Value) -> Option<Result<String>> {
    match format {
        "json" => Some(
            serde_json::to_string_pretty(value)
                .map_err(|e| CliError::internal(format!("JSON serialization failed: {}", e))),
        ),
        "yaml" => Some(
            serde_yaml::to_string(value)
                .map_err(|e| CliError::internal(format!("YAML serialization failed: {}", e))),
        ),
        _ => None,
    }
}

/// Shorten a UUID-like id to its first 8 chars (git-style). Empty → em dash.
pub fn short_id(id: &str) -> String {
    if id.is_empty() {
        "—".to_string()
    } else if id.len() > 8 {
        id[..8].to_string()
    } else {
        id.to_string()
    }
}

/// A value or an em dash when it's empty, so cells are never blank.
pub fn or_dash(s: &str) -> String {
    if s.is_empty() {
        "—".to_string()
    } else {
        s.to_string()
    }
}

/// A colored `● Label` status cell from a workflow-execution status code.
pub fn status_cell(status: i32) -> String {
    status_cell_str(crate::client::grpc_client::status_to_string(status))
}

/// A colored `● Label` status cell from a status word (any command's vocabulary).
///
/// Grouped by meaning so color is consistent across resources: green = healthy
/// terminal, cyan = in-flight, red = failure, yellow = stopped/degraded.
pub fn status_cell_str(status: &str) -> String {
    let label = nice_label(status);
    match status.to_ascii_uppercase().as_str() {
        "COMPLETED" | "SUCCEEDED" | "SUCCESS" | "ACTIVE" | "DONE" | "HEALTHY" | "READY" => {
            format!("{} {}", style("●").green(), style(label).green())
        }
        "RUNNING" | "PENDING" | "IN_PROGRESS" | "PROCESSING" | "SCHEDULED" | "QUEUED"
        | "STARTED" => {
            format!("{} {}", style("◐").cyan(), style(label).cyan())
        }
        "FAILED" | "FAILURE" | "ERROR" | "UNHEALTHY" => {
            format!("{} {}", style("✗").red(), style(label).red())
        }
        "CANCELED" | "CANCELLED" | "TERMINATED" | "TIMED_OUT" | "DEPRECATED" | "DELETED"
        | "PARTIAL" | "PAUSED" => {
            format!("{} {}", style("⊘").yellow(), style(label).yellow())
        }
        _ => format!("{} {}", style("○").dim(), style(label).dim()),
    }
}

/// A human display label for a status word ("TIMED_OUT" → "Timed out").
fn nice_label(status: &str) -> String {
    match status.to_ascii_uppercase().as_str() {
        "CANCELED" | "CANCELLED" => "Cancelled".to_string(),
        "TIMED_OUT" => "Timed out".to_string(),
        "RESTARTED_FRESH" => "Restarted".to_string(),
        "IN_PROGRESS" => "In progress".to_string(),
        "" => "Unknown".to_string(),
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

/// Relative "time ago" from a unix-epoch second count (`None`/future → `—`/`just now`).
pub fn relative_time(epoch_secs: Option<i64>) -> String {
    epoch_secs
        .and_then(Timestamptz::from_second)
        .map(relative_from)
        .unwrap_or_else(|| "—".to_string())
}

/// Relative "time ago" from an RFC 3339 timestamp string (empty/unparseable →
/// `—`/the leading date), for the REST-backed commands that return ISO strings.
pub fn relative_time_rfc3339(ts: &str) -> String {
    if ts.is_empty() {
        return "—".to_string();
    }
    Timestamptz::parse_rfc3339(ts)
        .ok()
        .map(relative_from)
        .unwrap_or_else(|| ts.chars().take(10).collect())
}

fn relative_from(then: Timestamptz) -> String {
    let elapsed = (Timestamptz::now() - then).as_secs();
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

/// Absolute UTC timestamp (for `--wide`).
pub fn absolute_time(epoch_secs: Option<i64>) -> String {
    epoch_secs
        .and_then(Timestamptz::from_second)
        .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_else(|| "—".to_string())
}

/// Render a modern list table: dim header, `psql` separators, no outer border.
///
/// Cells may contain ANSI color (widths are measured ignoring it).
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_id_cases() {
        assert_eq!(short_id("95dfe7b4-1c8c-4068"), "95dfe7b4");
        assert_eq!(short_id("abc"), "abc");
        assert_eq!(short_id(""), "—");
    }

    #[test]
    fn relative_time_cases() {
        assert_eq!(relative_time(None), "—");
        // A long-past fixed epoch renders as "…d ago", never panics.
        assert!(relative_time(Some(1_600_000_000)).ends_with("d ago"));
    }

    #[test]
    fn table_has_header_and_rows() {
        let out = table(&["ID", "STATUS"], vec![vec!["abc".into(), status_cell(2)]]);
        assert!(out.contains("ID"));
        assert!(out.contains("abc"));
        // psql separator row
        assert!(out.contains("-+-") || out.contains("--"));
    }
}
