//! `orcher test` — check a project, or run its workflows end to end.
//!
//! The project's language is detected from its files (see
//! [`Language::detect`]). `--dry-run` only checks that it builds; otherwise
//! its own test runs against the engine the CLI points at, which is passed on
//! as `ORCHER_SERVER`, `ORCHER_NAMESPACE` and, when set, `ORCHER_API_KEY`.

use crate::client::connection::ConnectionManager;
use crate::commands::new::Language;
use crate::error::{CliError, Result};
use crate::utils::GlobalConfig;
use console::style;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tracing::info;

/// Test mode for workflow testing
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestMode {
    /// Run the project's test, which runs its workflows against the engine
    E2E,
    /// Run only the worker (service) side
    Worker,
    /// Run only the client side (start workflows)
    Client,
    /// Check that the project builds, without running anything
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

    let project_path = if workflow.is_empty() {
        PathBuf::from(".")
    } else {
        PathBuf::from(&workflow)
    };
    if !project_path.is_dir() {
        return Err(CliError::invalid_input(format!(
            "'{}' is not a project directory. Run `orcher test` in a project made with \
             `orcher new`, or pass its directory.",
            workflow
        )));
    }
    let language = Language::detect(&project_path).ok_or_else(|| {
        CliError::invalid_input(format!(
            "No Python, TypeScript or Rust project in '{}'.",
            project_path.display()
        ))
    })?;

    let mode = if dry_run {
        TestMode::Check
    } else {
        TestMode::E2E
    };
    if !global_config.quiet {
        println!(
            "{}  {} {} project in {}",
            style("→").cyan(),
            if mode == TestMode::Check {
                "Checking"
            } else {
                "Testing"
            },
            language.name(),
            project_path.display()
        );
    }

    let manager = ConnectionManager::from_config(global_config);
    let namespace = environment.unwrap_or_else(|| manager.namespace().to_string());

    let steps = match mode {
        TestMode::Check => check_commands(language, &project_path),
        _ => {
            // The test needs an engine; say so plainly rather than let it time out.
            let status = manager.check_grpc().await;
            if !status.available {
                return Err(CliError::Network {
                    message: format!(
                        "No engine answers at {}: the project's test runs its workflows on one.\n\n\
                         Start one with: orcher dev start   (or check it builds with --dry-run)",
                        manager.grpc_addr()
                    ),
                    source: None,
                });
            }
            if watch {
                if language == Language::Rust && cargo_watch_installed() {
                    vec![step(
                        "cargo",
                        &["watch", "-x", "test"],
                        "Watching and testing",
                    )]
                } else {
                    if !global_config.quiet {
                        println!(
                            "{}  --watch needs cargo-watch and a Rust project; testing once.",
                            style("•").yellow()
                        );
                    }
                    test_commands(language, &project_path)
                }
            } else {
                test_commands(language, &project_path)
            }
        }
    };

    for s in steps {
        if !global_config.quiet {
            println!("{}  {}: {}", style("→").cyan(), s.label, s.display());
        }
        let mut cmd = Command::new(&s.program);
        cmd.args(&s.args)
            .current_dir(&project_path)
            .env("ORCHER_SERVER", manager.grpc_addr())
            .env("ORCHER_NAMESPACE", &namespace)
            .env("ORCHER_ENV", &namespace)
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        let status = cmd.status().map_err(|e| CliError::IO {
            message: format!("Failed to run {}: {}", s.program, e),
            path: Some(project_path.clone()),
        })?;
        if !status.success() {
            return Err(CliError::Execution {
                message: format!("{} failed ({})", s.display(), status),
                execution_id: None,
            });
        }
    }

    if !global_config.quiet {
        println!();
        println!(
            "{}  {}",
            style("✓").green(),
            if mode == TestMode::Check {
                "The project builds"
            } else {
                "The project's workflows ran end to end"
            }
        );
    }
    Ok(())
}

/// One command to run in the project.
struct Step {
    program: String,
    args: Vec<String>,
    label: &'static str,
}

impl Step {
    fn display(&self) -> String {
        std::iter::once(self.program.as_str())
            .chain(self.args.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

fn step(program: &str, args: &[&str], label: &'static str) -> Step {
    Step {
        program: program.to_string(),
        args: args.iter().map(|a| a.to_string()).collect(),
        label,
    }
}

/// The project's virtual environment's Python when it has one, else python3.
fn python(project: &Path) -> String {
    let venv = if cfg!(windows) {
        project.join(".venv").join("Scripts").join("python.exe")
    } else {
        project.join(".venv").join("bin").join("python")
    };
    if venv.is_file() {
        // Relative to the project, which is where the command runs.
        if cfg!(windows) {
            ".venv\\Scripts\\python.exe".to_string()
        } else {
            ".venv/bin/python".to_string()
        }
    } else {
        "python3".to_string()
    }
}

/// How to check that a project builds.
fn check_commands(language: Language, project: &Path) -> Vec<Step> {
    match language {
        Language::Python => vec![step(
            &python(project),
            &["-m", "compileall", "-q", "-x", r"[/\\]\.venv", "."],
            "Compiling",
        )],
        Language::TypeScript => vec![step("npm", &["run", "build"], "Building")],
        Language::Rust => vec![step("cargo", &["check", "--all-targets"], "Checking")],
    }
}

/// How to run a project's test.
fn test_commands(language: Language, project: &Path) -> Vec<Step> {
    match language {
        Language::Python if project.join("test_workflow.py").is_file() => {
            vec![step(&python(project), &["test_workflow.py"], "Running")]
        }
        Language::Python => vec![step(&python(project), &["-m", "pytest", "-q"], "Running")],
        Language::TypeScript => vec![step("npm", &["test"], "Running")],
        // --nocapture, so that the test's report is seen as with the others
        Language::Rust => vec![step("cargo", &["test", "--", "--nocapture"], "Running")],
    }
}

fn cargo_watch_installed() -> bool {
    Command::new("cargo")
        .args(["watch", "--version"])
        .output()
        .is_ok_and(|o| o.status.success())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_mode_display() {
        assert_eq!(TestMode::E2E.to_string(), "e2e");
        assert_eq!(TestMode::Worker.to_string(), "worker");
        assert_eq!(TestMode::Client.to_string(), "client");
        assert_eq!(TestMode::Check.to_string(), "check");
    }

    #[test]
    fn each_language_is_checked_and_tested_its_own_way() {
        let dir = TempDir::new().unwrap();
        let p = dir.path();
        assert_eq!(
            check_commands(Language::Rust, p)[0].display(),
            "cargo check --all-targets"
        );
        assert_eq!(
            test_commands(Language::Rust, p)[0].display(),
            "cargo test -- --nocapture"
        );
        assert_eq!(
            check_commands(Language::TypeScript, p)[0].display(),
            "npm run build"
        );
        assert_eq!(
            test_commands(Language::TypeScript, p)[0].display(),
            "npm test"
        );
        assert_eq!(
            test_commands(Language::Python, p)[0].display(),
            "python3 -m pytest -q"
        );
        std::fs::write(p.join("test_workflow.py"), "").unwrap();
        assert_eq!(
            test_commands(Language::Python, p)[0].display(),
            "python3 test_workflow.py"
        );
    }

    #[test]
    fn a_project_virtual_environment_is_used() {
        let dir = TempDir::new().unwrap();
        assert_eq!(python(dir.path()), "python3");
        let bin = dir.path().join(".venv").join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join("python"), "").unwrap();
        if !cfg!(windows) {
            assert_eq!(python(dir.path()), ".venv/bin/python");
        }
    }
}
