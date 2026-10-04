//! Input validation (resource names, config values, workflow definitions).

use crate::error::{CliError, Result, ValidationError, ValidationSeverity};
use regex::Regex;

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

/// Validate namespace name
pub fn validate_namespace_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(CliError::invalid_input("Namespace name cannot be empty"));
    }

    if name == "default" {
        return Ok(()); // Allow default namespace
    }

    validate_resource_name(name)
}

/// Validate label key
pub fn validate_label_key(key: &str) -> Result<()> {
    if key.is_empty() {
        return Err(CliError::invalid_input("Label key cannot be empty"));
    }

    if key.len() > 63 {
        return Err(CliError::invalid_input(
            "Label key must be 63 characters or fewer",
        ));
    }

    let re = Regex::new(r"^[a-zA-Z0-9]([a-zA-Z0-9._-]*[a-zA-Z0-9])?$").unwrap();
    if !re.is_match(key) {
        return Err(CliError::invalid_input(
            "Label key must start and end with alphanumeric character and can contain dots, dashes, and underscores",
        ));
    }

    Ok(())
}

/// Validate label value
pub fn validate_label_value(value: &str) -> Result<()> {
    if value.len() > 63 {
        return Err(CliError::invalid_input(
            "Label value must be 63 characters or fewer",
        ));
    }

    if value.is_empty() {
        return Ok(()); // Empty values are allowed
    }

    let re = Regex::new(r"^[a-zA-Z0-9]([a-zA-Z0-9._-]*[a-zA-Z0-9])?$").unwrap();
    if !re.is_match(value) {
        return Err(CliError::invalid_input(
            "Label value must start and end with alphanumeric character and can contain dots, dashes, and underscores",
        ));
    }

    Ok(())
}

/// Validate email address format
pub fn validate_email(email: &str) -> Result<()> {
    let re = Regex::new(r"^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$").unwrap();
    if !re.is_match(email) {
        return Err(CliError::invalid_input("Invalid email address format"));
    }
    Ok(())
}

/// Validate URL format
pub fn validate_url(url: &str) -> Result<()> {
    match url::Url::parse(url) {
        Ok(_) => Ok(()),
        Err(_) => Err(CliError::invalid_input("Invalid URL format")),
    }
}

/// Validate timeout format (e.g., "30s", "5m", "1h")
pub fn validate_timeout(timeout: &str) -> Result<()> {
    crate::utils::parse_duration(timeout)?;
    Ok(())
}

/// Validate CPU resource format (e.g., "100m", "1", "2.5")
pub fn validate_cpu_resource(cpu: &str) -> Result<()> {
    // CPU can be in millicores (100m) or cores (1, 1.5)
    if let Some(millis) = cpu.strip_suffix('m') {
        if millis.parse::<u32>().is_ok() {
            return Ok(());
        }
    } else if cpu.parse::<f64>().is_ok() {
        return Ok(());
    }

    Err(CliError::invalid_input(
        "CPU resource must be in format '100m' (millicores) or '1' (cores)",
    ))
}

/// Validate memory resource format (e.g., "128Mi", "1Gi", "512M")
pub fn validate_memory_resource(memory: &str) -> Result<()> {
    let re = Regex::new(r"^[0-9]+(\.[0-9]+)?(Ki|Mi|Gi|Ti|Pi|Ei|K|M|G|T|P|E)?$").unwrap();
    if !re.is_match(memory) {
        return Err(CliError::invalid_input(
            "Memory resource must be in format '128Mi', '1Gi', etc.",
        ));
    }
    Ok(())
}

/// Validate cron expression
pub fn validate_cron_expression(cron: &str) -> Result<()> {
    use cron::Schedule;
    use std::str::FromStr;

    Schedule::from_str(cron)
        .map_err(|e| CliError::invalid_input(format!("Invalid cron expression: {}", e)))?;

    Ok(())
}

/// Validate workflow definition structure
pub fn validate_workflow_definition(workflow: &serde_json::Value) -> Result<()> {
    let mut errors = Vec::new();

    // Check required fields
    if let Some(api_version) = workflow.get("apiVersion") {
        if api_version.as_str() != Some("orcher.io/v1") {
            errors.push(ValidationError {
                path: "apiVersion".to_string(),
                message: "Must be 'orcher.io/v1'".to_string(),
                severity: ValidationSeverity::Error,
                rule: Some("required_version".to_string()),
            });
        }
    } else {
        errors.push(ValidationError {
            path: "apiVersion".to_string(),
            message: "Required field missing".to_string(),
            severity: ValidationSeverity::Error,
            rule: Some("required".to_string()),
        });
    }

    if let Some(kind) = workflow.get("kind") {
        if kind.as_str() != Some("Workflow") {
            errors.push(ValidationError {
                path: "kind".to_string(),
                message: "Must be 'Workflow'".to_string(),
                severity: ValidationSeverity::Error,
                rule: Some("required_kind".to_string()),
            });
        }
    } else {
        errors.push(ValidationError {
            path: "kind".to_string(),
            message: "Required field missing".to_string(),
            severity: ValidationSeverity::Error,
            rule: Some("required".to_string()),
        });
    }

    // Validate metadata
    if let Some(metadata) = workflow.get("metadata") {
        validate_metadata(metadata, &mut errors, "metadata");
    } else {
        errors.push(ValidationError {
            path: "metadata".to_string(),
            message: "Required field missing".to_string(),
            severity: ValidationSeverity::Error,
            rule: Some("required".to_string()),
        });
    }

    // Validate spec
    if let Some(spec) = workflow.get("spec") {
        validate_workflow_spec(spec, &mut errors, "spec");
    } else {
        errors.push(ValidationError {
            path: "spec".to_string(),
            message: "Required field missing".to_string(),
            severity: ValidationSeverity::Error,
            rule: Some("required".to_string()),
        });
    }

    if !errors.is_empty() {
        return Err(CliError::validation_structured(
            "Workflow validation failed",
            errors,
        ));
    }

    Ok(())
}

/// Validate task definition structure
pub fn validate_task_definition(task: &serde_json::Value) -> Result<()> {
    let mut errors = Vec::new();

    // Check required fields
    if let Some(api_version) = task.get("apiVersion") {
        if api_version.as_str() != Some("orcher.io/v1") {
            errors.push(ValidationError {
                path: "apiVersion".to_string(),
                message: "Must be 'orcher.io/v1'".to_string(),
                severity: ValidationSeverity::Error,
                rule: Some("required_version".to_string()),
            });
        }
    } else {
        errors.push(ValidationError {
            path: "apiVersion".to_string(),
            message: "Required field missing".to_string(),
            severity: ValidationSeverity::Error,
            rule: Some("required".to_string()),
        });
    }

    if let Some(kind) = task.get("kind") {
        if kind.as_str() != Some("Task") {
            errors.push(ValidationError {
                path: "kind".to_string(),
                message: "Must be 'Task'".to_string(),
                severity: ValidationSeverity::Error,
                rule: Some("required_kind".to_string()),
            });
        }
    } else {
        errors.push(ValidationError {
            path: "kind".to_string(),
            message: "Required field missing".to_string(),
            severity: ValidationSeverity::Error,
            rule: Some("required".to_string()),
        });
    }

    // Validate metadata
    if let Some(metadata) = task.get("metadata") {
        validate_metadata(metadata, &mut errors, "metadata");
    } else {
        errors.push(ValidationError {
            path: "metadata".to_string(),
            message: "Required field missing".to_string(),
            severity: ValidationSeverity::Error,
            rule: Some("required".to_string()),
        });
    }

    // Validate spec
    if let Some(spec) = task.get("spec") {
        validate_task_spec(spec, &mut errors, "spec");
    } else {
        errors.push(ValidationError {
            path: "spec".to_string(),
            message: "Required field missing".to_string(),
            severity: ValidationSeverity::Error,
            rule: Some("required".to_string()),
        });
    }

    if !errors.is_empty() {
        return Err(CliError::validation_structured(
            "Task validation failed",
            errors,
        ));
    }

    Ok(())
}

/// Validate metadata section
fn validate_metadata(
    metadata: &serde_json::Value,
    errors: &mut Vec<ValidationError>,
    path_prefix: &str,
) {
    if let Some(name) = metadata.get("name") {
        if let Some(name_str) = name.as_str() {
            if let Err(e) = validate_resource_name(name_str) {
                errors.push(ValidationError {
                    path: format!("{}.name", path_prefix),
                    message: e.to_string(),
                    severity: ValidationSeverity::Error,
                    rule: Some("resource_name".to_string()),
                });
            }
        } else {
            errors.push(ValidationError {
                path: format!("{}.name", path_prefix),
                message: "Name must be a string".to_string(),
                severity: ValidationSeverity::Error,
                rule: Some("type".to_string()),
            });
        }
    } else {
        errors.push(ValidationError {
            path: format!("{}.name", path_prefix),
            message: "Required field missing".to_string(),
            severity: ValidationSeverity::Error,
            rule: Some("required".to_string()),
        });
    }

    // Validate labels if present
    if let Some(labels) = metadata.get("labels") {
        if let Some(labels_obj) = labels.as_object() {
            for (key, value) in labels_obj {
                if let Err(e) = validate_label_key(key) {
                    errors.push(ValidationError {
                        path: format!("{}.labels.{}", path_prefix, key),
                        message: e.to_string(),
                        severity: ValidationSeverity::Error,
                        rule: Some("label_key".to_string()),
                    });
                }

                if let Some(value_str) = value.as_str() {
                    if let Err(e) = validate_label_value(value_str) {
                        errors.push(ValidationError {
                            path: format!("{}.labels.{}", path_prefix, key),
                            message: e.to_string(),
                            severity: ValidationSeverity::Error,
                            rule: Some("label_value".to_string()),
                        });
                    }
                } else {
                    errors.push(ValidationError {
                        path: format!("{}.labels.{}", path_prefix, key),
                        message: "Label value must be a string".to_string(),
                        severity: ValidationSeverity::Error,
                        rule: Some("type".to_string()),
                    });
                }
            }
        }
    }
}

/// Validate workflow spec section
fn validate_workflow_spec(
    spec: &serde_json::Value,
    errors: &mut Vec<ValidationError>,
    path_prefix: &str,
) {
    // Validate strategy
    if let Some(strategy) = spec.get("strategy") {
        if let Some(strategy_str) = strategy.as_str() {
            let valid_strategies = ["sequential", "parallel", "dag", "conditional"];
            if !valid_strategies.contains(&strategy_str) {
                errors.push(ValidationError {
                    path: format!("{}.strategy", path_prefix),
                    message: format!(
                        "Invalid strategy '{}'. Must be one of: {}",
                        strategy_str,
                        valid_strategies.join(", ")
                    ),
                    severity: ValidationSeverity::Error,
                    rule: Some("enum".to_string()),
                });
            }
        }
    }

    // Validate timeout if present
    if let Some(timeout) = spec.get("timeout") {
        if let Some(timeout_str) = timeout.as_str() {
            if let Err(e) = validate_timeout(timeout_str) {
                errors.push(ValidationError {
                    path: format!("{}.timeout", path_prefix),
                    message: e.to_string(),
                    severity: ValidationSeverity::Error,
                    rule: Some("duration".to_string()),
                });
            }
        }
    }

    // Validate steps
    if let Some(steps) = spec.get("steps") {
        if let Some(steps_array) = steps.as_array() {
            if steps_array.is_empty() {
                errors.push(ValidationError {
                    path: format!("{}.steps", path_prefix),
                    message: "Workflow must have at least one step".to_string(),
                    severity: ValidationSeverity::Error,
                    rule: Some("min_length".to_string()),
                });
            }

            for (i, step) in steps_array.iter().enumerate() {
                validate_workflow_step(step, errors, &format!("{}.steps[{}]", path_prefix, i));
            }
        }
    }
}

/// Validate workflow step
fn validate_workflow_step(
    step: &serde_json::Value,
    errors: &mut Vec<ValidationError>,
    path_prefix: &str,
) {
    // Validate step name
    if let Some(name) = step.get("name") {
        if let Some(name_str) = name.as_str() {
            if let Err(e) = validate_resource_name(name_str) {
                errors.push(ValidationError {
                    path: format!("{}.name", path_prefix),
                    message: e.to_string(),
                    severity: ValidationSeverity::Error,
                    rule: Some("resource_name".to_string()),
                });
            }
        }
    } else {
        errors.push(ValidationError {
            path: format!("{}.name", path_prefix),
            message: "Step name is required".to_string(),
            severity: ValidationSeverity::Error,
            rule: Some("required".to_string()),
        });
    }

    // Must have either task or parallel
    let has_task = step.get("task").is_some();
    let has_parallel = step.get("parallel").is_some();

    if !has_task && !has_parallel {
        errors.push(ValidationError {
            path: path_prefix.to_string(),
            message: "Step must have either 'task' or 'parallel' field".to_string(),
            severity: ValidationSeverity::Error,
            rule: Some("required_one_of".to_string()),
        });
    }

    if has_task && has_parallel {
        errors.push(ValidationError {
            path: path_prefix.to_string(),
            message: "Step cannot have both 'task' and 'parallel' fields".to_string(),
            severity: ValidationSeverity::Error,
            rule: Some("exclusive".to_string()),
        });
    }
}

/// Validate task spec section
fn validate_task_spec(
    spec: &serde_json::Value,
    errors: &mut Vec<ValidationError>,
    path_prefix: &str,
) {
    // Task spec validation depends on the runtime type
    // For now, just check that it's an object
    if !spec.is_object() {
        errors.push(ValidationError {
            path: path_prefix.to_string(),
            message: "Task spec must be an object".to_string(),
            severity: ValidationSeverity::Error,
            rule: Some("type".to_string()),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
    fn test_validate_email() {
        assert!(validate_email("user@example.com").is_ok());
        assert!(validate_email("test.email+tag@example.org").is_ok());

        assert!(validate_email("invalid-email").is_err());
        assert!(validate_email("@example.com").is_err());
        assert!(validate_email("user@").is_err());
    }

    #[test]
    fn test_validate_url() {
        assert!(validate_url("https://example.com").is_ok());
        assert!(validate_url("http://localhost:8080").is_ok());
        assert!(validate_url("ftp://ftp.example.com").is_ok());

        assert!(validate_url("not-a-url").is_err());
        assert!(validate_url("://invalid").is_err());
    }

    #[test]
    fn test_validate_cpu_resource() {
        assert!(validate_cpu_resource("100m").is_ok());
        assert!(validate_cpu_resource("1").is_ok());
        assert!(validate_cpu_resource("2.5").is_ok());

        assert!(validate_cpu_resource("invalid").is_err());
        assert!(validate_cpu_resource("100").is_ok()); // Valid as cores
    }

    #[test]
    fn test_validate_memory_resource() {
        assert!(validate_memory_resource("128Mi").is_ok());
        assert!(validate_memory_resource("1Gi").is_ok());
        assert!(validate_memory_resource("512M").is_ok());

        assert!(validate_memory_resource("invalid").is_err());
        assert!(validate_memory_resource("128").is_ok()); // Valid without unit
    }

    #[test]
    fn test_validate_workflow_definition() {
        let valid_workflow = json!({
            "apiVersion": "orcher.io/v1",
            "kind": "Workflow",
            "metadata": {
                "name": "test-workflow"
            },
            "spec": {
                "strategy": "sequential",
                "steps": [
                    {
                        "name": "test-step",
                        "task": {
                            "name": "test-task"
                        }
                    }
                ]
            }
        });

        assert!(validate_workflow_definition(&valid_workflow).is_ok());

        let invalid_workflow = json!({
            "apiVersion": "wrong/v1",
            "kind": "NotWorkflow"
        });

        assert!(validate_workflow_definition(&invalid_workflow).is_err());
    }

    #[test]
    fn test_validate_cron_expression() {
        // The cron crate uses 7 fields: sec min hour day-of-month month day-of-week year
        assert!(validate_cron_expression("0 0 0 * * * *").is_ok()); // Daily at midnight
        assert!(validate_cron_expression("0 0 */6 * * * *").is_ok()); // Every 6 hours
        assert!(validate_cron_expression("0 0 9 * * 1-5 *").is_ok()); // Weekdays at 9 AM

        assert!(validate_cron_expression("invalid cron").is_err());
        assert!(validate_cron_expression("0 60 0 * * * *").is_err()); // Invalid minute (60)
    }
}
