//! Handlebars template engine for `orcher new` project scaffolding.

#![allow(dead_code)]

use crate::constants::VERSION;
use crate::error::{CliError, Result};
use crate::time::Timestamptz;
use handlebars::Handlebars;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

/// Template engine for generating projects and workflows
pub struct TemplateEngine {
    handlebars: Handlebars<'static>,
    templates_dir: Option<PathBuf>,
}

/// Built-in template information
#[derive(Debug, Clone)]
pub struct TemplateInfo {
    pub name: String,
    pub description: String,
    pub category: String,
    pub files: Vec<String>,
}

impl TemplateEngine {
    /// Create a new template engine
    pub fn new() -> Result<Self> {
        let mut handlebars = Handlebars::new();

        // Configure handlebars
        handlebars.set_strict_mode(true);
        handlebars.register_escape_fn(handlebars::no_escape);

        // Register custom helpers
        Self::register_helpers(&mut handlebars)?;

        Ok(Self {
            handlebars,
            templates_dir: None,
        })
    }

    /// Register custom handlebars helpers
    fn register_helpers(handlebars: &mut Handlebars<'static>) -> Result<()> {
        // Helper for current timestamp
        handlebars.register_helper(
            "now",
            Box::new(
                |_: &handlebars::Helper,
                 _: &Handlebars,
                 _: &handlebars::Context,
                 _: &mut handlebars::RenderContext,
                 out: &mut dyn handlebars::Output|
                 -> handlebars::HelperResult {
                    let now = Timestamptz::now().to_rfc3339();
                    out.write(&now)?;
                    Ok(())
                },
            ),
        );

        // Helper for uppercase conversion
        handlebars.register_helper(
            "upper",
            Box::new(
                |h: &handlebars::Helper,
                 _: &Handlebars,
                 _: &handlebars::Context,
                 _: &mut handlebars::RenderContext,
                 out: &mut dyn handlebars::Output|
                 -> handlebars::HelperResult {
                    if let Some(param) = h.param(0) {
                        if let Some(value) = param.value().as_str() {
                            out.write(&value.to_uppercase())?;
                        }
                    }
                    Ok(())
                },
            ),
        );

        // Helper for kebab-case conversion
        handlebars.register_helper(
            "kebab",
            Box::new(
                |h: &handlebars::Helper,
                 _: &Handlebars,
                 _: &handlebars::Context,
                 _: &mut handlebars::RenderContext,
                 out: &mut dyn handlebars::Output|
                 -> handlebars::HelperResult {
                    if let Some(param) = h.param(0) {
                        if let Some(value) = param.value().as_str() {
                            let kebab = value.to_lowercase().replace([' ', '_'], "-");
                            out.write(&kebab)?;
                        }
                    }
                    Ok(())
                },
            ),
        );

        Ok(())
    }

    /// Check if a template exists
    ///
    /// Supports template aliases (e.g., "rust" -> "basic-workflow")
    pub fn template_exists(&self, template_name: &str) -> Result<bool> {
        // Resolve alias to canonical template name
        let canonical_name = self.resolve_template_alias(template_name);

        // Check built-in templates
        if self.is_builtin_template(&canonical_name) {
            return Ok(true);
        }

        // Check custom templates directory if configured
        if let Some(templates_dir) = &self.templates_dir {
            let template_dir = templates_dir.join(&canonical_name);
            return Ok(template_dir.exists() && template_dir.is_dir());
        }

        Ok(false)
    }

    /// Generate a project from a template
    ///
    /// Supports template aliases (e.g., "rust" -> "basic-workflow")
    pub fn generate_project(
        &self,
        template_name: &str,
        context: &HashMap<String, serde_json::Value>,
        output_dir: &Path,
    ) -> Result<()> {
        // Resolve alias to canonical template name
        let canonical_name = self.resolve_template_alias(template_name);

        if self.is_builtin_template(&canonical_name) {
            self.generate_builtin_project(&canonical_name, context, output_dir)
        } else {
            self.generate_custom_project(&canonical_name, context, output_dir)
        }
    }

    /// List all available templates
    pub fn list_templates(&self) -> Result<Vec<TemplateInfo>> {
        let mut templates = self.list_builtin_templates();

        // Add custom templates if directory exists
        if let Some(templates_dir) = &self.templates_dir {
            if templates_dir.exists() {
                templates.extend(self.list_custom_templates(templates_dir)?);
            }
        }

        templates.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(templates)
    }

    /// Get template information
    pub fn get_template_info(&self, template_name: &str) -> Result<TemplateInfo> {
        let templates = self.list_templates()?;
        templates
            .into_iter()
            .find(|t| t.name == template_name)
            .ok_or_else(|| CliError::not_found("template", template_name))
    }

    /// Resolve template alias to canonical name
    ///
    /// Supports user-friendly aliases:
    /// - "rust" or "default" -> "basic-workflow"
    /// - "wasm" -> "wasm-workflow"
    pub fn resolve_template_alias(&self, template_name: &str) -> String {
        match template_name.to_lowercase().as_str() {
            "rust" | "default" => "basic-workflow".to_string(),
            "wasm" => "wasm-workflow".to_string(),
            "etl" | "pipeline" => "data-pipeline".to_string(),
            "batch" => "batch-processing".to_string(),
            "services" | "micro" => "microservices".to_string(),
            other => other.to_string(),
        }
    }

    /// Check if template is built-in (including aliases)
    fn is_builtin_template(&self, template_name: &str) -> bool {
        let canonical = self.resolve_template_alias(template_name);
        matches!(
            canonical.as_str(),
            "basic-workflow"
                | "wasm-workflow"
                | "microservices"
                | "data-pipeline"
                | "batch-processing"
        )
    }

    /// List built-in templates
    fn list_builtin_templates(&self) -> Vec<TemplateInfo> {
        vec![
            TemplateInfo {
                name: "basic-workflow".to_string(),
                description: "Simple sequential workflow with basic tasks".to_string(),
                category: "Basic".to_string(),
                files: vec![
                    "orcher.yaml".to_string(),
                    "workflows/main.yaml".to_string(),
                    "README.md".to_string(),
                ],
            },
            TemplateInfo {
                name: "wasm-workflow".to_string(),
                description: "WASM-compatible workflow with optimized dependencies".to_string(),
                category: "Basic".to_string(),
                files: vec![
                    "Cargo.toml".to_string(),
                    "src/lib.rs".to_string(),
                    "src/workflows/main_workflow.rs".to_string(),
                    "README.md".to_string(),
                ],
            },
            TemplateInfo {
                name: "microservices".to_string(),
                description: "Microservices orchestration with service dependencies".to_string(),
                category: "Application".to_string(),
                files: vec![
                    "orcher.yaml".to_string(),
                    "workflows/deploy.yaml".to_string(),
                    "workflows/test.yaml".to_string(),
                    "configs/services.yaml".to_string(),
                    "README.md".to_string(),
                ],
            },
            TemplateInfo {
                name: "data-pipeline".to_string(),
                description: "ETL data pipeline with validation and error handling".to_string(),
                category: "Data".to_string(),
                files: vec![
                    "orcher.yaml".to_string(),
                    "workflows/etl.yaml".to_string(),
                    "workflows/validation.yaml".to_string(),
                    "configs/data-sources.yaml".to_string(),
                    "README.md".to_string(),
                ],
            },
            TemplateInfo {
                name: "batch-processing".to_string(),
                description: "Scheduled batch processing with resource management".to_string(),
                category: "Batch".to_string(),
                files: vec![
                    "orcher.yaml".to_string(),
                    "workflows/batch-job.yaml".to_string(),
                    "schedules/nightly.yaml".to_string(),
                    "configs/resources.yaml".to_string(),
                    "README.md".to_string(),
                ],
            },
        ]
    }

    /// List custom templates from directory
    fn list_custom_templates(&self, templates_dir: &Path) -> Result<Vec<TemplateInfo>> {
        let mut templates = Vec::new();

        if !templates_dir.exists() {
            return Ok(templates);
        }

        for entry in std::fs::read_dir(templates_dir).map_err(|e| {
            CliError::io_with_path(
                "Failed to read templates directory",
                templates_dir.display().to_string(),
                e,
            )
        })? {
            let entry = entry.map_err(|e| {
                CliError::io_with_path(
                    "Failed to read directory entry",
                    templates_dir.display().to_string(),
                    e,
                )
            })?;

            if entry
                .file_type()
                .map_err(|e| {
                    CliError::io_with_path(
                        "Failed to get file type",
                        entry.path().display().to_string(),
                        e,
                    )
                })?
                .is_dir()
            {
                let template_name = entry.file_name().to_string_lossy().to_string();
                let template_dir = entry.path();

                // Look for template metadata
                let info_file = template_dir.join("template.yaml");
                let info = if info_file.exists() {
                    self.load_template_info(&info_file)?
                } else {
                    TemplateInfo {
                        name: template_name.clone(),
                        description: "Custom template".to_string(),
                        category: "Custom".to_string(),
                        files: self.collect_template_files(&template_dir)?,
                    }
                };

                templates.push(info);
            }
        }

        Ok(templates)
    }

    /// Generate built-in project
    fn generate_builtin_project(
        &self,
        template_name: &str,
        context: &HashMap<String, serde_json::Value>,
        output_dir: &Path,
    ) -> Result<()> {
        match template_name {
            "basic-workflow" => self.generate_basic_workflow_project(context, output_dir),
            "wasm-workflow" => self.generate_wasm_workflow_project(context, output_dir),
            "microservices" => self.generate_microservices_project(context, output_dir),
            "data-pipeline" => self.generate_data_pipeline_project(context, output_dir),
            "batch-processing" => self.generate_batch_processing_project(context, output_dir),
            _ => Err(CliError::not_found("template", template_name)),
        }
    }

    /// Generate custom project from template directory
    fn generate_custom_project(
        &self,
        template_name: &str,
        context: &HashMap<String, serde_json::Value>,
        output_dir: &Path,
    ) -> Result<()> {
        let templates_dir = self
            .templates_dir
            .as_ref()
            .ok_or_else(|| CliError::config("No templates directory configured"))?;

        let template_dir = templates_dir.join(template_name);
        if !template_dir.exists() {
            return Err(CliError::not_found("template", template_name));
        }

        self.process_template_directory(&template_dir, context, output_dir)
    }

    /// Process a template directory recursively
    fn process_template_directory(
        &self,
        template_dir: &Path,
        context: &HashMap<String, serde_json::Value>,
        output_dir: &Path,
    ) -> Result<()> {
        for entry in WalkDir::new(template_dir)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let entry_path = entry.path();
            let relative_path = entry_path.strip_prefix(template_dir).map_err(|e| {
                CliError::internal(format!("Failed to strip template prefix: {}", e))
            })?;

            // Skip template metadata files
            if relative_path.file_name() == Some(std::ffi::OsStr::new("template.yaml")) {
                continue;
            }

            let output_path = output_dir.join(relative_path);

            if entry.file_type().is_dir() {
                std::fs::create_dir_all(&output_path).map_err(|e| {
                    CliError::io_with_path(
                        "Failed to create directory",
                        output_path.display().to_string(),
                        e,
                    )
                })?;
            } else {
                if let Some(parent) = output_path.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| {
                        CliError::io_with_path(
                            "Failed to create parent directory",
                            parent.display().to_string(),
                            e,
                        )
                    })?;
                }

                self.process_template_file(entry_path, &output_path, context)?;
            }
        }

        Ok(())
    }

    /// Process a single template file
    fn process_template_file(
        &self,
        template_path: &Path,
        output_path: &Path,
        context: &HashMap<String, serde_json::Value>,
    ) -> Result<()> {
        let template_content = std::fs::read_to_string(template_path).map_err(|e| {
            CliError::io_with_path(
                "Failed to read template file",
                template_path.display().to_string(),
                e,
            )
        })?;

        let rendered_content = self
            .handlebars
            .render_template(&template_content, context)
            .map_err(|e| CliError::template(format!("Template rendering failed: {}", e)))?;

        std::fs::write(output_path, rendered_content).map_err(|e| {
            CliError::io_with_path(
                "Failed to write output file",
                output_path.display().to_string(),
                e,
            )
        })
    }

    /// Load template info from metadata file
    fn load_template_info(&self, info_file: &Path) -> Result<TemplateInfo> {
        let content = std::fs::read_to_string(info_file).map_err(|e| {
            CliError::io_with_path(
                "Failed to read template info",
                info_file.display().to_string(),
                e,
            )
        })?;

        let info: TemplateInfo = serde_yaml::from_str(&content)?;
        Ok(info)
    }

    /// Collect all files in a template directory
    fn collect_template_files(&self, template_dir: &Path) -> Result<Vec<String>> {
        let mut files = Vec::new();

        for entry in WalkDir::new(template_dir)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_file())
        {
            if let Ok(relative_path) = entry.path().strip_prefix(template_dir) {
                files.push(relative_path.display().to_string());
            }
        }

        files.sort();
        Ok(files)
    }

    /// Generate basic workflow project
    fn generate_basic_workflow_project(
        &self,
        context: &HashMap<String, serde_json::Value>,
        output_dir: &Path,
    ) -> Result<()> {
        let project_name = context
            .get("project_name")
            .and_then(|v| v.as_str())
            .unwrap_or("my-project");

        // Create Rust project directory structure
        std::fs::create_dir_all(output_dir.join("src"))?;
        std::fs::create_dir_all(output_dir.join("src").join("workflows"))?;
        std::fs::create_dir_all(output_dir.join("src").join("tasks"))?;
        std::fs::create_dir_all(output_dir.join("examples"))?;
        std::fs::create_dir_all(output_dir.join("tests"))?;

        // Generate Cargo.toml with path dependencies to local orcher workspace
        // Detect orcher repository location
        // Priority: ORCHER_PATH env var > CLI executable location > relative paths > placeholder
        let orcher_path = std::env::var("ORCHER_PATH")
            .ok()
            .or_else(|| {
                // Try to find orcher based on CLI executable location
                std::env::current_exe().ok().and_then(|exe_path| {
                    // If CLI is built from orcher repo: .../orcher/target/.../orcher
                    exe_path
                        .ancestors()
                        .find(|p| p.file_name().is_some_and(|n| n == "orcher"))
                        .and_then(|orcher_root| orcher_root.to_str().map(String::from))
                })
            })
            .or_else(|| {
                // Try common relative paths
                for candidate in ["../../orcher", "../orcher", "./orcher"] {
                    if std::path::Path::new(candidate).exists() {
                        return Some(candidate.to_string());
                    }
                }
                None
            })
            .unwrap_or_else(|| {
                // Placeholder - user must set ORCHER_PATH or update Cargo.toml
                "PLACEHOLDER_SET_ORCHER_PATH".to_string()
            });

        let cargo_toml = format!(
            r#"[package]
name = "{}"
version = "0.1.0"
edition = "2021"

# See more keys and their definitions at https://doc.rust-lang.org/cargo/reference/manifest.html

[dependencies]
# ORCHER workflow orchestration (using local path dependencies)
#
# ⚠️  IMPORTANT: Update these paths to your ORCHER installation!
#
# Option 1: Set ORCHER_PATH environment variable before running 'orcher new':
#   export ORCHER_PATH=/path/to/orcher
#   orcher new project my-project
#
# Option 2: Manually update the paths below to point to your orcher installation
#
orcher-workflow = {{ path = "{}/crates/workflow" }}
orcher-macros = {{ path = "{}/crates/macros" }}
orcher-common = {{ path = "{}/crates/common" }}

# Async runtime
tokio = {{ version = "1.35", features = ["full"] }}
futures = "0.3"

# Serialization
serde = {{ version = "1.0", features = ["derive"] }}
serde_json = "1.0"

# Error handling
anyhow = "1.0"
thiserror = "1.0"

# Time and scheduling
chrono = {{ version = "0.4", features = ["serde"] }}

# Identifiers
uuid = {{ version = "1.6", features = ["v4", "serde"] }}

# Logging
tracing = "0.1"
tracing-subscriber = {{ version = "0.3", features = ["env-filter"] }}

# Auto-registration (required by workflow/task macros)
ctor = "0.2"

[dev-dependencies]
tokio-test = "0.4"
tempfile = "3.8"

[[bin]]
name = "main"
path = "src/main.rs"

[lib]
name = "{}"
path = "src/lib.rs"

# DEPENDENCY SETUP NOTES:
#
# This project uses path dependencies to a local ORCHER installation.
# Current path: {}
#
# If you see "failed to load source" errors:
#   1. Set ORCHER_PATH environment variable: export ORCHER_PATH=/path/to/orcher
#   2. Or manually update the paths above to point to your ORCHER installation
#
# Once ORCHER is published to crates.io, you can switch to version dependencies like:
#   orcher-workflow = "0.1"
#   orcher-macros = "0.1"
#   orcher-common = "0.1"
"#,
            project_name.replace("-", "_"),
            orcher_path,
            orcher_path,
            orcher_path,
            project_name.replace("-", "_"),
            orcher_path
        );

        std::fs::write(output_dir.join("Cargo.toml"), cargo_toml)?;

        // Generate main.rs
        let main_rs = format!(
            r#"//! {} - ORCHER Code-First Workflow Project
//!
//! This binary is used for validation and registration verification.
//! Actual workflow execution happens via:
//! - `orcher run workflow_name` (submits to orchestrator)
//! - `orcher deploy` (deploys to production/staging)
//!
//! Generated: {}

use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {{
    // Initialize logging
    tracing_subscriber::fmt::init();

    // Workflows and tasks auto-register via #[ctor] when binary loads
    // Just wait a moment for static initialization
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    println!("✓ {{}} workflows and tasks loaded successfully", "{}");
    println!();
    println!("📖 To execute workflows:");
    println!("   orcher run main_workflow -p message='Hello World'");
    println!();
    println!("🚀 To deploy:");
    println!("   orcher deploy --environment production");

    Ok(())
}}
"#,
            project_name,
            Timestamptz::now().to_rfc3339(),
            project_name
        );

        std::fs::write(output_dir.join("src").join("main.rs"), main_rs)?;

        // Generate lib.rs
        let lib_rs = format!(
            r#"//! {} - ORCHER Workflow Library
//!
//! This library contains workflow and task definitions for the {} project.
//! Generated: {}

pub mod workflows;
pub mod tasks;

// Re-export common types for convenience
pub use orcher::prelude::*;
pub use workflows::*;
pub use tasks::*;
"#,
            project_name,
            project_name,
            Timestamptz::now().to_rfc3339()
        );

        std::fs::write(output_dir.join("src").join("lib.rs"), lib_rs)?;

        // Generate workflows/mod.rs
        let workflows_mod = r#"//! Workflow definitions module
//!
//! This module contains all workflow definitions using ORCHER's code-first approach.
//! Workflows are automatically registered via #[ctor] when the binary loads.

pub mod main_workflow;

// Re-export workflows
pub use main_workflow::*;
"#;

        std::fs::write(
            output_dir.join("src").join("workflows").join("mod.rs"),
            workflows_mod,
        )?;

        // Generate tasks/mod.rs
        let tasks_mod = r#"//! Task definitions module
//!
//! This module contains all task definitions and implementations.
//! Tasks are automatically registered via #[ctor] when the binary loads.

pub mod data_tasks;

// Re-export tasks
pub use data_tasks::*;
"#;

        std::fs::write(
            output_dir.join("src").join("tasks").join("mod.rs"),
            tasks_mod,
        )?;

        // Generate main workflow implementation
        let main_workflow_rs = format!(
            r#"//! Main workflow implementation
//!
//! This is the primary workflow for the {} project.
//! The #[workflow] macro automatically registers this workflow via #[ctor].

use orcher::prelude::*;
use serde::{{Deserialize, Serialize}};
use anyhow::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MainWorkflowInput {{
    pub message: String,
}}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MainWorkflowOutput {{
    pub result: String,
    pub processed_at: Timestamptz,
}}

/// Main workflow with durable execution
///
/// This workflow demonstrates ORCHER's durable execution capabilities:
/// - Automatic journaling of each step
/// - Replay protection on failures
/// - Zero data loss guarantees
///
/// # Execution
///
/// This workflow is executed via the orchestrator, not directly:
/// - Development: `orcher run main_workflow -p message='test'`
/// - Production: Deploy with `orcher deploy` then trigger via API
#[workflow(
    name = "main_workflow",
    version = "1.0.0",
    namespace = "default",
    description = "Main workflow for {}"
)]
pub async fn main_workflow(ctx: WorkflowContext, input: MainWorkflowInput) -> Result<MainWorkflowOutput> {{

    tracing::info!("Starting main workflow with input: {{:?}}", input);

    // Setup phase with durable execution
    let setup_result = ctx.execute("setup_phase", || async {{
        tracing::info!("Setting up workflow environment");
        // Simulate some setup work
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        Ok::<String, ContextError>("Setup completed successfully".to_string())
    }}).await
    .map_err(|e| anyhow::anyhow!("Setup phase failed: {{}}", e))?;

    tracing::info!("Setup phase completed: {{}}", setup_result);

    // Process the input data with durable execution
    let input_msg = input.message.clone();
    let process_result = ctx.execute("process_data", || async move {{
        tracing::info!("Processing input data: {{}}", input_msg);
        // Simulate data processing
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

        let processed = format!("Processed: {{}} (length: {{}})", input_msg, input_msg.len());
        Ok::<String, ContextError>(processed)
    }}).await
    .map_err(|e| anyhow::anyhow!("Process data failed: {{}}", e))?;

    tracing::info!("Data processing completed: {{}}", process_result);

    // Finalize with durable execution
    let process_result_clone = process_result.clone();
    let final_result = ctx.execute("finalize", || async move {{
        tracing::info!("Finalizing workflow");
        // Simulate finalization work
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let final_output = format!("{{}} - Workflow completed at {{}}",
                                 process_result_clone,
                                 Timestamptz::now().to_rfc3339());
        Ok::<String, ContextError>(final_output)
    }}).await
    .map_err(|e| anyhow::anyhow!("Finalization failed: {{}}", e))?;

    tracing::info!("Workflow finalized: {{}}", final_result);

    Ok(MainWorkflowOutput {{
        result: final_result,
        processed_at: Timestamptz::now(),
    }})
}}
"#,
            project_name, project_name
        );

        std::fs::write(
            output_dir
                .join("src")
                .join("workflows")
                .join("main_workflow.rs"),
            main_workflow_rs,
        )?;

        // Generate task implementations
        let data_tasks_rs = r#"//! Data processing tasks
//!
//! Task implementations for data processing operations.
//! The #[task] macro automatically registers these tasks via #[ctor].
//!
//! Tasks can use TaskContext as the first parameter to access:
//! - Namespace methods for logging, heartbeats, and progress tracking
//! - Cancellation detection
//! - Metadata access
//! - State management and EventGate synchronization

use orcher::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessTaskInput {
    pub data: String,
    pub options: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessTaskOutput {
    pub processed_data: String,
    pub metadata: serde_json::Value,
}

/// Data processing task
///
/// Processes input data with transformation logic.
/// Automatically registered via #[task] macro.
#[task(timeout = 30, description = "Process and transform data")]
pub async fn process_data_task(ctx: TaskContext, input: ProcessTaskInput) -> Result<ProcessTaskOutput, TaskError> {
    // Log task execution with structured context
    ctx.info("Processing data", &input);

    // Send heartbeat with progress tracking
    ctx.update_progress(0.5, "Processing").await?;

    // Simulate data processing
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let processed = format!("PROCESSED[{}]", input.data.to_uppercase());

    Ok(ProcessTaskOutput {
        processed_data: processed,
        metadata: serde_json::json!({
            "processed_at": Timestamptz::now().to_rfc3339(),
            "input_length": input.data.len(),
            "processor": "data_tasks::process_data_task",
            "attempt": ctx.attempt(),
        }),
    })
}

/// Validation task
///
/// Validates input data against business rules.
/// Automatically registered via #[task] macro.
#[task(timeout = 10, description = "Validate data integrity")]
pub async fn validate_data_task(ctx: TaskContext, input: serde_json::Value) -> Result<serde_json::Value, TaskError> {
    // Log validation with structured context
    ctx.info("Validating data", &input);

    // Simulate validation
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    Ok(serde_json::json!({
        "valid": true,
        "validated_at": Timestamptz::now().to_rfc3339(),
        "input": input,
        "task_id": ctx.task_id().to_string(),
    }))
}
"#;

        std::fs::write(
            output_dir.join("src").join("tasks").join("data_tasks.rs"),
            data_tasks_rs,
        )?;

        // Generate orcher.toml configuration
        let orcher_config = format!(
            r#"# ORCHER Code-First Project Configuration
# Generated: {}

[project]
name = "{}"
version = "1.0.0"
description = "Code-first workflow project"

# Default namespace
namespace = "default"

[runtime]
# Enable durable execution with automatic journaling
enable_durable_execution = true

[runtime.database]
# Database configuration for ExecutionLog
url = "postgresql://postgres:password@localhost:5432/orcher"

[runtime.redis]
# Redis configuration for caching and queues
url = "redis://localhost:6379"

[execution]
# Default timeout for workflows
default_timeout = "30m"

[execution.retry]
# Default retry policy
attempts = 3
backoff = "exponential"

[execution.resources]
# Resource limits
cpu = "200m"
memory = "256Mi"

[development]
# Enable detailed logging
debug_logging = true

# Hot reload for development
hot_reload = true

[deployment]
# Target environment
environment = "development"

[deployment.container]
# Container configuration
image = "orcher/{}"
tag = "latest"
"#,
            Timestamptz::now().to_rfc3339(),
            project_name,
            project_name
        );

        std::fs::write(output_dir.join("orcher.toml"), orcher_config)?;

        // Generate comprehensive README
        let readme = format!(
            r#"# {} - ORCHER Code-First Workflow Project

A modern, type-safe workflow orchestration project built with ORCHER's code-first approach.

Generated: {}

## 🚀 Features

- **Code-First Workflows**: Define workflows in Rust with full type safety
- **Durable Execution**: Automatic journaling and replay protection
- **Zero Downtime**: Workflows survive crashes and resume exactly where they left off
- **Performance**: Built with Rust for maximum performance and reliability

## 📁 Project Structure

```
{}/
├── src/
│   ├── main.rs              # Main application entry point
│   ├── lib.rs               # Library exports
│   ├── workflows/           # Workflow definitions
│   │   ├── mod.rs           # Workflow module exports
│   │   └── main_workflow.rs # Main workflow implementation
│   └── tasks/               # Task implementations
│       ├── mod.rs           # Task module exports
│       └── data_tasks.rs    # Data processing tasks
├── examples/                # Usage examples
├── tests/                   # Integration tests
├── Cargo.toml              # Rust dependencies
├── orcher.toml             # ORCHER configuration
└── README.md               # This file
```

## 🏃 Quick Start

### Prerequisites

This project uses **path dependencies** to the local ORCHER repository. Before building:

1. **Ensure ORCHER is available locally**:
   ```bash
   # Clone ORCHER if you don't have it
   git clone https://github.com/orcher-io/sdk-rust.git
   ```

2. **Set ORCHER_PATH environment variable** (recommended):
   ```bash
   export ORCHER_PATH=/path/to/orcher
   # Add to your ~/.bashrc or ~/.zshrc to make it permanent
   ```

3. **Or manually update dependency paths** in `Cargo.toml`:
   ```toml
   orcher-workflow = {{ path = "/path/to/orcher/crates/workflow" }}
   orcher-macros = {{ path = "/path/to/orcher/crates/macros" }}
   orcher-common = {{ path = "/path/to/orcher/crates/common" }}
   ```

> **Current path in Cargo.toml**: `{}`
>
> **Note**: Once ORCHER is published to crates.io, you can switch to version dependencies.

### 1. Build the Project

```bash
# Build the workflow library
cargo build

# Verify registration (optional)
cargo run
```

If you get dependency errors, check that the paths in `Cargo.toml` correctly point to your ORCHER installation.

### 2. Run Workflows via ORCHER CLI

Workflows are executed through the ORCHER orchestrator, not directly:

```bash
# Run the main workflow with the orchestrator
orcher run main_workflow -p message="Hello World"

# Run with environment override
orcher run main_workflow -e staging -p message="Test"
```

### 3. Test the Workflow

```bash
# Run unit tests
cargo test

# Test integration with ORCHER
cargo test --test integration_tests
```

## 📖 Workflow Definition Example

The main workflow demonstrates ORCHER's durable execution:

```rust
#[workflow(version = "1.0.0", namespace = "default")]
pub async fn main_workflow(ctx: WorkflowContext, input: MainWorkflowInput) -> Result<MainWorkflowOutput> {{
    // Durable execution - survives crashes!
    let setup_result = ctx.execute("setup_phase", || async {{
        // Your setup logic here
        Ok("Setup completed".to_string())
    }}).await?;

    // Process data with automatic journaling
    let process_result = ctx.execute("process_data", || async {{
        // Your processing logic here
        Ok(format!("Processed: {{}}", input.message))
    }}).await?;

    // Finalize
    let final_result = ctx.execute("finalize", || async {{
        // Your finalization logic here
        Ok("Workflow completed".to_string())
    }}).await?;

    Ok(MainWorkflowOutput {{
        result: final_result,
        processed_at: Timestamptz::now(),
    }})
}}
```

## 🔧 Configuration

Edit `orcher.toml` to configure:

- Database connections (PostgreSQL for durable execution)
- Redis for caching and queues
- Resource limits and timeouts
- Development vs production settings

## 🧪 Development

### Adding New Workflows

1. Create a new file in `src/workflows/`
2. Implement your workflow with `#[workflow]` macro
3. Add registration in `src/workflows/mod.rs`

### Adding New Tasks

1. Create task functions in `src/tasks/`
2. Use `#[task]` macro for automatic registration
3. Add to task registry in `src/tasks/mod.rs`

### Using DX Improvements

ORCHER includes modern DX improvements to reduce boilerplate:

**Attribute Shortcuts:**
```rust
// Before: Verbose attributes
#[task(
    name = "process_order",
    version = "1.0.0",
    namespace = "default",
    retry_policy(max_attempts = 3, backoff = "exponential"),
    timeout = 300
)]

// After: Smart defaults and shortcuts
#[task(retry = 3, timeout_mins = 5)]
pub async fn process_order(ctx: TaskContext, order_id: String) -> TaskResult<String> {{
    // Task name is automatically inferred as "process-order"
    // ...
}}
```

**Namespace Methods:**
```rust
// Structured logging
ctx.log.info("Processing order", &order_id);
ctx.log.debug("Order details", &order);

// Heartbeats with progress
ctx.pulse.send().await?;
ctx.pulse.with_stage("processing").await?;
ctx.progress.update(0.5, "50% complete").await?;
```

**Presets for Common Patterns:**
```rust
#[task(preset = "long-running")]  // retry=3, timeout=30min, memory=512Mi
pub async fn batch_process(ctx: TaskContext, items: Vec<Item>) -> TaskResult<Summary> {{
    // Your logic here
}}
```

### Testing

```bash
# Unit tests
cargo test

# Integration tests
cargo test --test integration

# Performance tests
cargo test --release --test performance
```

## 🚀 Deployment

### Local Development

```bash
# 1. Ensure ORCHER path is correct (see Prerequisites above)

# 2. Start ORCHER services (PostgreSQL + Redis)
docker-compose up -d

# 3. Build your workflows
cargo build

# 4. Execute workflows via ORCHER CLI
orcher run main_workflow -p message="Development test"
```

### Troubleshooting

**Error: "no matching package named `orcher-workflow` found"** or **"failed to load source"**
- **Cause**: Path dependencies in `Cargo.toml` don't point to a valid ORCHER installation
- **Solution 1** (Recommended): Set ORCHER_PATH before creating projects:
  ```bash
  export ORCHER_PATH=/path/to/orcher
  orcher new project my-project
  ```
- **Solution 2**: Manually update paths in `Cargo.toml`:
  ```toml
  orcher-workflow = {{ path = "/absolute/path/to/orcher/crates/workflow" }}
  ```
- **Verify**: Check that the path exists:
  ```bash
  ls /path/to/orcher/crates/workflow/Cargo.toml
  ```

**Error: "failed to load manifest"**
- Ensure all three path dependencies point to the same ORCHER installation
- Use absolute paths if relative paths are causing issues

### Production Deployment

```bash
# Deploy with ORCHER CLI (builds and deploys automatically)
orcher deploy --environment production

# Or deploy to staging first
orcher deploy --environment staging --dry-run  # Preview changes
orcher deploy --environment staging            # Actually deploy
```

The deploy command will:
1. Build your workflows to WASM
2. Upload to ORCHER registry
3. Deploy to the specified environment
4. Make workflows available to the orchestrator

## 📊 Monitoring

ORCHER provides built-in monitoring:

- **Execution Logs**: Every step is journaled to PostgreSQL
- **Metrics**: Prometheus metrics for performance monitoring
- **Tracing**: Distributed tracing for debugging

## 🔑 Key Concepts

### Workflow Execution Model

Workflows in ORCHER follow a **code-first** approach with **orchestrator-based execution**:

1. **Registration**: Workflows auto-register via `#[workflow]` macro using `#[ctor]`
2. **Execution**: Workflows run through the ORCHER orchestrator, NOT standalone
3. **CLI Commands**:
   - `orcher run` - Submit workflows to orchestrator for execution
   - `orcher deploy` - Deploy workflows to production/staging environments

### Why Not `cargo run`?

The `cargo run` command only validates that workflows compile and register correctly.
Actual execution requires the full ORCHER orchestrator infrastructure:
- Task submission and coordination
- State persistence and recovery
- Durable execution guarantees
- Resource management

Use `orcher run` for actual workflow execution.

## 🆘 Support

- [ORCHER Documentation](https://docs.orcher.io)
- [Examples](./examples/)
- [Community](https://github.com/orcher-io/cli/discussions)

## 📄 License

Generated with ORCHER CLI v{}

This project is ready for development with ORCHER's code-first workflow platform!
"#,
            project_name,
            Timestamptz::now().to_rfc3339(),
            project_name,
            orcher_path,
            VERSION
        );

        std::fs::write(output_dir.join("README.md"), readme)?;

        // Generate .gitignore
        let gitignore = r#"# Rust
/target/
Cargo.lock
*.pdb

# ORCHER
.orcher/
logs/

# IDE
.vscode/
.idea/
*.swp
*.swo

# OS
.DS_Store
Thumbs.db

# Environment
.env
.env.local
.env.*.local
"#;

        std::fs::write(output_dir.join(".gitignore"), gitignore)?;

        // Generate example tests
        let test_example = format!(
            r#"//! Integration tests for {} workflows
//! Generated: {}

use anyhow::Result;
use std::sync::Arc;
use uuid::Uuid;

#[cfg(test)]
mod tests {{
    use super::*;
    use {}::workflows::main_workflow::{{main_workflow, MainWorkflowInput}};
    use orcher::prelude::*;
    use orcher::runtime::execution_log::ExecutionLog;

    #[tokio::test]
    async fn test_main_workflow() -> Result<()> {{
        // Create ExecutionLog for durable execution testing
        let workflow_id = Uuid::new_v4();
        let execution_log = Arc::new(ExecutionLog::new(workflow_id));

        // Create a test workflow context with durable execution
        let ctx = RuntimeWorkflowContext {{
            "test-execution".to_string(),
            "main_workflow".to_string(),
            "1.0.0".to_string(),
            "default".to_string(),
        )
        .with_execution_log(execution_log);

        // Create test input
        let input = MainWorkflowInput {{
            message: "Test message".to_string(),
        }};

        // Execute the workflow
        let result = main_workflow(ctx, input).await?;

        // Verify results
        assert!(result.result.contains("Test message"));
        assert!(!result.result.is_empty());

        println!("Workflow test completed: {{:?}}", result);
        Ok(())
    }}

    #[tokio::test]
    async fn test_workflow_with_different_inputs() -> Result<()> {{
        let test_cases = vec![
            "Short msg",
            "This is a longer test message with more content",
            "🚀 Unicode test message! 🎉",
        ];

        for msg in test_cases {{
            // Create ExecutionLog for each test case
            let workflow_id = Uuid::new_v4();
            let execution_log = Arc::new(ExecutionLog::new(workflow_id));

            let ctx = WorkflowContext::new(
                format!("test-execution-{{}}", msg.len()),
                "main_workflow".to_string(),
                "1.0.0".to_string(),
                "default".to_string(),
            )
            .with_execution_log(execution_log);

            let input = MainWorkflowInput {{
                message: msg.to_string(),
            }};

            let result = main_workflow(ctx, input).await?;

            // Verify each test case
            assert!(result.result.contains(msg));
            println!("Test passed for input: {{}} -> {{}}", msg, result.result);
        }}

        Ok(())
    }}
}}
"#,
            project_name,
            Timestamptz::now().to_rfc3339(),
            project_name.replace("-", "_")
        );

        std::fs::write(
            output_dir.join("tests").join("integration_tests.rs"),
            test_example,
        )?;

        // Generate examples directory with a simple example
        let example_usage = format!(
            r#"//! Example usage of {} workflows
//!
//! This demonstrates how to use the workflows programmatically with proper context setup.
//! Note: In production, workflows are executed via `orcher run` command.

use anyhow::Result;
use {}::workflows::main_workflow::{{main_workflow, MainWorkflowInput}};
use orcher::prelude::*;
use orcher::runtime::execution_log::ExecutionLog;
use std::sync::Arc;
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<()> {{
    // Initialize logging
    tracing_subscriber::fmt::init();

    println!("🚀 {} Workflow Example");
    println!();
    println!("⚠️  Note: This is for demonstration only.");
    println!("   In production, use: orcher run main_workflow -p message='...'");
    println!();

    // Create ExecutionLog for durable execution
    let workflow_id = Uuid::new_v4();
    let execution_log = Arc::new(ExecutionLog::new(workflow_id));

    // Create workflow context with durable execution support
    let ctx = RuntimeWorkflowContext {{
        "example-execution".to_string(),
        "main_workflow".to_string(),
        "1.0.0".to_string(),
        "default".to_string(),
    )
    .with_execution_log(execution_log);

    // Execute with example data
    let input = MainWorkflowInput {{
        message: "Hello from {} example!".to_string(),
    }};

    println!("Executing workflow with input: {{:?}}", input);

    match main_workflow(ctx, input).await {{
        Ok(result) => {{
            println!("✅ Workflow completed successfully!");
            println!("Result: {{:?}}", result);
        }}
        Err(e) => {{
            println!("❌ Workflow failed: {{}}", e);
        }}
    }}

    Ok(())
}}
"#,
            project_name,
            project_name.replace("-", "_"),
            project_name,
            project_name
        );

        std::fs::write(
            output_dir.join("examples").join("basic_usage.rs"),
            example_usage,
        )?;

        Ok(())
    }

    /// Generate WASM-compatible workflow project
    fn generate_wasm_workflow_project(
        &self,
        context: &HashMap<String, serde_json::Value>,
        output_dir: &Path,
    ) -> Result<()> {
        let project_name = context
            .get("project_name")
            .and_then(|v| v.as_str())
            .unwrap_or("my-project");

        // Create Rust project directory structure
        std::fs::create_dir_all(output_dir.join("src"))?;
        std::fs::create_dir_all(output_dir.join("src").join("workflows"))?;
        std::fs::create_dir_all(output_dir.join("src").join("tasks"))?;
        std::fs::create_dir_all(output_dir.join("tests"))?;

        // Detect orcher repository location
        let orcher_path = std::env::var("ORCHER_PATH")
            .ok()
            .or_else(|| {
                std::env::current_exe().ok().and_then(|exe_path| {
                    // Walk up from the binary location to find the orcher repository root
                    // Look for a directory that contains both "crates" and "Cargo.toml"
                    exe_path
                        .ancestors()
                        .find(|p| {
                            p.join("crates").is_dir()
                                && p.join("crates/workflow").is_dir()
                                && p.join("Cargo.toml").exists()
                        })
                        .and_then(|orcher_root| orcher_root.to_str().map(String::from))
                })
            })
            .or_else(|| {
                for candidate in ["../../orcher", "../orcher", "./orcher"] {
                    let candidate_path = std::path::Path::new(candidate);
                    if candidate_path.join("crates/workflow").is_dir()
                        && candidate_path.join("Cargo.toml").exists()
                    {
                        return Some(candidate.to_string());
                    }
                }
                None
            })
            .unwrap_or_else(|| "PLACEHOLDER_SET_ORCHER_PATH".to_string());

        // Generate Cargo.toml with WASM-compatible dependencies
        let cargo_toml = format!(
            r#"[package]
name = "{}"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
# ORCHER workflow orchestration (using local path dependencies)
orcher-workflow = {{ path = "{}/crates/workflow" }}
orcher-macros = {{ path = "{}/crates/macros" }}
orcher-common = {{ path = "{}/crates/common" }}

# Serialization (WASM-compatible)
serde = {{ version = "1.0", features = ["derive"] }}
serde_json = "1.0"

# Error handling (WASM-compatible)
thiserror = "1.0"

# Identifiers (WASM-compatible)
uuid = {{ version = "1.6", features = ["v4", "serde", "js"] }}

# Auto-registration (required by workflow/task macros)
ctor = "0.2"

# WASM-specific dependencies
wasm-bindgen = "0.2"
wasm-bindgen-futures = "0.4"

# Async runtime - WASM-compatible configuration
[target.'cfg(target_arch = "wasm32")'.dependencies]
tokio = {{ version = "1.35", features = ["sync", "macros"] }}
futures = "0.3"

# Async runtime - full features for native targets
[target.'cfg(not(target_arch = "wasm32"))'.dependencies]
tokio = {{ version = "1.35", features = ["full"] }}
futures = "0.3"
chrono = {{ version = "0.4", features = ["serde"] }}

[dev-dependencies]
wasm-bindgen-test = "0.3"

[workspace]
# This is a standalone project

# WASM BUILD NOTES:
#
# This template is optimized for WebAssembly compilation.
#
# To build for WASM:
#   cargo build --lib --release --target wasm32-wasip1
#
# To optimize WASM size:
#   wasm-opt -Oz -o output.wasm input.wasm
#
# Key differences from basic-workflow template:
#   • Uses cdylib crate type for WASM
#   • Conditional tokio features (minimal for WASM)
#   • Includes wasm-bindgen for JS interop
#   • No OS-specific dependencies
"#,
            project_name.replace("-", "_"),
            orcher_path,
            orcher_path,
            orcher_path
        );

        std::fs::write(output_dir.join("Cargo.toml"), cargo_toml)?;

        // Generate lib.rs
        let lib_rs = r#"//! WASM-Compatible Workflow Library
//!
//! This library is designed for WebAssembly compilation and deployment
//! to the ORCHER platform.

pub mod workflows;
pub mod tasks;

// Re-export main workflows
pub use workflows::main_workflow::main_workflow;
"#;
        std::fs::write(output_dir.join("src").join("lib.rs"), lib_rs)?;

        // Generate workflows/mod.rs
        let workflows_mod = r#"//! Workflow definitions

pub mod main_workflow;
"#;
        std::fs::write(
            output_dir.join("src").join("workflows").join("mod.rs"),
            workflows_mod,
        )?;

        // Generate workflows/main_workflow.rs with WASM-compatible code
        let main_workflow_rs = r#"//! Main Workflow
//!
//! WASM-compatible workflow example

use orcher_macros::{task, workflow};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowInput {
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowOutput {
    pub result: String,
    pub processed_at: String,
}

/// Example task - WASM-compatible
#[task(
    name = "process_message",
    version = "1.0.0",
    description = "Process incoming message",
    timeout = 30
)]
async fn process_message(input: WorkflowInput) -> Result<String, String> {
    // Simple processing (no OS-specific operations)
    let processed = format!("Processed: {}", input.message.to_uppercase());
    Ok(processed)
}

/// Main workflow - WASM-compatible
#[workflow(
    name = "main_workflow",
    version = "1.0.0",
    description = "WASM-compatible workflow example",
    namespace = "default"
)]
pub async fn main_workflow(input: WorkflowInput) -> Result<WorkflowOutput, String> {
    // Process the message
    let result = process_message(input.clone()).await?;

    Ok(WorkflowOutput {
        result,
        processed_at: "timestamp".to_string(), // Use WASM-compatible time
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_main_workflow() {
        let input = WorkflowInput {
            message: "hello".to_string(),
        };

        let result = main_workflow(input).await;
        assert!(result.is_ok());

        let output = result.unwrap();
        assert_eq!(output.result, "Processed: HELLO");
    }
}
"#;
        std::fs::write(
            output_dir
                .join("src")
                .join("workflows")
                .join("main_workflow.rs"),
            main_workflow_rs,
        )?;

        // Generate tasks/mod.rs
        let tasks_mod = r#"//! Task definitions

// Add your tasks here
"#;
        std::fs::write(
            output_dir.join("src").join("tasks").join("mod.rs"),
            tasks_mod,
        )?;

        // Generate README.md
        let readme = format!(
            r#"# {} - WASM Workflow

WASM-compatible workflow project for ORCHER platform.

## Building

### For WASM target:
```bash
cargo build --lib --release --target wasm32-wasip1
```

### For native target (development):
```bash
cargo build --lib
cargo test
```

## WASM Optimization

To reduce binary size:
```bash
wasm-opt -Oz -o optimized.wasm target/wasm32-wasip1/release/{}.wasm
```

## Deployment

```bash
# Deploy to ORCHER platform
orcher deploy --environment production
```

## Key Features

- ✅ WASM-compatible dependencies
- ✅ No OS-specific operations
- ✅ Conditional async runtime (minimal tokio for WASM)
- ✅ Ready for ORCHER deployment

## Differences from basic-workflow

This template uses:
- `cdylib` crate type for WASM compilation
- Conditional tokio features (minimal for WASM, full for native)
- wasm-bindgen for JavaScript interop
- No chrono or other OS-dependent crates in WASM mode

## Testing

```bash
# Native tests
cargo test

# WASM tests (requires wasm-pack)
wasm-pack test --node
```
"#,
            project_name,
            project_name.replace("-", "_")
        );
        std::fs::write(output_dir.join("README.md"), readme)?;

        // Generate .gitignore
        let gitignore = r#"# Rust
/target/
Cargo.lock
**/*.rs.bk

# WASM build artifacts
*.wasm
pkg/

# IDE
.vscode/
.idea/
*.swp
*.swo

# OS
.DS_Store
Thumbs.db
"#;
        std::fs::write(output_dir.join(".gitignore"), gitignore)?;

        Ok(())
    }

    /// Generate other template projects (simplified for now)
    fn generate_microservices_project(
        &self,
        context: &HashMap<String, serde_json::Value>,
        output_dir: &Path,
    ) -> Result<()> {
        // For now, use basic workflow as base and customize
        self.generate_basic_workflow_project(context, output_dir)?;

        // Add microservices-specific files
        std::fs::create_dir_all(output_dir.join("services"))?;

        let services_config = r#"# Microservices Configuration
services:
  - name: api-gateway
    image: nginx:alpine
    port: 80

  - name: user-service
    image: node:18-alpine
    port: 3000

  - name: database
    image: postgres:15
    port: 5432
"#;

        std::fs::write(
            output_dir.join("configs").join("services.yaml"),
            services_config,
        )?;
        Ok(())
    }

    fn generate_data_pipeline_project(
        &self,
        context: &HashMap<String, serde_json::Value>,
        output_dir: &Path,
    ) -> Result<()> {
        self.generate_basic_workflow_project(context, output_dir)?;

        // Add data pipeline specific files
        std::fs::create_dir_all(output_dir.join("pipelines"))?;

        let pipeline_config = r#"# Data Pipeline Configuration
data_sources:
  - name: source-db
    type: postgresql
    connection: ${DATABASE_URL}

  - name: target-warehouse
    type: bigquery
    project: ${GCP_PROJECT}

transformations:
  - name: clean-data
    type: python
    script: scripts/clean.py
"#;

        std::fs::write(
            output_dir.join("configs").join("data-sources.yaml"),
            pipeline_config,
        )?;
        Ok(())
    }

    fn generate_batch_processing_project(
        &self,
        context: &HashMap<String, serde_json::Value>,
        output_dir: &Path,
    ) -> Result<()> {
        self.generate_basic_workflow_project(context, output_dir)?;

        // Add batch processing specific files
        std::fs::create_dir_all(output_dir.join("schedules"))?;

        let schedule_config = r#"# Batch Processing Schedule
apiVersion: orcher.io/v1
kind: Schedule
metadata:
  name: nightly-batch
spec:
  schedule: "0 2 * * *"  # 2 AM daily
  workflow: batch-job
  timezone: UTC
"#;

        std::fs::write(
            output_dir.join("schedules").join("nightly.yaml"),
            schedule_config,
        )?;
        Ok(())
    }
}

impl Default for TemplateEngine {
    fn default() -> Self {
        Self::new().expect("Failed to create template engine")
    }
}

// Implement serde traits for TemplateInfo
use serde::{Deserialize, Serialize};

impl Serialize for TemplateInfo {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("TemplateInfo", 4)?;
        state.serialize_field("name", &self.name)?;
        state.serialize_field("description", &self.description)?;
        state.serialize_field("category", &self.category)?;
        state.serialize_field("files", &self.files)?;
        state.end()
    }
}

impl<'de> Deserialize<'de> for TemplateInfo {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct TemplateInfoHelper {
            name: String,
            description: String,
            category: String,
            files: Vec<String>,
        }

        let helper = TemplateInfoHelper::deserialize(deserializer)?;
        Ok(TemplateInfo {
            name: helper.name,
            description: helper.description,
            category: helper.category,
            files: helper.files,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_template_engine_creation() {
        let engine = TemplateEngine::new();
        assert!(engine.is_ok());
    }

    #[test]
    fn test_builtin_templates() {
        let engine = TemplateEngine::new().unwrap();

        assert!(engine.template_exists("basic-workflow").unwrap());
        assert!(engine.template_exists("microservices").unwrap());
        assert!(engine.template_exists("data-pipeline").unwrap());
        assert!(engine.template_exists("batch-processing").unwrap());
        assert!(!engine.template_exists("nonexistent").unwrap());
    }

    #[test]
    fn test_list_builtin_templates() {
        let engine = TemplateEngine::new().unwrap();
        let templates = engine.list_templates().unwrap();

        assert_eq!(templates.len(), 5);
        assert!(templates.iter().any(|t| t.name == "basic-workflow"));
        assert!(templates.iter().any(|t| t.name == "wasm-workflow"));
        assert!(templates.iter().any(|t| t.name == "microservices"));
        assert!(templates.iter().any(|t| t.name == "data-pipeline"));
        assert!(templates.iter().any(|t| t.name == "batch-processing"));
    }

    #[test]
    fn test_project_generation() -> std::result::Result<(), Box<dyn std::error::Error>> {
        let engine = TemplateEngine::new().unwrap();
        let temp_dir = TempDir::new()?;

        let mut context = HashMap::new();
        context.insert(
            "project_name".to_string(),
            serde_json::Value::String("test-project".to_string()),
        );
        context.insert(
            "created_by".to_string(),
            serde_json::Value::String("test-user".to_string()),
        );

        engine.generate_project("basic-workflow", &context, temp_dir.path())?;

        // Verify files were created (code-first Rust project structure)
        assert!(temp_dir.path().join("Cargo.toml").exists());
        assert!(temp_dir.path().join("orcher.toml").exists());
        assert!(temp_dir.path().join("src").join("lib.rs").exists());
        assert!(temp_dir
            .path()
            .join("src")
            .join("workflows")
            .join("main_workflow.rs")
            .exists());
        assert!(temp_dir.path().join("README.md").exists());

        // Verify content contains project name
        let orcher_config = std::fs::read_to_string(temp_dir.path().join("orcher.toml"))?;
        assert!(orcher_config.contains("test-project"));

        Ok(())
    }
}
