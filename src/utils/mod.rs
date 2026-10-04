//! Shared utilities and `GlobalConfig`.

#![allow(dead_code)]

use std::path::PathBuf;

pub mod colors;
pub mod output;
pub mod spinner;
pub mod validation;

use crate::constants::{CONFIG_DIR, CONFIG_FILE, CREDENTIALS_FILE};
use crate::error::{CliError, Result};

/// Global configuration passed to all commands
#[derive(Debug, Clone)]
pub struct GlobalConfig {
    /// Optional configuration file path
    pub config_path: Option<String>,

    /// Context to use for operations
    pub context: Option<String>,

    /// Namespace to use for operations
    pub namespace: Option<String>,

    /// Profile to use for operations
    pub profile: Option<String>,

    /// Output format (table, json, yaml, name)
    pub output_format: String,

    /// Enable verbose logging
    pub verbose: bool,

    /// Enable quiet mode
    pub quiet: bool,

    /// Disable colored output
    pub no_color: bool,

    /// Redis URL for DLQ operations
    pub redis_url: Option<String>,

    /// Server URL (gRPC endpoint) - overrides context config
    pub server: Option<String>,

    /// HTTP gateway URL - overrides the context's `api`
    pub api_url: Option<String>,
}

impl Default for GlobalConfig {
    fn default() -> Self {
        Self {
            config_path: None,
            context: None,
            namespace: None,
            profile: None,
            output_format: "table".to_string(),
            verbose: false,
            quiet: false,
            no_color: false,
            redis_url: None,
            server: None,
            api_url: None,
        }
    }
}

impl GlobalConfig {
    /// Get the configuration directory path
    pub fn config_dir(&self) -> Result<PathBuf> {
        if let Some(config_path) = &self.config_path {
            let path = PathBuf::from(config_path);
            if let Some(parent) = path.parent() {
                Ok(parent.to_path_buf())
            } else {
                Ok(PathBuf::from("."))
            }
        } else {
            dirs::home_dir()
                .map(|home| home.join(CONFIG_DIR))
                .ok_or_else(|| CliError::config("Unable to determine home directory"))
        }
    }

    /// Get the full configuration file path
    pub fn config_file_path(&self) -> Result<PathBuf> {
        if let Some(config_path) = &self.config_path {
            Ok(PathBuf::from(config_path))
        } else {
            Ok(self.config_dir()?.join(CONFIG_FILE))
        }
    }

    /// Get the credentials file path
    pub fn credentials_file_path(&self) -> Result<PathBuf> {
        Ok(self.config_dir()?.join(CREDENTIALS_FILE))
    }

    /// Check if output format is structured (json/yaml)
    pub fn is_structured_output(&self) -> bool {
        matches!(self.output_format.as_str(), "json" | "yaml")
    }

    /// Check if colors should be used in output
    pub fn use_colors(&self) -> bool {
        !self.no_color && console::Term::stdout().features().colors_supported()
    }

    /// Get the namespace from config or default
    pub fn get_namespace(&self) -> Option<String> {
        self.namespace.clone()
    }
}

/// Parse duration string (e.g., "30d", "1h", "30m")
pub fn parse_duration(duration_str: &str) -> Result<std::time::Duration> {
    if duration_str.is_empty() {
        return Err(CliError::invalid_input("Duration cannot be empty"));
    }

    let duration_str = duration_str.trim();
    let (number_part, unit_part) =
        if let Some(pos) = duration_str.chars().position(|c| c.is_alphabetic()) {
            duration_str.split_at(pos)
        } else {
            return Err(CliError::invalid_input_with_details(
                "Duration must include a unit (s, m, h, d)",
                "duration",
                "30d, 1h, 30m, 60s",
            ));
        };

    let number: u64 = number_part.parse().map_err(|_| {
        CliError::invalid_input_with_details(
            "Invalid number in duration",
            "duration",
            "positive integer",
        )
    })?;

    let duration = match unit_part.to_lowercase().as_str() {
        "s" | "sec" | "second" | "seconds" => std::time::Duration::from_secs(number),
        "m" | "min" | "minute" | "minutes" => std::time::Duration::from_secs(number * 60),
        "h" | "hr" | "hour" | "hours" => std::time::Duration::from_secs(number * 3600),
        "d" | "day" | "days" => std::time::Duration::from_secs(number * 86400),
        "w" | "week" | "weeks" => std::time::Duration::from_secs(number * 604800),
        "mon" | "month" | "months" => std::time::Duration::from_secs(number * 2592000), // 30 days
        "y" | "year" | "years" => std::time::Duration::from_secs(number * 31536000),    // 365 days
        _ => {
            return Err(CliError::invalid_input_with_details(
                format!("Unknown duration unit: {}", unit_part),
                "duration_unit",
                "s, m, h, d, w, mon, y",
            ));
        }
    };

    Ok(duration)
}

/// Parse key=value pairs from command line arguments
pub fn parse_key_value_pairs(
    pairs: Vec<String>,
) -> Result<std::collections::HashMap<String, String>> {
    let mut result = std::collections::HashMap::new();

    for pair in pairs {
        if let Some(eq_pos) = pair.find('=') {
            let (key, value) = pair.split_at(eq_pos);
            let value = &value[1..]; // Remove the '=' character

            if key.is_empty() {
                return Err(CliError::invalid_input(format!(
                    "Empty key in parameter: {}",
                    pair
                )));
            }

            result.insert(key.to_string(), value.to_string());
        } else {
            return Err(CliError::invalid_input_with_details(
                format!("Invalid key=value format: {}", pair),
                "parameter",
                "key=value",
            ));
        }
    }

    Ok(result)
}

/// Validate resource name according to Kubernetes naming conventions
pub fn validate_resource_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(CliError::invalid_input("Resource name cannot be empty"));
    }

    if name.len() > 253 {
        return Err(CliError::invalid_input(
            "Resource name must be 253 characters or fewer",
        ));
    }

    // Must start and end with alphanumeric character
    if !name.chars().next().unwrap_or('_').is_ascii_alphanumeric() {
        return Err(CliError::invalid_input(
            "Resource name must start with alphanumeric character",
        ));
    }

    if !name.chars().last().unwrap_or('_').is_ascii_alphanumeric() {
        return Err(CliError::invalid_input(
            "Resource name must end with alphanumeric character",
        ));
    }

    // Can only contain lowercase letters, numbers, and hyphens
    for char in name.chars() {
        if !char.is_ascii_lowercase() && !char.is_ascii_digit() && char != '-' {
            return Err(CliError::invalid_input(
                "Resource name can only contain lowercase letters, numbers, and hyphens",
            ));
        }
    }

    Ok(())
}

/// Check if a path exists and is readable
pub fn check_file_readable(path: &std::path::Path) -> Result<()> {
    if !path.exists() {
        return Err(CliError::not_found("file", path.display().to_string()));
    }

    if !path.is_file() {
        return Err(CliError::invalid_input(format!(
            "Path is not a file: {}",
            path.display()
        )));
    }

    // Try to read the file to check permissions
    std::fs::File::open(path)
        .map_err(|e| CliError::io_with_path("Cannot read file", path.display().to_string(), e))?;

    Ok(())
}

/// Expand environment variables in a string
pub fn expand_env_vars(input: &str) -> String {
    let mut result = input.to_string();

    // Simple environment variable expansion
    while let Some(start) = result.find("${") {
        if let Some(end) = result[start..].find('}') {
            let var_name = &result[start + 2..start + end];
            let env_value = std::env::var(var_name).unwrap_or_default();
            result.replace_range(start..start + end + 1, &env_value);
        } else {
            break;
        }
    }

    // Also handle $VAR format
    let re = regex::Regex::new(r"\$([A-Za-z_][A-Za-z0-9_]*)").unwrap();
    result = re
        .replace_all(&result, |caps: &regex::Captures| {
            std::env::var(&caps[1]).unwrap_or_else(|_| caps[0].to_string())
        })
        .to_string();

    result
}

/// Get the user's preferred editor
pub fn get_preferred_editor() -> String {
    std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| {
            if cfg!(target_os = "windows") {
                "notepad".to_string()
            } else {
                "vi".to_string()
            }
        })
}

/// Create a directory if it doesn't exist with proper permissions
pub fn ensure_directory_exists(path: &std::path::Path) -> Result<()> {
    if path.exists() {
        if !path.is_dir() {
            return Err(CliError::invalid_input(format!(
                "Path exists but is not a directory: {}",
                path.display()
            )));
        }
        return Ok(());
    }

    std::fs::create_dir_all(path).map_err(|e| {
        CliError::io_with_path("Failed to create directory", path.display().to_string(), e)
    })?;

    // Set secure permissions on Unix-like systems
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path)
            .map_err(|e| {
                CliError::io_with_path(
                    "Failed to get directory metadata",
                    path.display().to_string(),
                    e,
                )
            })?
            .permissions();
        perms.set_mode(0o700); // rwx------
        std::fs::set_permissions(path, perms).map_err(|e| {
            CliError::io_with_path(
                "Failed to set directory permissions",
                path.display().to_string(),
                e,
            )
        })?;
    }

    Ok(())
}

/// Truncate string to specified length with ellipsis
pub fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else if max_len <= 3 {
        "...".to_string()
    } else {
        format!("{}...", &s[..max_len - 3])
    }
}

/// Convert bytes to human readable format
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB", "PB"];

    if bytes == 0 {
        return "0 B".to_string();
    }

    let base = 1024_f64;
    let log = (bytes as f64).log(base).floor() as usize;
    let unit_index = std::cmp::min(log, UNITS.len() - 1);

    let value = bytes as f64 / base.powi(unit_index as i32);

    if value >= 100.0 {
        format!("{:.0} {}", value, UNITS[unit_index])
    } else if value >= 10.0 {
        format!("{:.1} {}", value, UNITS[unit_index])
    } else {
        format!("{:.2} {}", value, UNITS[unit_index])
    }
}

/// Format duration in human readable format
pub fn format_duration(duration: std::time::Duration) -> String {
    let total_seconds = duration.as_secs();

    if total_seconds < 60 {
        format!("{}s", total_seconds)
    } else if total_seconds < 3600 {
        let minutes = total_seconds / 60;
        let seconds = total_seconds % 60;
        if seconds == 0 {
            format!("{}m", minutes)
        } else {
            format!("{}m{}s", minutes, seconds)
        }
    } else if total_seconds < 86400 {
        let hours = total_seconds / 3600;
        let minutes = (total_seconds % 3600) / 60;
        if minutes == 0 {
            format!("{}h", hours)
        } else {
            format!("{}h{}m", hours, minutes)
        }
    } else {
        let days = total_seconds / 86400;
        let hours = (total_seconds % 86400) / 3600;
        if hours == 0 {
            format!("{}d", days)
        } else {
            format!("{}d{}h", days, hours)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_duration() {
        assert_eq!(
            parse_duration("30s").unwrap(),
            std::time::Duration::from_secs(30)
        );
        assert_eq!(
            parse_duration("5m").unwrap(),
            std::time::Duration::from_secs(300)
        );
        assert_eq!(
            parse_duration("2h").unwrap(),
            std::time::Duration::from_secs(7200)
        );
        assert_eq!(
            parse_duration("1d").unwrap(),
            std::time::Duration::from_secs(86400)
        );

        assert!(parse_duration("").is_err());
        assert!(parse_duration("30").is_err());
        assert!(parse_duration("30x").is_err());
    }

    #[test]
    fn test_parse_key_value_pairs() {
        let pairs = vec!["key1=value1".to_string(), "key2=value2".to_string()];
        let result = parse_key_value_pairs(pairs).unwrap();

        assert_eq!(result.get("key1"), Some(&"value1".to_string()));
        assert_eq!(result.get("key2"), Some(&"value2".to_string()));

        let invalid = vec!["invalid".to_string()];
        assert!(parse_key_value_pairs(invalid).is_err());
    }

    #[test]
    fn test_validate_resource_name() {
        assert!(validate_resource_name("valid-name").is_ok());
        assert!(validate_resource_name("valid123").is_ok());
        assert!(validate_resource_name("a").is_ok());

        assert!(validate_resource_name("").is_err());
        assert!(validate_resource_name("Invalid-Name").is_err()); // uppercase
        assert!(validate_resource_name("-invalid").is_err()); // starts with hyphen
        assert!(validate_resource_name("invalid-").is_err()); // ends with hyphen
        assert!(validate_resource_name("invalid_name").is_err()); // underscore
    }

    #[test]
    fn test_truncate_string() {
        assert_eq!(truncate_string("hello", 10), "hello");
        assert_eq!(truncate_string("hello world", 8), "hello...");
        assert_eq!(truncate_string("hi", 2), "hi");
        assert_eq!(truncate_string("hello", 3), "...");
    }

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(1024), "1.00 KB");
        assert_eq!(format_bytes(1536), "1.50 KB");
        assert_eq!(format_bytes(1048576), "1.00 MB");
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(std::time::Duration::from_secs(30)), "30s");
        assert_eq!(format_duration(std::time::Duration::from_secs(90)), "1m30s");
        assert_eq!(format_duration(std::time::Duration::from_secs(3600)), "1h");
        assert_eq!(format_duration(std::time::Duration::from_secs(86400)), "1d");
    }

    #[test]
    fn test_expand_env_vars() {
        std::env::set_var("TEST_VAR", "test_value");

        assert_eq!(expand_env_vars("${TEST_VAR}"), "test_value");
        assert_eq!(expand_env_vars("$TEST_VAR"), "test_value");
        assert_eq!(
            expand_env_vars("prefix_${TEST_VAR}_suffix"),
            "prefix_test_value_suffix"
        );
        assert_eq!(expand_env_vars("${NONEXISTENT}"), "");
    }
}
