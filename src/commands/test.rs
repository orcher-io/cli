//! `orcher test` — build, check, and run workflow project tests.

use crate::client::connection::{ConnectionManager, ServerType};
use crate::error::{CliError, Result};
use crate::utils::GlobalConfig;
use console::style;
use std::path::Path;
use std::process::{Command, Stdio};
use tracing::{debug, info};

/// Test mode for workflow testing
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestMode {
    /// Run the service and execute workflows (E2E)
    E2E,
    /// Run only the worker (service) side
    Worker,
    /// Run only the client side (start workflows)
    Client,
    /// Validate registration without execution
    Check,
}

impl std::fmt::Display for TestMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TestMode::E2E => write!(f, "e2e"),
            TestMode::Worker => write!(f, "worker"),
            TestMode::Client => write!(f, "client"),
            TestMode::Check => write!(f, "check"),
        }
    }
}

pub async fn execute(
    workflow: String,
    dry_run: bool,
    environment: Option<String>,
    watch: bool,
    global_config: &GlobalConfig,
) -> Result<()> {
    info!(
        "Testing workflow project: {} (dry_run: {}, watch: {})",
        workflow, dry_run, watch
    );

    let project_path = Path::new(&workflow);

    if project_path.is_dir() {
        // Testing a Cargo project directory
        test_cargo_project(
            project_path,
            dry_run,
            environment.as_deref(),
            watch,
            global_config,
        )
        .await
    } else if project_path.extension().map(|e| e == "rs").unwrap_or(false) {
        // Testing a specific Rust file - not directly supported
        println!(
            "{}  Orcher uses code-first workflows. To test, provide the project directory.",
            style("•").yellow()
        );
        println!();
        println!("   Usage:");
        println!(
            "     {}  Test current directory",
            style("orcher test .").cyan()
        );
        println!(
            "     {}  Test specific project",
            style("orcher test ./my-workflow").cyan()
        );
        println!(
            "     {}  Check registration only",
            style("orcher test . --dry-run").cyan()
        );
        Ok(())
    } else if workflow == "." || workflow.is_empty() {
        // Test current directory
        test_cargo_project(
            Path::new("."),
            dry_run,
            environment.as_deref(),
            watch,
            global_config,
        )
        .await
    } else {
        // Assume it's a project directory
        test_cargo_project(
            project_path,
            dry_run,
            environment.as_deref(),
            watch,
            global_config,
        )
        .await
    }
}

async fn test_cargo_project(
    project_path: &Path,
    dry_run: bool,
    environment: Option<&str>,
    watch: bool,
    global_config: &GlobalConfig,
) -> Result<()> {
    // Validate it's a Cargo project
    let cargo_toml = project_path.join("Cargo.toml");
    if !cargo_toml.exists() {
        return Err(CliError::NotFound {
            resource_type: "Cargo project".to_string(),
            name: project_path.display().to_string(),
            namespace: None,
        });
    }

    if !global_config.quiet {
        println!(
            "{}  Testing Orcher workflow project: {}",
            style("→").cyan(),
            project_path.display()
        );
    }

    // Determine test mode
    let mode = if dry_run {
        TestMode::Check
    } else {
        TestMode::E2E
    };

    match mode {
        TestMode::Check => {
            // Just build and check - validates workflow registration
            run_cargo_check(project_path, global_config).await
        }
        TestMode::E2E => {
            // Run cargo test for the project
            if watch {
                run_cargo_watch_test(project_path, environment, global_config).await
            } else {
                run_cargo_test(project_path, environment, global_config).await
            }
        }
        TestMode::Worker | TestMode::Client => {
            // These would be implemented for more granular testing
            run_cargo_test(project_path, environment, global_config).await
        }
    }
}

/// Run `cargo check` to validate workflow registration
async fn run_cargo_check(project_path: &Path, global_config: &GlobalConfig) -> Result<()> {
    if !global_config.quiet {
        println!(
            "{}  Checking workflow registration (cargo check)...",
            style("→").cyan()
        );
    }

    let output = Command::new("cargo")
        .arg("check")
        .current_dir(project_path)
        .stdout(if global_config.quiet {
            Stdio::null()
        } else {
            Stdio::inherit()
        })
        .stderr(if global_config.quiet {
            Stdio::null()
        } else {
            Stdio::inherit()
        })
        .output()
        .map_err(|e| CliError::IO {
            message: format!("Failed to run cargo check: {}", e),
            path: Some(project_path.to_path_buf()),
        })?;

    if output.status.success() {
        if !global_config.quiet {
            println!();
            println!(
                "{}  Workflow project compiles successfully",
                style("✓").green()
            );
            println!();
            println!("   Workflows and tasks with #[workflow] and #[task] macros are valid.");
            println!("   Run {} to execute tests.", style("orcher test .").cyan());
        }
        Ok(())
    } else {
        Err(CliError::Execution {
            message: "Cargo check failed - workflow registration errors".to_string(),
            execution_id: None,
        })
    }
}

/// Run `cargo test` to execute workflow tests
async fn run_cargo_test(
    project_path: &Path,
    environment: Option<&str>,
    global_config: &GlobalConfig,
) -> Result<()> {
    if !global_config.quiet {
        println!(
            "{}  Running workflow tests (cargo test)...",
            style("→").cyan()
        );
        println!();
    }

    let mut cmd = Command::new("cargo");
    cmd.arg("test");
    cmd.current_dir(project_path);

    // Set environment for tests
    if let Some(env) = environment {
        cmd.env("ORCHER_ENV", env);
    }

    // Check if server is available and set connection info
    let manager = ConnectionManager::from_config(global_config);
    if let Ok(server_type) = manager.detect_best_server().await {
        match server_type {
            ServerType::Grpc => {
                cmd.env("ORCHER_SERVER_URL", manager.grpc_addr());
            }
            ServerType::Http => {
                cmd.env("ORCHER_HTTP_URL", manager.http_addr());
            }
        }
    }

    // Run with output
    cmd.stdout(Stdio::inherit());
    cmd.stderr(Stdio::inherit());

    let status = cmd.status().map_err(|e| CliError::IO {
        message: format!("Failed to run cargo test: {}", e),
        path: Some(project_path.to_path_buf()),
    })?;

    if status.success() {
        if !global_config.quiet {
            println!();
            println!("{}  All workflow tests passed", style("✓").green());
        }
        Ok(())
    } else {
        Err(CliError::Execution {
            message: "Workflow tests failed".to_string(),
            execution_id: None,
        })
    }
}

/// Run `cargo watch` for continuous testing
async fn run_cargo_watch_test(
    project_path: &Path,
    environment: Option<&str>,
    global_config: &GlobalConfig,
) -> Result<()> {
    // Check if cargo-watch is installed
    let watch_check = Command::new("cargo").args(["watch", "--version"]).output();

    if watch_check.is_err() || !watch_check.unwrap().status.success() {
        if !global_config.quiet {
            println!(
                "{}  cargo-watch not installed. Install with:",
                style("⚠").yellow()
            );
            println!("     {}", style("cargo install cargo-watch").cyan());
            println!();
            println!("   Running single test instead...");
        }
        return run_cargo_test(project_path, environment, global_config).await;
    }

    if !global_config.quiet {
        println!(
            "{}  Watching for changes (press Ctrl+C to stop)...",
            style("→").cyan()
        );
        println!();
    }

    let mut cmd = Command::new("cargo");
    cmd.args(["watch", "-x", "test"]);
    cmd.current_dir(project_path);

    if let Some(env) = environment {
        cmd.env("ORCHER_ENV", env);
    }

    cmd.stdout(Stdio::inherit());
    cmd.stderr(Stdio::inherit());

    let mut child = cmd.spawn().map_err(|e| CliError::IO {
        message: format!("Failed to run cargo watch: {}", e),
        path: Some(project_path.to_path_buf()),
    })?;

    // Wait for Ctrl+C
    tokio::signal::ctrl_c().await.ok();

    let _ = child.kill();
    let _ = child.wait();

    if !global_config.quiet {
        println!();
        println!("{}  Watch mode stopped", style("•").yellow());
    }

    Ok(())
}

/// Run a workflow service binary in E2E mode
#[allow(dead_code)]
async fn run_e2e_test(project_path: &Path, global_config: &GlobalConfig) -> Result<()> {
    debug!("Running E2E test for project: {}", project_path.display());

    // Build the project first
    if !global_config.quiet {
        println!("{}  Building workflow service...", style("→").cyan());
    }

    let build_output = Command::new("cargo")
        .arg("build")
        .current_dir(project_path)
        .output()
        .map_err(|e| CliError::IO {
            message: format!("Failed to build project: {}", e),
            path: Some(project_path.to_path_buf()),
        })?;

    if !build_output.status.success() {
        return Err(CliError::Execution {
            message: "Failed to build workflow service".to_string(),
            execution_id: None,
        });
    }

    // Run the binary with --e2e flag (convention for Orcher services)
    if !global_config.quiet {
        println!("{}  Running E2E tests...", style("→").cyan());
    }

    let run_output = Command::new("cargo")
        .args(["run", "--", "--e2e"])
        .current_dir(project_path)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|e| CliError::IO {
            message: format!("Failed to run E2E tests: {}", e),
            path: Some(project_path.to_path_buf()),
        })?;

    if run_output.success() {
        if !global_config.quiet {
            println!();
            println!("{}  E2E tests passed", style("✓").green());
        }
        Ok(())
    } else {
        Err(CliError::Execution {
            message: "E2E tests failed".to_string(),
            execution_id: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mode_display() {
        assert_eq!(TestMode::E2E.to_string(), "e2e");
        assert_eq!(TestMode::Worker.to_string(), "worker");
        assert_eq!(TestMode::Client.to_string(), "client");
        assert_eq!(TestMode::Check.to_string(), "check");
    }
}
