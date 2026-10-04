//! `orcher new` — scaffold projects, workflows, and tasks from templates.

use crate::error::{CliError, Result};
use crate::templates::TemplateEngine;
use crate::time::Timestamptz;
use crate::utils::{validation::validate_resource_name, GlobalConfig};
use crate::{commands::common, constants::VERSION, types::NewCommands};
use convert_case::{Case, Casing};
use std::path::PathBuf;
use tracing::{info, warn};

pub async fn execute(resource: NewCommands, global_config: &GlobalConfig) -> Result<()> {
    match resource {
        NewCommands::Project {
            name,
            template,
            directory,
        } => create_project(name, template, directory, global_config).await,
        NewCommands::Workflow {
            name,
            workflow_type,
            output,
        } => create_workflow(name, workflow_type, output, global_config).await,
    }
}

/// Create a new project from template
async fn create_project(
    name: String,
    template: String,
    directory: Option<String>,
    global_config: &GlobalConfig,
) -> Result<()> {
    info!(
        "Creating new project '{}' from template '{}'",
        name, template
    );

    // Validate project name
    validate_resource_name(&name)?;

    // Determine output directory
    let output_dir = if let Some(dir) = directory {
        PathBuf::from(dir).join(&name)
    } else {
        PathBuf::from(&name)
    };

    // Check if directory already exists
    if output_dir.exists() {
        return Err(CliError::already_exists("project", &name));
    }

    // Initialize template engine
    let template_engine = TemplateEngine::new()?;

    // Check if template exists
    if !template_engine.template_exists(&template)? {
        return Err(CliError::not_found("template", template));
    }

    // Create project directory
    std::fs::create_dir_all(&output_dir).map_err(|e| {
        CliError::io_with_path(
            "Failed to create project directory",
            output_dir.display().to_string(),
            e,
        )
    })?;

    // Generate project from template
    let context = create_project_context(&name, &template)?;
    template_engine.generate_project(&template, &context, &output_dir)?;

    // Create initial git repository if git is available
    if let Err(e) = initialize_git_repo(&output_dir) {
        warn!("Failed to initialize git repository: {}", e);
    }

    common::print_success(
        global_config,
        &format!(
            "✓ Project '{}' created successfully in {}",
            name,
            output_dir.display()
        ),
    );

    // Print next steps
    if !global_config.quiet {
        println!();
        println!("⚠️  IMPORTANT: Path Dependencies");
        println!("   This project uses path dependencies to the local ORCHER repository.");
        println!("   Before building, ensure ORCHER is available at: ../orcher/");
        println!("   Or set ORCHER_PATH environment variable to your ORCHER location.");
        println!();
        println!("Next steps:");
        println!("  cd {}", name);
        println!();
        println!("1. Verify ORCHER path dependencies:");
        println!("   ls ../orcher/crates/workflow");
        println!();
        println!("2. Build and verify registration:");
        println!("   cargo build                          # Build the workflow library");
        println!("   cargo run                            # Verify registration");
        println!("   cargo test                           # Run tests");
        println!();
        println!("3. Execute workflows via ORCHER orchestrator:");
        println!("   orcher run main_workflow -p message='Hello World'");
        println!();
        println!("4. Deploy to production:");
        println!("   orcher deploy --environment production");
        println!();
        println!("If you get 'no matching package' errors:");
        println!("  • Check Cargo.toml path dependencies point to your ORCHER installation");
        println!("  • Update paths if ORCHER is in a different location");
        println!();
        println!("⚠️  Important: Workflows execute via the orchestrator (orcher run),");
        println!("   not directly with cargo run. cargo run only verifies registration.");
    }

    Ok(())
}

/// Create a new workflow definition
async fn create_workflow(
    name: String,
    workflow_type: String,
    output: Option<String>,
    global_config: &GlobalConfig,
) -> Result<()> {
    info!(
        "Creating new workflow '{}' of type '{}'",
        name, workflow_type
    );

    // Validate workflow name
    validate_resource_name(&name)?;

    // Validate workflow type
    let valid_types = ["sequential", "parallel", "dag", "conditional"];
    if !valid_types.contains(&workflow_type.as_str()) {
        return Err(CliError::invalid_input_with_details(
            format!("Invalid workflow type: {}", workflow_type),
            "workflow_type",
            "sequential, parallel, dag, conditional",
        ));
    }

    // Determine output file (Rust source file)
    let safe_name = name.replace("-", "_");
    let output_path = if let Some(output_file) = output {
        PathBuf::from(output_file)
    } else {
        PathBuf::from(format!("{}.rs", safe_name))
    };

    // Check if file already exists
    if output_path.exists() {
        return Err(CliError::already_exists("workflow", &name));
    }

    // Generate workflow definition
    let workflow_def = generate_workflow_definition(&name, &workflow_type)?;

    // Write workflow to file
    std::fs::write(&output_path, workflow_def).map_err(|e| {
        CliError::io_with_path(
            "Failed to write workflow file",
            output_path.display().to_string(),
            e,
        )
    })?;

    common::print_success(
        global_config,
        &format!(
            "✓ Code-first workflow '{}' created successfully: {}",
            name,
            output_path.display()
        ),
    );

    // Print next steps
    if !global_config.quiet {
        println!();
        println!("Next steps:");
        println!("  # Add to your Cargo.toml dependencies:");
        println!("  # orcher-workflow = \"0.1\"");
        println!("  # orcher-macros = \"0.1\"");
        println!();
        println!("  cargo build                           # Build the workflow");
        println!("  cargo test                            # Test the workflow");
        println!();
        println!("To execute:");
        println!("  orcher run {} -p key=value", name);
    }

    Ok(())
}

/// Create a new task definition
#[allow(dead_code)]
async fn create_task(
    name: String,
    runtime: String,
    output: Option<String>,
    global_config: &GlobalConfig,
) -> Result<()> {
    info!("Creating new task '{}' with runtime '{}'", name, runtime);

    // Validate task name
    validate_resource_name(&name)?;

    // Validate runtime
    let valid_runtimes = ["docker", "shell", "http", "python", "nodejs"];
    if !valid_runtimes.contains(&runtime.as_str()) {
        return Err(CliError::invalid_input_with_details(
            format!("Invalid task runtime: {}", runtime),
            "runtime",
            "docker, shell, http, python, nodejs",
        ));
    }

    // Determine output file
    let output_path = if let Some(output_file) = output {
        PathBuf::from(output_file)
    } else {
        PathBuf::from(format!("{}-task.yaml", name))
    };

    // Check if file already exists
    if output_path.exists() {
        return Err(CliError::already_exists("task", &name));
    }

    // Generate task definition
    let task_def = generate_task_definition(&name, &runtime)?;

    // Write task to file
    std::fs::write(&output_path, task_def).map_err(|e| {
        CliError::io_with_path(
            "Failed to write task file",
            output_path.display().to_string(),
            e,
        )
    })?;

    common::print_success(
        global_config,
        &format!(
            "✓ Task '{}' created successfully: {}",
            name,
            output_path.display()
        ),
    );

    // Print next steps
    if !global_config.quiet {
        println!();
        println!("Next steps:");
        println!("  orcher validate {}", output_path.display());
        println!("  # Add this task to a workflow");
    }

    Ok(())
}

/// Create project context for template rendering
fn create_project_context(
    name: &str,
    template: &str,
) -> Result<std::collections::HashMap<String, serde_json::Value>> {
    let mut context = std::collections::HashMap::new();

    context.insert(
        "project_name".to_string(),
        serde_json::Value::String(name.to_string()),
    );
    context.insert(
        "template".to_string(),
        serde_json::Value::String(template.to_string()),
    );
    context.insert(
        "created_at".to_string(),
        serde_json::Value::String(Timestamptz::now().to_rfc3339()),
    );
    context.insert(
        "created_by".to_string(),
        serde_json::Value::String(std::env::var("USER").unwrap_or_else(|_| "unknown".to_string())),
    );
    context.insert(
        "orcher_version".to_string(),
        serde_json::Value::String(VERSION.to_string()),
    );

    Ok(context)
}

/// Generate workflow definition YAML
fn generate_workflow_definition(name: &str, workflow_type: &str) -> Result<String> {
    let workflow_def = match workflow_type {
        "sequential" => generate_sequential_workflow(name),
        "parallel" => generate_parallel_workflow(name),
        "dag" => generate_dag_workflow(name),
        "conditional" => generate_conditional_workflow(name),
        _ => {
            return Err(CliError::invalid_input(format!(
                "Unknown workflow type: {}",
                workflow_type
            )))
        }
    };

    Ok(workflow_def)
}

/// Generate sequential workflow template
fn generate_sequential_workflow(name: &str) -> String {
    let safe_name = name.replace("-", "_");
    format!(
        r#"//! {} - Sequential Workflow
//! Generated: {}

use orcher::prelude::*;
use serde::{{Deserialize, Serialize}};
use anyhow::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct {}Input {{
    pub data: String,
    pub options: Option<serde_json::Value>,
}}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct {}Output {{
    pub result: String,
    pub steps_completed: Vec<String>,
    pub processed_at: Timestamptz,
}}

// Task input/output types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetupTaskInput {{
    pub workflow_data: String,
}}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessTaskInput {{
    pub data: String,
}}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessTaskOutput {{
    pub processed: String,
}}

// Registered tasks
#[task(timeout = 30, description = "Setup phase")]
pub async fn setup_phase_task(_ctx: TaskContext, _input: SetupTaskInput) -> Result<String> {{
    tracing::info!("Setting up workflow");
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    Ok("Setup completed".to_string())
}}

#[task(timeout = 60, description = "Main processing phase")]
pub async fn main_process_task(_ctx: TaskContext, input: ProcessTaskInput) -> Result<ProcessTaskOutput> {{
    tracing::info!("Processing data: {{}}", input.data);
    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

    let processed = format!("PROCESSED[{{}}]", input.data.to_uppercase());
    Ok(ProcessTaskOutput {{ processed }})
}}

#[task(timeout = 30, description = "Cleanup phase")]
pub async fn cleanup_task(_ctx: TaskContext, _input: serde_json::Value) -> Result<String> {{
    tracing::info!("Cleaning up resources");
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    Ok("Cleanup completed".to_string())
}}

/// Sequential workflow with registered tasks
#[workflow(version = "1.0.0", namespace = "default")]
pub async fn {}(ctx: WorkflowContext, input: {}Input) -> Result<{}Output> {{
    tracing::info!("Starting sequential workflow with input: {{:?}}", input);

    let mut completed_steps = Vec::new();

    // Setup phase - execute registered task
    let setup_result: String = ctx.execute_task(
        "setup_phase_task",
        SetupTaskInput {{
            workflow_data: input.data.clone(),
        }}
    ).await?;

    completed_steps.push("setup".to_string());
    tracing::info!("Setup completed: {{}}", setup_result);

    // Main processing phase - execute registered task
    let process_result: ProcessTaskOutput = ctx.execute_task(
        "main_process_task",
        ProcessTaskInput {{
            data: input.data.clone(),
        }}
    ).await?;

    completed_steps.push("main_process".to_string());
    tracing::info!("Processing completed: {{}}", process_result.processed);

    // Cleanup phase - execute registered task
    let cleanup_result: String = ctx.execute_task(
        "cleanup_task",
        serde_json::json!({{}})
    ).await?;

    completed_steps.push("cleanup".to_string());
    tracing::info!("Cleanup completed: {{}}", cleanup_result);

    let final_result = format!("Sequential workflow completed: {{}} -> {{}}",
                              setup_result, process_result.processed);

    Ok({}Output {{
        result: final_result,
        steps_completed: completed_steps,
        processed_at: Timestamptz::now(),
    }})
}}
"#,
        name,
        Timestamptz::now().to_rfc3339(),
        safe_name.to_case(Case::Pascal),
        safe_name.to_case(Case::Pascal),
        safe_name,
        safe_name.to_case(Case::Pascal),
        safe_name.to_case(Case::Pascal),
        safe_name.to_case(Case::Pascal),
    )
}

/// Generate parallel workflow template
fn generate_parallel_workflow(name: &str) -> String {
    let safe_name = name.replace("-", "_");
    format!(
        r#"//! {} - Parallel Workflow
//! Generated: {}

use orcher::prelude::*;
use serde::{{Deserialize, Serialize}};
use anyhow::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct {}Input {{
    pub items: Vec<String>,
    pub max_concurrency: Option<usize>,
}}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct {}Output {{
    pub results: Vec<String>,
    pub total_processed: usize,
    pub completed_at: Timestamptz,
}}

// Task input/output types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchTaskInput {{
    pub batch_items: Vec<String>,
    pub batch_id: String,
}}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregateTaskInput {{
    pub all_results: Vec<String>,
}}

// Registered tasks for parallel processing
#[task(timeout = 60, description = "Process batch A")]
pub async fn process_task_a(_ctx: TaskContext, input: BatchTaskInput) -> Result<Vec<String>> {{
    tracing::info!("Processing task A");
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    let processed = input.batch_items.iter()
        .map(|item| format!("A[{{}}]", item))
        .collect();
    Ok(processed)
}}

#[task(timeout = 60, description = "Process batch B")]
pub async fn process_task_b(_ctx: TaskContext, input: BatchTaskInput) -> Result<Vec<String>> {{
    tracing::info!("Processing task B");
    tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;

    let processed = input.batch_items.iter()
        .map(|item| format!("B[{{}}]", item))
        .collect();
    Ok(processed)
}}

#[task(timeout = 60, description = "Process batch C")]
pub async fn process_task_c(_ctx: TaskContext, input: BatchTaskInput) -> Result<Vec<String>> {{
    tracing::info!("Processing task C");
    tokio::time::sleep(tokio::time::Duration::from_millis(180)).await;

    let processed = input.batch_items.iter()
        .map(|item| format!("C[{{}}]", item))
        .collect();
    Ok(processed)
}}

#[task(timeout = 60, description = "Aggregate results from parallel tasks")]
pub async fn aggregate_results_task(_ctx: TaskContext, input: AggregateTaskInput) -> Result<Vec<String>> {{
    tracing::info!("Aggregating {{}} results", input.all_results.len());
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let final_results = input.all_results.iter()
        .map(|r| format!("AGGREGATED[{{}}]", r))
        .collect();
    Ok(final_results)
}}

/// Parallel workflow with concurrent registered task execution
#[workflow(version = "1.0.0", namespace = "default")]
pub async fn {}(ctx: WorkflowContext, input: {}Input) -> Result<{}Output> {{
    tracing::info!("Starting parallel workflow with {{}} items", input.items.len());

    let _max_concurrency = input.max_concurrency.unwrap_or(5);

    // Split items into batches
    let batch_size = (input.items.len() + 2) / 3;
    let batch_a: Vec<String> = input.items.iter().take(batch_size).cloned().collect();
    let batch_b: Vec<String> = input.items.iter().skip(batch_size).take(batch_size).cloned().collect();
    let batch_c: Vec<String> = input.items.iter().skip(batch_size * 2).cloned().collect();

    // Execute tasks in parallel using tokio::join!
    let (task_a_result, task_b_result, task_c_result) = tokio::join!(
        ctx.execute_task::<_, Vec<String>>("process_task_a", BatchTaskInput {{
            batch_items: batch_a,
            batch_id: "A".to_string(),
        }}),
        ctx.execute_task::<_, Vec<String>>("process_task_b", BatchTaskInput {{
            batch_items: batch_b,
            batch_id: "B".to_string(),
        }}),
        ctx.execute_task::<_, Vec<String>>("process_task_c", BatchTaskInput {{
            batch_items: batch_c,
            batch_id: "C".to_string(),
        }})
    );

    // Collect all results
    let mut all_results = Vec::new();
    all_results.extend(task_a_result?);
    all_results.extend(task_b_result?);
    all_results.extend(task_c_result?);

    // Aggregate results using registered task
    let aggregated: Vec<String> = ctx.execute_task(
        "aggregate_results_task",
        AggregateTaskInput {{ all_results }}
    ).await?;

    tracing::info!("Parallel workflow completed with {{}} results", aggregated.len());

    Ok({}Output {{
        results: aggregated,
        total_processed: input.items.len(),
        completed_at: Timestamptz::now(),
    }})
}}
"#,
        name,
        Timestamptz::now().to_rfc3339(),
        safe_name.to_case(Case::Pascal),
        safe_name.to_case(Case::Pascal),
        safe_name,
        safe_name.to_case(Case::Pascal),
        safe_name.to_case(Case::Pascal),
        safe_name.to_case(Case::Pascal),
    )
}

/// Generate DAG workflow template
fn generate_dag_workflow(name: &str) -> String {
    let safe_name = name.replace("-", "_");
    format!(
        r#"//! {} - DAG Workflow
//! Generated: {}

use orcher::prelude::*;
use serde::{{Deserialize, Serialize}};
use anyhow::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct {}Input {{
    pub source_url: String,
    pub validation_rules: Option<serde_json::Value>,
}}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct {}Output {{
    pub final_result: String,
    pub processing_chain: Vec<String>,
    pub completed_at: Timestamptz,
}}

// Task input/output types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FetchDataInput {{
    pub source_url: String,
}}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidateInput {{
    pub data: String,
}}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeInput {{
    pub result_a: String,
    pub result_b: String,
}}

// Registered tasks for DAG workflow
#[task(timeout = 30, description = "Initialize DAG workflow")]
pub async fn initialize_task(_ctx: TaskContext, _input: serde_json::Value) -> Result<String> {{
    tracing::info!("Initializing DAG workflow");
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    Ok("Initialization completed".to_string())
}}

#[task(timeout = 60, description = "Fetch data from source")]
pub async fn fetch_data_task(_ctx: TaskContext, input: FetchDataInput) -> Result<String> {{
    tracing::info!("Fetching data from: {{}}", input.source_url);
    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
    Ok(format!("Data fetched from {{}}", input.source_url))
}}

#[task(timeout = 60, description = "Process branch A")]
pub async fn process_branch_a_task(_ctx: TaskContext, _input: serde_json::Value) -> Result<String> {{
    tracing::info!("Processing branch A");
    tokio::time::sleep(tokio::time::Duration::from_millis(250)).await;
    Ok("Branch A processed".to_string())
}}

#[task(timeout = 60, description = "Process branch B")]
pub async fn process_branch_b_task(_ctx: TaskContext, _input: serde_json::Value) -> Result<String> {{
    tracing::info!("Processing branch B");
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
    Ok("Branch B processed".to_string())
}}

#[task(timeout = 30, description = "Validate branch A")]
pub async fn validate_a_task(_ctx: TaskContext, input: ValidateInput) -> Result<String> {{
    tracing::info!("Validating branch A result: {{}}", input.data);
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    Ok("Branch A validated".to_string())
}}

#[task(timeout = 30, description = "Validate branch B")]
pub async fn validate_b_task(_ctx: TaskContext, input: ValidateInput) -> Result<String> {{
    tracing::info!("Validating branch B result: {{}}", input.data);
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    Ok("Branch B validated".to_string())
}}

#[task(timeout = 30, description = "Merge validated results")]
pub async fn merge_results_task(_ctx: TaskContext, input: MergeInput) -> Result<String> {{
    tracing::info!("Merging validated results");
    tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;

    let merged = format!("MERGED[{{}} + {{}}]", input.result_a, input.result_b);
    Ok(merged)
}}

#[task(timeout = 30, description = "Finalize DAG workflow")]
pub async fn finalize_task(_ctx: TaskContext, input: String) -> Result<String> {{
    tracing::info!("Finalizing DAG workflow");
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let final_output = format!("FINAL[{{}}] - DAG completed successfully", input);
    Ok(final_output)
}}

/// DAG workflow with complex dependencies using registered tasks
#[workflow(version = "1.0.0", namespace = "default")]
pub async fn {}(ctx: WorkflowContext, input: {}Input) -> Result<{}Output> {{
    tracing::info!("Starting DAG workflow with source: {{}}", input.source_url);

    let mut processing_chain = Vec::new();

    // Initialize
    let _init_result: String = ctx.execute_task("initialize_task", serde_json::json!({{}})).await?;
    processing_chain.push("initialize".to_string());

    // Fetch data (depends on init)
    let _fetch_result: String = ctx.execute_task(
        "fetch_data_task",
        FetchDataInput {{
            source_url: input.source_url.clone(),
        }}
    ).await?;
    processing_chain.push("fetch_data".to_string());

    // Parallel processing branches (both depend on fetch)
    let (process_a_result, process_b_result) = tokio::join!(
        ctx.execute_task::<_, String>("process_branch_a_task", serde_json::json!({{}})),
        ctx.execute_task::<_, String>("process_branch_b_task", serde_json::json!({{}}))
    );

    let process_a_result = process_a_result?;
    let process_b_result = process_b_result?;
    processing_chain.push("process_branch_a".to_string());
    processing_chain.push("process_branch_b".to_string());

    // Parallel validation (depends on respective processing)
    let (validate_a_result, validate_b_result) = tokio::join!(
        ctx.execute_task::<_, String>("validate_a_task", ValidateInput {{
            data: process_a_result.clone(),
        }}),
        ctx.execute_task::<_, String>("validate_b_task", ValidateInput {{
            data: process_b_result.clone(),
        }})
    );

    let validate_a_result = validate_a_result?;
    let validate_b_result = validate_b_result?;
    processing_chain.push("validate_a".to_string());
    processing_chain.push("validate_b".to_string());

    // Merge results (depends on both validations)
    let merge_result: String = ctx.execute_task(
        "merge_results_task",
        MergeInput {{
            result_a: validate_a_result,
            result_b: validate_b_result,
        }}
    ).await?;
    processing_chain.push("merge_results".to_string());

    // Finalize (depends on merge)
    let final_result: String = ctx.execute_task("finalize_task", merge_result).await?;
    processing_chain.push("finalize".to_string());

    tracing::info!("DAG workflow completed with {{}} steps", processing_chain.len());

    Ok({}Output {{
        final_result,
        processing_chain,
        completed_at: Timestamptz::now(),
    }})
}}
"#,
        name,
        Timestamptz::now().to_rfc3339(),
        safe_name.to_case(Case::Pascal),
        safe_name.to_case(Case::Pascal),
        safe_name,
        safe_name.to_case(Case::Pascal),
        safe_name.to_case(Case::Pascal),
        safe_name.to_case(Case::Pascal),
    )
}

/// Generate conditional workflow template
fn generate_conditional_workflow(name: &str) -> String {
    let safe_name = name.replace("-", "_");
    format!(
        r#"//! {} - Conditional Workflow
//! Generated: {}

use orcher::prelude::*;
use serde::{{Deserialize, Serialize}};
use anyhow::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct {}Input {{
    pub environment: String,
    pub threshold: Option<i32>,
}}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct {}Output {{
    pub execution_path: String,
    pub results: serde_json::Value,
    pub completed_at: Timestamptz,
}}

// Task input/output types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnvCheckInput {{
    pub environment: String,
}}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThresholdInput {{
    pub threshold: i32,
}}

// Registered tasks for conditional workflow
#[task(timeout = 30, description = "Check environment type")]
pub async fn check_environment_task(_ctx: TaskContext, input: EnvCheckInput) -> Result<bool> {{
    tracing::info!("Checking environment: {{}}", input.environment);
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let is_production = input.environment.to_lowercase() == "production";
    Ok(is_production)
}}

#[task(timeout = 60, description = "Execute production flow")]
pub async fn production_flow_task(_ctx: TaskContext, _input: serde_json::Value) -> Result<(String, serde_json::Value)> {{
    tracing::info!("Executing production workflow");
    tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;

    let result = serde_json::json!({{
        "deployment": "production",
        "security_checks": true,
        "backup_created": true,
        "monitoring_enabled": true
    }});

    Ok(("production".to_string(), result))
}}

#[task(timeout = 60, description = "Execute development flow")]
pub async fn development_flow_task(_ctx: TaskContext, _input: serde_json::Value) -> Result<(String, serde_json::Value)> {{
    tracing::info!("Executing development workflow");
    tokio::time::sleep(tokio::time::Duration::from_millis(150)).await;

    let result = serde_json::json!({{
        "deployment": "development",
        "debug_enabled": true,
        "hot_reload": true,
        "test_data_seeded": true
    }});

    Ok(("development".to_string(), result))
}}

#[task(timeout = 30, description = "Process with threshold")]
pub async fn threshold_processing_task(_ctx: TaskContext, input: ThresholdInput) -> Result<serde_json::Value> {{
    tracing::info!("Processing with threshold: {{}}", input.threshold);
    tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;

    let meets_threshold = input.threshold > 50;
    let result = if meets_threshold {{
        serde_json::json!({{
            "threshold_met": true,
            "high_priority": true,
            "additional_resources": true
        }})
    }} else {{
        serde_json::json!({{
            "threshold_met": false,
            "standard_priority": true,
            "basic_resources": true
        }})
    }};

    Ok(result)
}}

#[task(timeout = 30, description = "Common cleanup")]
pub async fn common_cleanup_task(_ctx: TaskContext, _input: serde_json::Value) -> Result<String> {{
    tracing::info!("Performing common cleanup");
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    Ok("Cleanup completed".to_string())
}}

/// Conditional workflow with dynamic execution paths using registered tasks
#[workflow(version = "1.0.0", namespace = "default")]
pub async fn {}(ctx: WorkflowContext, input: {}Input) -> Result<{}Output> {{
    tracing::info!("Starting conditional workflow for environment: {{}}", input.environment);

    // Environment check
    let env_check: bool = ctx.execute_task(
        "check_environment_task",
        EnvCheckInput {{
            environment: input.environment.clone(),
        }}
    ).await?;

    tracing::info!("Environment check completed: production = {{}}", env_check);

    // Conditional execution based on environment
    let execution_result: (String, serde_json::Value) = if env_check {{
        ctx.execute_task("production_flow_task", serde_json::json!({{}})).await?
    }} else {{
        ctx.execute_task("development_flow_task", serde_json::json!({{}})).await?
    }};

    let (execution_path, path_results) = execution_result;

    // Optional threshold-based processing
    let threshold_result = if let Some(threshold) = input.threshold {{
        ctx.execute_task::<_, serde_json::Value>(
            "threshold_processing_task",
            ThresholdInput {{ threshold }}
        ).await?
    }} else {{
        serde_json::json!({{"threshold_processing": "skipped"}})
    }};

    // Common cleanup regardless of path
    let cleanup_result: String = ctx.execute_task(
        "common_cleanup_task",
        serde_json::json!({{}})
    ).await?;

    tracing::info!("Conditional workflow completed via {{}} path", execution_path);

    // Combine all results
    let final_results = serde_json::json!({{
        "path_results": path_results,
        "threshold_results": threshold_result,
        "cleanup": cleanup_result
    }});

    Ok({}Output {{
        execution_path,
        results: final_results,
        completed_at: Timestamptz::now(),
    }})
}}
"#,
        name,
        Timestamptz::now().to_rfc3339(),
        safe_name.to_case(Case::Pascal),
        safe_name.to_case(Case::Pascal),
        safe_name,
        safe_name.to_case(Case::Pascal),
        safe_name.to_case(Case::Pascal),
        safe_name.to_case(Case::Pascal),
    )
}

/// Generate task definition YAML
#[allow(dead_code)]
fn generate_task_definition(name: &str, runtime: &str) -> Result<String> {
    let task_def = match runtime {
        "docker" => generate_docker_task(name),
        "shell" => generate_shell_task(name),
        "http" => generate_http_task(name),
        "python" => generate_python_task(name),
        "nodejs" => generate_nodejs_task(name),
        _ => {
            return Err(CliError::invalid_input(format!(
                "Unknown runtime: {}",
                runtime
            )))
        }
    };

    Ok(task_def)
}

/// Generate Docker task template
#[allow(dead_code)]
fn generate_docker_task(name: &str) -> String {
    format!(
        r#"# ORCHER Task Definition
# Generated: {}
# Runtime: Docker

apiVersion: orcher.io/v1
kind: Task
metadata:
  name: {}
  labels:
    runtime: docker
spec:
  # Docker container configuration
  container:
    image: alpine:latest
    command: ["echo"]
    args: ["Hello from {}"]

    # Environment variables
    env:
      - name: TASK_NAME
        value: {}
      - name: LOG_LEVEL
        value: info

    # Working directory
    workingDir: /app

    # Resource limits
    resources:
      requests:
        cpu: 100m
        memory: 128Mi
      limits:
        cpu: 500m
        memory: 512Mi

  # Input parameters
  inputs:
    parameters:
      - name: message
        description: Message to display
        default: "Hello World"
        type: string

    # File inputs (if needed)
    # artifacts:
    #   - name: input-data
    #     path: /app/data
    #     optional: true

  # Output artifacts
  outputs:
    # parameters:
    #   - name: result
    #     valueFrom:
    #       path: /app/output/result.txt

    artifacts:
      - name: logs
        path: /app/logs/
        optional: true

  # Retry configuration
  retry:
    limit: 3
    backoffStrategy: exponential

  # Timeout
  timeout: 10m
"#,
        Timestamptz::now().to_rfc3339(),
        name,
        name,
        name,
    )
}

/// Generate Shell task template
#[allow(dead_code)]
fn generate_shell_task(name: &str) -> String {
    format!(
        r#"# ORCHER Task Definition
# Generated: {}
# Runtime: Shell

apiVersion: orcher.io/v1
kind: Task
metadata:
  name: {}
  labels:
    runtime: shell
spec:
  # Shell script configuration
  script:
    interpreter: /bin/bash
    source: |
      #!/bin/bash
      set -euo pipefail

      echo "Starting task: {}"
      echo "Timestamp: $(date)"

      # Task logic here
      echo "Processing data..."
      sleep 2

      echo "Task completed successfully"
      exit 0

  # Environment variables
  env:
    - name: TASK_NAME
      value: {}
    - name: SHELL
      value: /bin/bash

  # Working directory
  workingDir: /tmp/orcher-tasks

  # Resource limits
  resources:
    requests:
      cpu: 50m
      memory: 64Mi
    limits:
      cpu: 200m
      memory: 256Mi

  # Input parameters
  inputs:
    parameters:
      - name: input_file
        description: Input file to process
        type: string
        default: "/dev/null"

  # Timeout
  timeout: 5m
"#,
        Timestamptz::now().to_rfc3339(),
        name,
        name,
        name,
    )
}

/// Generate HTTP task template
#[allow(dead_code)]
fn generate_http_task(name: &str) -> String {
    format!(
        r#"# ORCHER Task Definition
# Generated: {}
# Runtime: HTTP

apiVersion: orcher.io/v1
kind: Task
metadata:
  name: {}
  labels:
    runtime: http
spec:
  # HTTP request configuration
  http:
    url: "https://api.example.com/endpoint"
    method: GET

    # Headers
    headers:
      Content-Type: application/json
      User-Agent: "orcher-cli/{}"

    # Query parameters
    # params:
    #   key1: value1
    #   key2: value2

    # Request body (for POST/PUT)
    # body: |
    #   {{
    #     "task_name": "{}",
    #     "timestamp": "${{{{ now() }}}}"
    #   }}

    # Timeout for the request
    timeout: 30s

    # Retry configuration
    retries: 3
    retry_delay: 1s

  # Input parameters
  inputs:
    parameters:
      - name: api_endpoint
        description: API endpoint URL
        type: string
        default: "https://api.example.com/endpoint"

      - name: api_key
        description: API key for authentication
        type: string
        secret: true

  # Output processing
  outputs:
    parameters:
      - name: response_status
        valueFrom:
          http_response: status_code

      - name: response_body
        valueFrom:
          http_response: body

  # Success criteria
  successCondition: "outputs.parameters.response_status == '200'"
"#,
        Timestamptz::now().to_rfc3339(),
        name,
        VERSION,
        name,
    )
}

/// Generate Python task template
#[allow(dead_code)]
fn generate_python_task(name: &str) -> String {
    format!(
        r#"# ORCHER Task Definition
# Generated: {}
# Runtime: Python

apiVersion: orcher.io/v1
kind: Task
metadata:
  name: {}
  labels:
    runtime: python
spec:
  # Python script configuration
  python:
    version: "3.9"

    # Python script
    source: |
      #!/usr/bin/env python3
      import os
      import sys
      import json
      from datetime import datetime

      def main():
          print(f"Starting Python task: {}")
          print(f"Timestamp: {{datetime.now().isoformat()}}")

          # Get task parameters
          task_name = os.environ.get('TASK_NAME', 'unknown')

          # Task logic here
          print("Processing data with Python...")

          # Example: process some data
          result = {{
              'task_name': task_name,
              'status': 'completed',
              'timestamp': datetime.now().isoformat()
          }}

          # Output results
          print(f"Results: {{json.dumps(result, indent=2)}}")
          print("Python task completed successfully")

          return 0

      if __name__ == '__main__':
          sys.exit(main())

    # Python dependencies
    requirements: |
      requests>=2.25.0
      pyyaml>=6.0

    # Working directory
    workingDir: /app

  # Environment variables
  env:
    - name: TASK_NAME
      value: {}
    - name: PYTHONPATH
      value: /app

  # Resource limits
  resources:
    requests:
      cpu: 100m
      memory: 256Mi
    limits:
      cpu: 1000m
      memory: 1Gi

  # Input parameters
  inputs:
    parameters:
      - name: data_file
        description: Data file to process
        type: string
        default: "data.json"

  # Timeout
  timeout: 15m
"#,
        Timestamptz::now().to_rfc3339(),
        name,
        name,
        name,
    )
}

/// Generate Node.js task template
#[allow(dead_code)]
fn generate_nodejs_task(name: &str) -> String {
    format!(
        r#"# ORCHER Task Definition
# Generated: {}
# Runtime: Node.js

apiVersion: orcher.io/v1
kind: Task
metadata:
  name: {}
  labels:
    runtime: nodejs
spec:
  # Node.js script configuration
  nodejs:
    version: "18"

    # JavaScript source
    source: |
      const fs = require('fs');
      const path = require('path');

      async function main() {{
          console.log('Starting Node.js task: {}');
          console.log('Timestamp:', new Date().toISOString());

          // Get task parameters
          const taskName = process.env.TASK_NAME || 'unknown';

          // Task logic here
          console.log('Processing data with Node.js...');

          // Example: process some data
          const result = {{
              taskName,
              status: 'completed',
              timestamp: new Date().toISOString(),
              nodeVersion: process.version
          }};

          // Output results
          console.log('Results:', JSON.stringify(result, null, 2));
          console.log('Node.js task completed successfully');

          return 0;
      }}

      main()
          .then(code => process.exit(code))
          .catch(error => {{
              console.error('Task failed:', error);
              process.exit(1);
          }});

    # Package.json dependencies
    package_json: |
      {{
        "name": "{}",
        "version": "1.0.0",
        "description": "ORCHER task",
        "dependencies": {{
          "axios": "^1.0.0",
          "lodash": "^4.17.21"
        }}
      }}

    # Working directory
    workingDir: /app

  # Environment variables
  env:
    - name: TASK_NAME
      value: {}
    - name: NODE_ENV
      value: production

  # Resource limits
  resources:
    requests:
      cpu: 100m
      memory: 256Mi
    limits:
      cpu: 1000m
      memory: 1Gi

  # Input parameters
  inputs:
    parameters:
      - name: config_file
        description: Configuration file
        type: string
        default: "config.json"

  # Timeout
  timeout: 15m
"#,
        Timestamptz::now().to_rfc3339(),
        name,
        name,
        name,
        name,
    )
}

/// Initialize git repository in the project directory
fn initialize_git_repo(project_dir: &PathBuf) -> Result<()> {
    // Check if git is available
    let output = std::process::Command::new("git").arg("--version").output();

    if output.is_err() {
        return Err(CliError::command_failed(
            "git",
            None,
            Some("Git not found".to_string()),
        ));
    }

    // Initialize git repository
    let output = std::process::Command::new("git")
        .arg("init")
        .current_dir(project_dir)
        .output()
        .map_err(|e| CliError::internal(format!("Failed to execute git init: {}", e)))?;

    if !output.status.success() {
        return Err(CliError::command_failed(
            "git init",
            output.status.code(),
            Some(String::from_utf8_lossy(&output.stderr).to_string()),
        ));
    }

    // Create .gitignore file
    let gitignore_content = r#"# ORCHER CLI generated .gitignore

# OS generated files
.DS_Store
.DS_Store?
._*
.Spotlight-V100
.Trashes
ehthumbs.db
Thumbs.db

# IDE files
.vscode/
.idea/
*.swp
*.swo
*~

# Logs
*.log
logs/

# Temporary files
*.tmp
*.temp
.cache/

# Environment files
.env
.env.local
.env.*.local

# Build artifacts
dist/
build/
target/

# Dependencies
node_modules/
__pycache__/
*.pyc
.pytest_cache/
"#;

    let gitignore_path = project_dir.join(".gitignore");
    std::fs::write(&gitignore_path, gitignore_content).map_err(|e| {
        CliError::io_with_path(
            "Failed to create .gitignore",
            gitignore_path.display().to_string(),
            e,
        )
    })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_validate_workflow_type() {
        assert!(generate_workflow_definition("test", "sequential").is_ok());
        assert!(generate_workflow_definition("test", "parallel").is_ok());
        assert!(generate_workflow_definition("test", "dag").is_ok());
        assert!(generate_workflow_definition("test", "conditional").is_ok());
        assert!(generate_workflow_definition("test", "invalid").is_err());
    }

    #[test]
    fn test_validate_task_runtime() {
        assert!(generate_task_definition("test", "docker").is_ok());
        assert!(generate_task_definition("test", "shell").is_ok());
        assert!(generate_task_definition("test", "http").is_ok());
        assert!(generate_task_definition("test", "python").is_ok());
        assert!(generate_task_definition("test", "nodejs").is_ok());
        assert!(generate_task_definition("test", "invalid").is_err());
    }

    #[test]
    fn test_create_project_context() {
        let context = create_project_context("my-project", "basic-workflow").unwrap();

        assert_eq!(context.get("project_name").unwrap(), "my-project");
        assert_eq!(context.get("template").unwrap(), "basic-workflow");
        assert!(context.contains_key("created_at"));
        assert!(context.contains_key("created_by"));
        assert!(context.contains_key("orcher_version"));
    }

    #[tokio::test]
    async fn test_workflow_generation_formats() {
        let temp_dir = TempDir::new().unwrap();
        let global_config = GlobalConfig::default();

        // Test workflow creation - now generates Rust code (code-first approach)
        let result = create_workflow(
            "test-workflow".to_string(),
            "sequential".to_string(),
            Some(
                temp_dir
                    .path()
                    .join("test_workflow.rs")
                    .to_string_lossy()
                    .to_string(),
            ),
            &global_config,
        )
        .await;

        assert!(result.is_ok());

        // Check file was created
        let workflow_file = temp_dir.path().join("test_workflow.rs");
        assert!(workflow_file.exists());

        // Check file contains expected Rust code-first content
        let content = std::fs::read_to_string(&workflow_file).unwrap();
        assert!(content.contains("use orcher::prelude::*;"));
        assert!(content.contains("#[workflow("));
        assert!(content.contains("test_workflow"));
        assert!(content.contains("Sequential Workflow"));
    }

    #[tokio::test]
    async fn test_task_generation_formats() {
        let temp_dir = TempDir::new().unwrap();
        let global_config = GlobalConfig::default();

        // Test task creation
        let result = create_task(
            "test-task".to_string(),
            "docker".to_string(),
            Some(
                temp_dir
                    .path()
                    .join("task.yaml")
                    .to_string_lossy()
                    .to_string(),
            ),
            &global_config,
        )
        .await;

        assert!(result.is_ok());

        // Check file was created
        let task_file = temp_dir.path().join("task.yaml");
        assert!(task_file.exists());

        // Check file contains expected content
        let content = std::fs::read_to_string(&task_file).unwrap();
        assert!(content.contains("apiVersion: orcher.io/v1"));
        assert!(content.contains("kind: Task"));
        assert!(content.contains("name: test-task"));
        assert!(content.contains("runtime: docker"));
    }
}
