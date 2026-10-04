//! CLI command implementations.

#![allow(dead_code)]

pub mod auth;
pub mod batch;
pub mod completion;
pub mod config;
pub mod dev;
pub mod logs;
pub mod namespace;
pub mod new;
pub mod queue;
pub mod run;
pub mod server;
pub mod status;
pub mod test;
pub mod workflow;

use crate::error::Result;
use crate::utils::GlobalConfig;

/// Common trait for all command handlers
pub trait CommandHandler {
    /// Execute the command with the given configuration
    fn execute(
        &self,
        global_config: &GlobalConfig,
    ) -> impl std::future::Future<Output = Result<()>>;
}

/// Common utilities for command implementations
pub mod common {
    use crate::error::{CliError, Result};
    use crate::utils::GlobalConfig;
    use std::path::Path;

    /// Load and parse a file as YAML or JSON
    pub fn load_resource_file(path: &Path) -> Result<serde_json::Value> {
        let content = std::fs::read_to_string(path).map_err(|e| {
            CliError::io_with_path("Failed to read file", path.display().to_string(), e)
        })?;

        // Try to parse as YAML first, then JSON
        if let Ok(yaml_value) = serde_yaml::from_str::<serde_json::Value>(&content) {
            Ok(yaml_value)
        } else if let Ok(json_value) = serde_json::from_str::<serde_json::Value>(&content) {
            Ok(json_value)
        } else {
            Err(CliError::serialization(
                "Failed to parse file as YAML or JSON",
                "YAML/JSON",
            ))
        }
    }

    /// Collect files from paths, handling directories recursively if requested
    pub fn collect_files(paths: Vec<String>, recursive: bool) -> Result<Vec<std::path::PathBuf>> {
        let mut files = Vec::new();

        for path_str in paths {
            let path = std::path::Path::new(&path_str);

            if path.is_file() {
                files.push(path.to_path_buf());
            } else if path.is_dir() && recursive {
                collect_files_from_dir(path, &mut files)?;
            } else if path.is_dir() {
                return Err(CliError::invalid_input(format!(
                    "Path is a directory but --recursive not specified: {}",
                    path.display()
                )));
            } else {
                return Err(CliError::not_found("file", path.display().to_string()));
            }
        }

        Ok(files)
    }

    fn collect_files_from_dir(dir: &Path, files: &mut Vec<std::path::PathBuf>) -> Result<()> {
        let entries = std::fs::read_dir(dir).map_err(|e| {
            CliError::io_with_path("Failed to read directory", dir.display().to_string(), e)
        })?;

        for entry in entries {
            let entry = entry.map_err(|e| {
                CliError::io_with_path(
                    "Failed to read directory entry",
                    dir.display().to_string(),
                    e,
                )
            })?;
            let path = entry.path();

            if path.is_file() {
                // Only include YAML and JSON files
                if let Some(extension) = path.extension() {
                    match extension.to_str() {
                        Some("yaml") | Some("yml") | Some("json") => {
                            files.push(path);
                        }
                        _ => continue,
                    }
                }
            } else if path.is_dir() {
                collect_files_from_dir(&path, files)?;
            }
        }

        Ok(())
    }

    /// Parse selector string (e.g., "app=my-app,version=1.0")
    pub fn parse_selector(selector: &str) -> Result<std::collections::HashMap<String, String>> {
        let mut labels = std::collections::HashMap::new();

        if selector.is_empty() {
            return Ok(labels);
        }

        for pair in selector.split(',') {
            let pair = pair.trim();
            if let Some(eq_pos) = pair.find('=') {
                let (key, value) = pair.split_at(eq_pos);
                let value = &value[1..]; // Remove the '=' character

                if key.is_empty() {
                    return Err(CliError::invalid_input(format!(
                        "Empty key in selector: {}",
                        pair
                    )));
                }

                labels.insert(key.to_string(), value.to_string());
            } else {
                return Err(CliError::invalid_input_with_details(
                    format!("Invalid selector format: {}", pair),
                    "selector",
                    "key=value,key2=value2",
                ));
            }
        }

        Ok(labels)
    }

    /// Confirm action with user
    pub fn confirm_action(message: &str, force: bool) -> Result<bool> {
        if force {
            return Ok(true);
        }

        use dialoguer::Confirm;
        use std::io::IsTerminal;

        // Without a terminal there is nobody to answer, so say how to confirm
        // instead of failing on the prompt.
        if !std::io::stdin().is_terminal() || !std::io::stderr().is_terminal() {
            return Err(CliError::invalid_input(format!(
                "{} Not running interactively: pass --force to confirm.",
                message
            )));
        }

        Confirm::new()
            .with_prompt(message)
            .default(false)
            .interact()
            .map_err(|e| CliError::internal(format!("Failed to get user confirmation: {}", e)))
    }

    /// Print a formatted message if not in quiet mode
    pub fn print_message(global_config: &GlobalConfig, message: &str) {
        if !global_config.quiet {
            println!("{}", message);
        }
    }

    /// Print a formatted error message
    pub fn print_error(global_config: &GlobalConfig, message: &str) {
        if global_config.use_colors() {
            eprintln!("{}", console::style(message).red());
        } else {
            eprintln!("{}", message);
        }
    }

    /// Print a formatted success message
    pub fn print_success(global_config: &GlobalConfig, message: &str) {
        if !global_config.quiet {
            if global_config.use_colors() {
                println!("{}", console::style(message).green());
            } else {
                println!("{}", message);
            }
        }
    }

    /// Print a formatted warning message
    pub fn print_warning(global_config: &GlobalConfig, message: &str) {
        if !global_config.quiet {
            if global_config.use_colors() {
                eprintln!("{}", console::style(message).yellow());
            } else {
                eprintln!("{}", message);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::common::*;
    use std::collections::HashMap;
    use tempfile::TempDir;

    #[test]
    fn test_parse_selector() {
        let result = parse_selector("app=my-app,version=1.0").unwrap();
        let mut expected = HashMap::new();
        expected.insert("app".to_string(), "my-app".to_string());
        expected.insert("version".to_string(), "1.0".to_string());
        assert_eq!(result, expected);

        let result = parse_selector("").unwrap();
        assert!(result.is_empty());

        assert!(parse_selector("invalid").is_err());
        assert!(parse_selector("=value").is_err());
    }

    #[test]
    fn test_collect_files() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let temp_path = temp_dir.path();

        // Create test files
        std::fs::write(temp_path.join("workflow.yaml"), "test: content")?;
        std::fs::write(temp_path.join("task.json"), r#"{"test": "content"}"#)?;
        std::fs::write(temp_path.join("readme.txt"), "Not a workflow file")?;

        let files = collect_files(vec![temp_path.to_string_lossy().to_string()], true)?;

        // Should only include YAML and JSON files
        assert_eq!(files.len(), 2);
        assert!(files
            .iter()
            .any(|f| f.file_name().unwrap() == "workflow.yaml"));
        assert!(files.iter().any(|f| f.file_name().unwrap() == "task.json"));

        Ok(())
    }

    #[test]
    fn test_load_resource_file() -> Result<(), Box<dyn std::error::Error>> {
        let temp_dir = TempDir::new()?;
        let yaml_file = temp_dir.path().join("test.yaml");
        let json_file = temp_dir.path().join("test.json");

        std::fs::write(&yaml_file, "name: test-workflow\ntype: sequential")?;
        std::fs::write(
            &json_file,
            r#"{"name": "test-workflow", "type": "sequential"}"#,
        )?;

        let yaml_result = load_resource_file(&yaml_file)?;
        assert_eq!(yaml_result["name"], "test-workflow");

        let json_result = load_resource_file(&json_file)?;
        assert_eq!(json_result["name"], "test-workflow");

        Ok(())
    }
}
