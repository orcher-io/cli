//! Output formatting helpers (table, JSON, YAML, plain text).

use crate::error::{CliError, Result};
use console::style;
use serde::{Deserialize, Serialize};

use std::fmt;
use std::io::Write;

/// Output format options
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    Table,
    Json,
    Yaml,
    Name,
    Wide,
}

impl fmt::Display for OutputFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OutputFormat::Table => write!(f, "table"),
            OutputFormat::Json => write!(f, "json"),
            OutputFormat::Yaml => write!(f, "yaml"),
            OutputFormat::Name => write!(f, "name"),
            OutputFormat::Wide => write!(f, "wide"),
        }
    }
}

impl std::str::FromStr for OutputFormat {
    type Err = CliError;

    fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "table" => Ok(OutputFormat::Table),
            "json" => Ok(OutputFormat::Json),
            "yaml" => Ok(OutputFormat::Yaml),
            "name" => Ok(OutputFormat::Name),
            "wide" => Ok(OutputFormat::Wide),
            _ => Err(CliError::invalid_input_with_details(
                format!("Invalid output format: {}", s),
                "output_format",
                "table, json, yaml, name, wide",
            )),
        }
    }
}

/// Trait for objects that can be displayed in various formats
pub trait Displayable {
    /// Convert to table rows
    fn to_table_rows(&self) -> Vec<Vec<String>>;

    /// Get table headers
    fn table_headers(&self) -> Vec<String>;

    /// Get wide table headers (with additional columns)
    fn wide_table_headers(&self) -> Vec<String> {
        self.table_headers()
    }

    /// Convert to wide table rows
    fn to_wide_table_rows(&self) -> Vec<Vec<String>> {
        self.to_table_rows()
    }

    /// Get resource name for name-only output
    fn resource_name(&self) -> String;

    /// Convert to JSON value
    fn to_json(&self) -> serde_json::Value;
}

/// Table builder for consistent table formatting
pub struct Table {
    headers: Vec<String>,
    rows: Vec<Vec<String>>,
    colors_enabled: bool,
}

impl Table {
    /// Create a new table
    pub fn new() -> Self {
        Self {
            headers: Vec::new(),
            rows: Vec::new(),
            colors_enabled: true,
        }
    }

    /// Create a table with headers
    pub fn with_headers(headers: Vec<String>) -> Self {
        Self {
            headers,
            rows: Vec::new(),
            colors_enabled: true,
        }
    }

    /// Set whether colors are enabled
    pub fn set_colors_enabled(mut self, enabled: bool) -> Self {
        self.colors_enabled = enabled;
        self
    }

    /// Set table headers
    pub fn set_headers(&mut self, headers: Vec<String>) {
        self.headers = headers;
    }

    /// Add a row to the table
    pub fn add_row(&mut self, row: Vec<String>) {
        self.rows.push(row);
    }

    /// Add multiple rows to the table
    pub fn add_rows(&mut self, rows: Vec<Vec<String>>) {
        self.rows.extend(rows);
    }

    /// Display the table
    pub fn display(&self) -> Result<()> {
        self.write_to(&mut std::io::stdout())
    }

    /// Write table to a writer
    pub fn write_to<W: Write>(&self, writer: &mut W) -> Result<()> {
        if self.rows.is_empty() {
            return Ok(());
        }

        // Calculate column widths
        let mut col_widths = self.headers.iter().map(|h| h.len()).collect::<Vec<_>>();

        for row in &self.rows {
            for (i, cell) in row.iter().enumerate() {
                if i < col_widths.len() {
                    col_widths[i] = col_widths[i].max(cell.len());
                }
            }
        }

        // Print headers
        if !self.headers.is_empty() {
            self.print_row(writer, &self.headers, &col_widths, true)?;
            self.print_separator(writer, &col_widths)?;
        }

        // Print rows
        for row in &self.rows {
            self.print_row(writer, row, &col_widths, false)?;
        }

        Ok(())
    }

    fn print_row<W: Write>(
        &self,
        writer: &mut W,
        row: &[String],
        col_widths: &[usize],
        is_header: bool,
    ) -> Result<()> {
        for (i, cell) in row.iter().enumerate() {
            if i > 0 {
                write!(writer, "  ")?;
            }

            let width = col_widths.get(i).copied().unwrap_or(0);
            let formatted_cell = if is_header && self.colors_enabled {
                style(format!("{:<width$}", cell, width = width))
                    .bold()
                    .to_string()
            } else {
                format!("{:<width$}", cell, width = width)
            };

            write!(writer, "{}", formatted_cell)?;
        }
        writeln!(writer)?;
        Ok(())
    }

    fn print_separator<W: Write>(&self, writer: &mut W, col_widths: &[usize]) -> Result<()> {
        for (i, &width) in col_widths.iter().enumerate() {
            if i > 0 {
                write!(writer, "  ")?;
            }
            write!(writer, "{}", "-".repeat(width))?;
        }
        writeln!(writer)?;
        Ok(())
    }
}

impl Default for Table {
    fn default() -> Self {
        Self::new()
    }
}

/// Output formatter for displaying data in various formats
pub struct OutputFormatter {
    format: OutputFormat,
    colors_enabled: bool,
    quiet: bool,
}

impl OutputFormatter {
    /// Create a new output formatter
    pub fn new(format: OutputFormat, colors_enabled: bool, quiet: bool) -> Self {
        Self {
            format,
            colors_enabled,
            quiet,
        }
    }

    /// Display a single item
    pub fn display_item<T: Displayable>(&self, item: &T) -> Result<()> {
        match self.format {
            OutputFormat::Table => {
                let mut table = Table::new().set_colors_enabled(self.colors_enabled);
                table.set_headers(item.table_headers());
                table.add_rows(item.to_table_rows());
                table.display()
            }
            OutputFormat::Wide => {
                let mut table = Table::new().set_colors_enabled(self.colors_enabled);
                table.set_headers(item.wide_table_headers());
                table.add_rows(item.to_wide_table_rows());
                table.display()
            }
            OutputFormat::Json => {
                let json = serde_json::to_string_pretty(&item.to_json())?;
                println!("{}", json);
                Ok(())
            }
            OutputFormat::Yaml => {
                let yaml = serde_yaml::to_string(&item.to_json())?;
                print!("{}", yaml);
                Ok(())
            }
            OutputFormat::Name => {
                println!("{}", item.resource_name());
                Ok(())
            }
        }
    }

    /// Display multiple items
    pub fn display_items<T: Displayable>(&self, items: &[T]) -> Result<()> {
        if items.is_empty() {
            if !self.quiet {
                println!("No resources found");
            }
            return Ok(());
        }

        match self.format {
            OutputFormat::Table => {
                let mut table = Table::new().set_colors_enabled(self.colors_enabled);
                table.set_headers(items[0].table_headers());
                for item in items {
                    table.add_rows(item.to_table_rows());
                }
                table.display()
            }
            OutputFormat::Wide => {
                let mut table = Table::new().set_colors_enabled(self.colors_enabled);
                table.set_headers(items[0].wide_table_headers());
                for item in items {
                    table.add_rows(item.to_wide_table_rows());
                }
                table.display()
            }
            OutputFormat::Json => {
                let json_items: Vec<serde_json::Value> =
                    items.iter().map(|item| item.to_json()).collect();
                let json = serde_json::to_string_pretty(&json_items)?;
                println!("{}", json);
                Ok(())
            }
            OutputFormat::Yaml => {
                let json_items: Vec<serde_json::Value> =
                    items.iter().map(|item| item.to_json()).collect();
                let yaml = serde_yaml::to_string(&json_items)?;
                print!("{}", yaml);
                Ok(())
            }
            OutputFormat::Name => {
                for item in items {
                    println!("{}", item.resource_name());
                }
                Ok(())
            }
        }
    }

    /// Print a success message
    pub fn success(&self, message: &str) {
        if !self.quiet {
            if self.colors_enabled {
                println!("{}", style(message).green());
            } else {
                println!("{}", message);
            }
        }
    }

    /// Print an info message
    pub fn info(&self, message: &str) {
        if !self.quiet {
            if self.colors_enabled {
                println!("{}", style(message).blue());
            } else {
                println!("{}", message);
            }
        }
    }

    /// Print a warning message
    pub fn warning(&self, message: &str) {
        if !self.quiet {
            if self.colors_enabled {
                eprintln!("{}", style(message).yellow());
            } else {
                eprintln!("{}", message);
            }
        }
    }

    /// Print an error message
    pub fn error(&self, message: &str) {
        if self.colors_enabled {
            eprintln!("{}", style(message).red());
        } else {
            eprintln!("{}", message);
        }
    }
}

/// Status indicator for resources
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourceStatus {
    Running,
    Completed,
    Failed,
    Pending,
    Unknown,
}

impl ResourceStatus {
    /// Get colored representation of status
    pub fn colored(&self, colors_enabled: bool) -> String {
        let status_str = self.to_string();
        if !colors_enabled {
            return status_str;
        }

        match self {
            ResourceStatus::Running => style(status_str).green().to_string(),
            ResourceStatus::Completed => style(status_str).green().bold().to_string(),
            ResourceStatus::Failed => style(status_str).red().to_string(),
            ResourceStatus::Pending => style(status_str).yellow().to_string(),
            ResourceStatus::Unknown => style(status_str).dim().to_string(),
        }
    }
}

impl fmt::Display for ResourceStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ResourceStatus::Running => write!(f, "Running"),
            ResourceStatus::Completed => write!(f, "Completed"),
            ResourceStatus::Failed => write!(f, "Failed"),
            ResourceStatus::Pending => write!(f, "Pending"),
            ResourceStatus::Unknown => write!(f, "Unknown"),
        }
    }
}

impl From<&str> for ResourceStatus {
    fn from(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "running" => ResourceStatus::Running,
            "completed" | "success" | "succeeded" => ResourceStatus::Completed,
            "failed" | "error" | "failure" => ResourceStatus::Failed,
            "pending" | "waiting" => ResourceStatus::Pending,
            _ => ResourceStatus::Unknown,
        }
    }
}

/// Progress indicator for long-running operations
pub struct ProgressIndicator {
    message: String,
    spinner: indicatif::ProgressBar,
    colors_enabled: bool,
}

impl ProgressIndicator {
    /// Create a new progress indicator
    pub fn new(message: &str, colors_enabled: bool) -> Self {
        let spinner = if colors_enabled {
            indicatif::ProgressBar::new_spinner()
        } else {
            indicatif::ProgressBar::hidden()
        };

        spinner.set_style(
            indicatif::ProgressStyle::default_spinner()
                .tick_chars("⠁⠂⠄⡀⢀⠠⠐⠈ ")
                .template("{spinner:.blue} {msg}")
                .expect("Invalid template"),
        );

        spinner.set_message(message.to_string());

        Self {
            message: message.to_string(),
            spinner,
            colors_enabled,
        }
    }

    /// Update the progress message
    pub fn set_message(&mut self, message: &str) {
        self.message = message.to_string();
        self.spinner.set_message(message.to_string());
    }

    /// Finish with success message
    pub fn finish_with_message(&self, message: &str) {
        if self.colors_enabled {
            self.spinner
                .finish_with_message(format!("{} {}", style("✓").green().bold(), message));
        } else {
            self.spinner.finish_with_message(message.to_string());
        }
    }

    /// Finish with error message
    pub fn finish_with_error(&self, message: &str) {
        if self.colors_enabled {
            self.spinner
                .finish_with_message(format!("{} {}", style("✗").red().bold(), message));
        } else {
            self.spinner.finish_with_message(message.to_string());
        }
    }

    /// Clear the progress indicator
    pub fn clear(&self) {
        self.spinner.finish_and_clear();
    }
}

impl Drop for ProgressIndicator {
    fn drop(&mut self) {
        self.spinner.finish_and_clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[derive(Debug)]
    struct TestResource {
        name: String,
        status: String,
        age: String,
    }

    impl Displayable for TestResource {
        fn to_table_rows(&self) -> Vec<Vec<String>> {
            vec![vec![
                self.name.clone(),
                self.status.clone(),
                self.age.clone(),
            ]]
        }

        fn table_headers(&self) -> Vec<String> {
            vec!["NAME".to_string(), "STATUS".to_string(), "AGE".to_string()]
        }

        fn resource_name(&self) -> String {
            self.name.clone()
        }

        fn to_json(&self) -> serde_json::Value {
            json!({
                "name": self.name,
                "status": self.status,
                "age": self.age
            })
        }
    }

    #[test]
    fn test_output_format_parsing() {
        assert_eq!(
            "table".parse::<OutputFormat>().unwrap(),
            OutputFormat::Table
        );
        assert_eq!("json".parse::<OutputFormat>().unwrap(), OutputFormat::Json);
        assert_eq!("yaml".parse::<OutputFormat>().unwrap(), OutputFormat::Yaml);
        assert_eq!("name".parse::<OutputFormat>().unwrap(), OutputFormat::Name);
        assert_eq!("wide".parse::<OutputFormat>().unwrap(), OutputFormat::Wide);

        assert!("invalid".parse::<OutputFormat>().is_err());
    }

    #[test]
    fn test_resource_status() {
        assert_eq!(ResourceStatus::from("running"), ResourceStatus::Running);
        assert_eq!(ResourceStatus::from("completed"), ResourceStatus::Completed);
        assert_eq!(ResourceStatus::from("failed"), ResourceStatus::Failed);
        assert_eq!(ResourceStatus::from("pending"), ResourceStatus::Pending);
        assert_eq!(ResourceStatus::from("unknown"), ResourceStatus::Unknown);
    }

    #[test]
    fn test_table_creation() {
        let mut table = Table::new();
        table.set_headers(vec!["NAME".to_string(), "STATUS".to_string()]);
        table.add_row(vec!["test".to_string(), "running".to_string()]);

        // Test that table can be created without errors
        assert_eq!(table.headers.len(), 2);
        assert_eq!(table.rows.len(), 1);
    }

    #[test]
    fn test_displayable_trait() {
        let resource = TestResource {
            name: "test-workflow".to_string(),
            status: "running".to_string(),
            age: "2m".to_string(),
        };

        assert_eq!(resource.resource_name(), "test-workflow");
        assert_eq!(resource.table_headers(), vec!["NAME", "STATUS", "AGE"]);

        let json = resource.to_json();
        assert_eq!(json["name"], "test-workflow");
        assert_eq!(json["status"], "running");
        assert_eq!(json["age"], "2m");
    }
}
