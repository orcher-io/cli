//! `orcher new` — scaffold a project, or add a workflow to one.
//!
//! Projects come from the templates in `crate::templates`: the same small
//! project in Python, TypeScript or Rust, built on the published SDK for that
//! language. `new workflow` adds a workflow with one task to the project in
//! the current directory, in its language.

use crate::error::{CliError, Result};
use crate::templates::TemplateEngine;
use crate::time::Timestamptz;
use crate::utils::{validation::validate_resource_name, GlobalConfig};
use crate::{commands::common, constants::VERSION, types::NewCommands};
use convert_case::{Case, Casing};
use std::path::{Path, PathBuf};
use tracing::{info, warn};

/// The languages a project can be in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Python,
    TypeScript,
    Rust,
}

impl Language {
    /// The language of the project in `dir`, from the files that define it.
    pub fn detect(dir: &Path) -> Option<Self> {
        if dir.join("Cargo.toml").is_file() {
            Some(Self::Rust)
        } else if dir.join("package.json").is_file() {
            Some(Self::TypeScript)
        } else if ["requirements.txt", "pyproject.toml", "setup.py"]
            .iter()
            .any(|f| dir.join(f).is_file())
        {
            Some(Self::Python)
        } else {
            None
        }
    }

    pub fn parse(name: &str) -> Result<Self> {
        match TemplateEngine::default()
            .resolve_template_alias(name)
            .as_str()
        {
            "python" => Ok(Self::Python),
            "typescript" => Ok(Self::TypeScript),
            "rust" => Ok(Self::Rust),
            _ => Err(CliError::invalid_input_with_details(
                format!("Unknown language: {}", name),
                "language",
                "python, typescript, rust",
            )),
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Python => "python",
            Self::TypeScript => "typescript",
            Self::Rust => "rust",
        }
    }
}

pub async fn execute(resource: NewCommands, global_config: &GlobalConfig) -> Result<()> {
    match resource {
        NewCommands::Project {
            name,
            template,
            directory,
        } => create_project(name, template, directory, global_config).await,
        NewCommands::Python { name, directory } => {
            create_project(name, "python".to_string(), directory, global_config).await
        }
        NewCommands::Typescript { name, directory } => {
            create_project(name, "typescript".to_string(), directory, global_config).await
        }
        NewCommands::Rust { name, directory } => {
            create_project(name, "rust".to_string(), directory, global_config).await
        }
        NewCommands::Workflow {
            name,
            workflow_type,
            file,
            language,
        } => create_workflow(name, workflow_type, file, language, global_config).await,
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

    validate_project_name(&name)?;

    let output_dir = match directory {
        Some(dir) => PathBuf::from(dir).join(&name),
        None => PathBuf::from(&name),
    };
    if output_dir.exists() {
        return Err(CliError::already_exists("project", &name));
    }

    let template_engine = TemplateEngine::new()?;
    if !template_engine.template_exists(&template)? {
        return Err(CliError::invalid_input_with_details(
            format!("Unknown template: {}", template),
            "template",
            "python, typescript, rust",
        ));
    }
    let language = Language::parse(&template)?;

    std::fs::create_dir_all(&output_dir).map_err(|e| {
        CliError::io_with_path(
            "Failed to create project directory",
            output_dir.display().to_string(),
            e,
        )
    })?;
    let context = create_project_context(&name, language.name())?;
    template_engine.generate_project(language.name(), &context, &output_dir)?;

    if let Err(e) = initialize_git_repo(&output_dir) {
        warn!("Failed to initialize git repository: {}", e);
    }

    common::print_success(
        global_config,
        &format!(
            "✓ {} project '{}' created in {}",
            language.name(),
            name,
            output_dir.display()
        ),
    );

    if !global_config.quiet {
        println!();
        println!("Next steps:");
        println!("  cd {}", output_dir.display());
        match language {
            Language::Python => {
                println!("  python3 -m venv .venv && .venv/bin/pip install -r requirements.txt");
                println!("  orcher dev start              # a local engine, if you have none");
                println!("  orcher test                   # run the hello workflow end to end");
                println!("  .venv/bin/python worker.py    # run the worker");
            }
            Language::TypeScript => {
                println!("  npm install");
                println!("  orcher dev start              # a local engine, if you have none");
                println!("  orcher test                   # run the hello workflow end to end");
                println!("  npm run worker                # run the worker");
            }
            Language::Rust => {
                println!("  orcher dev start              # a local engine, if you have none");
                println!("  orcher test                   # run the hello workflow end to end");
                println!("  cargo run -- worker           # run the worker");
            }
        }
        println!();
        println!("Then start workflows from anywhere:");
        println!(
            "  orcher workflow start hello --task-queue {} --input '\"Ada\"' --wait",
            name
        );
    }

    Ok(())
}

/// A project name must be a valid package name in every language: lowercase
/// letters, digits and hyphens, starting with a letter.
fn validate_project_name(name: &str) -> Result<()> {
    validate_resource_name(name)?;
    if !name.chars().next().is_some_and(|c| c.is_ascii_lowercase()) {
        return Err(CliError::invalid_input(
            "Project name must start with a lowercase letter",
        ));
    }
    Ok(())
}

/// Add a workflow, with one task, to the project in the current directory
async fn create_workflow(
    name: String,
    workflow_type: String,
    output: Option<String>,
    language: Option<String>,
    global_config: &GlobalConfig,
) -> Result<()> {
    info!(
        "Creating new workflow '{}' of type '{}'",
        name, workflow_type
    );

    validate_resource_name(&name)?;
    if workflow_type != "sequential" {
        return Err(CliError::invalid_input_with_details(
            format!(
                "Only sequential workflows are generated, not '{}'. Start from one and run \
                 tasks concurrently with your SDK.",
                workflow_type
            ),
            "workflow_type",
            "sequential",
        ));
    }

    let language = match language {
        Some(l) => Language::parse(&l)?,
        None => Language::detect(Path::new(".")).ok_or_else(|| {
            CliError::invalid_input(
                "No project here to add a workflow to. Run this in a project made with \
                 `orcher new`, or pass --language python|typescript|rust.",
            )
        })?,
    };

    let (default_path, source, register) = workflow_source(&name, language);
    let output_path = output.map(PathBuf::from).unwrap_or(default_path);
    if output_path.exists() {
        return Err(CliError::already_exists("workflow", &name));
    }
    if let Some(parent) = output_path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| {
            CliError::io_with_path(
                "Failed to create directory",
                parent.display().to_string(),
                e,
            )
        })?;
    }
    std::fs::write(&output_path, source).map_err(|e| {
        CliError::io_with_path(
            "Failed to write workflow file",
            output_path.display().to_string(),
            e,
        )
    })?;

    common::print_success(
        global_config,
        &format!("✓ Workflow '{}' created: {}", name, output_path.display()),
    );
    if !global_config.quiet {
        println!();
        println!("Register it with the worker: {}", register);
        println!("Then start it:");
        println!(
            "  orcher workflow start {} --task-queue <your queue> --input '\"something\"' --wait",
            name
        );
    }
    Ok(())
}

/// The file for a new workflow in `language`: where it goes, what it says,
/// and how the worker picks it up.
fn workflow_source(name: &str, language: Language) -> (PathBuf, String, String) {
    let snake = name.to_case(Case::Snake);
    let camel = name.to_case(Case::Camel);
    let pascal = name.to_case(Case::Pascal);
    match language {
        Language::Python => (
            PathBuf::from(format!("{snake}.py")),
            format!(
                r#""""The {name} workflow and its task."""

from orcher import TaskContext, WorkflowContext, task, workflow


@task(name="{snake}_step")
async def {snake}_step(ctx: TaskContext, value: str) -> str:
    # Side effects go here: call an API or write to a database.
    return f"{name} handled {{value}}"


@workflow(name="{name}")
async def {snake}(ctx: WorkflowContext, value: str) -> str:
    return await ctx.execute_task({snake}_step, value=value)
"#
            ),
            format!("add `import {snake}  # noqa: F401` to worker.py"),
        ),
        Language::TypeScript => (
            PathBuf::from(format!("src/{camel}.ts")),
            format!(
                r#"// The {name} workflow and its task.
import {{
  Task, Tasks, Workflow, createTaskRefs,
  type TaskContext, type WorkflowContext,
}} from '@orcher/sdk';

@Tasks()
export class {pascal}Tasks {{
  // Side effects go here: call an API or write to a database.
  @Task()
  async {camel}Step(_ctx: TaskContext, value: string): Promise<string> {{
    return `{name} handled ${{value}}`;
  }}
}}

export const {camel}Tasks = createTaskRefs({pascal}Tasks);

@Workflow({{ name: '{name}' }})
export class {pascal} {{
  async run(ctx: WorkflowContext, value: string): Promise<string> {{
    return ctx.executeTask({camel}Tasks.{camel}Step, value);
  }}
}}
"#
            ),
            format!("add `import './{camel}';` to src/worker.ts"),
        ),
        Language::Rust => (
            PathBuf::from(format!("src/{snake}.rs")),
            format!(
                r#"//! The {name} workflow and its task.

use orcher_sdk::prelude::*;

// Side effects go here: call an API or write to a database.
#[task]
pub async fn {snake}_step(_ctx: TaskContext, value: String) -> Result<String> {{
    Ok(format!("{name} handled {{value}}"))
}}

#[workflow(name = "{name}")]
pub async fn {snake}(ctx: WorkflowContext, value: String) -> Result<String> {{
    ctx.execute_task({snake}_step, value).await
}}
"#
            ),
            format!("add `mod {snake};` to src/main.rs"),
        ),
    }
}

/// The values a project template is filled in with
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
        "task_queue".to_string(),
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
        serde_json::Value::String(std::env::var("USER").unwrap_or_else(|_| "unknown".into())),
    );
    context.insert(
        "orcher_version".to_string(),
        serde_json::Value::String(VERSION.to_string()),
    );
    Ok(context)
}

/// Initialize a git repository in the project directory. The template's own
/// .gitignore covers its build output.
fn initialize_git_repo(project_dir: &Path) -> Result<()> {
    let output = std::process::Command::new("git")
        .arg("init")
        .arg("--quiet")
        .current_dir(project_dir)
        .output()
        .map_err(|e| {
            CliError::command_failed("git init", None, Some(format!("Git not found: {}", e)))
        })?;
    if !output.status.success() {
        return Err(CliError::command_failed(
            "git init",
            output.status.code(),
            Some(String::from_utf8_lossy(&output.stderr).to_string()),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_create_project_context() {
        let context = create_project_context("my-project", "rust").unwrap();
        assert_eq!(context.get("project_name").unwrap(), "my-project");
        assert_eq!(context.get("task_queue").unwrap(), "my-project");
        assert_eq!(context.get("template").unwrap(), "rust");
        assert!(context.contains_key("created_at"));
        assert!(context.contains_key("created_by"));
        assert!(context.contains_key("orcher_version"));
    }

    #[test]
    fn project_names_must_suit_every_package_manager() {
        assert!(validate_project_name("my-app2").is_ok());
        assert!(validate_project_name("2app").is_err());
        assert!(validate_project_name("My-App").is_err());
        assert!(validate_project_name("my_app").is_err());
    }

    #[test]
    fn languages_are_detected_from_project_files() {
        let dir = TempDir::new().unwrap();
        assert_eq!(Language::detect(dir.path()), None);
        std::fs::write(dir.path().join("requirements.txt"), "").unwrap();
        assert_eq!(Language::detect(dir.path()), Some(Language::Python));
        std::fs::write(dir.path().join("package.json"), "{}").unwrap();
        assert_eq!(Language::detect(dir.path()), Some(Language::TypeScript));
        std::fs::write(dir.path().join("Cargo.toml"), "").unwrap();
        assert_eq!(Language::detect(dir.path()), Some(Language::Rust));
        assert_eq!(Language::parse("ts").unwrap(), Language::TypeScript);
        assert!(Language::parse("cobol").is_err());
    }

    #[test]
    fn workflow_files_are_named_and_registered_per_language() {
        let (path, source, register) = workflow_source("ship-order", Language::Python);
        assert_eq!(path, PathBuf::from("ship_order.py"));
        assert!(source.contains("@workflow(name=\"ship-order\")"));
        assert!(source.contains("async def ship_order("));
        assert!(register.contains("import ship_order"));

        let (path, source, register) = workflow_source("ship-order", Language::TypeScript);
        assert_eq!(path, PathBuf::from("src/shipOrder.ts"));
        assert!(source.contains("@Workflow({ name: 'ship-order' })"));
        assert!(source.contains("export class ShipOrder {"));
        assert!(register.contains("import './shipOrder'"));

        let (path, source, register) = workflow_source("ship-order", Language::Rust);
        assert_eq!(path, PathBuf::from("src/ship_order.rs"));
        assert!(source.contains("#[workflow(name = \"ship-order\")]"));
        assert!(source.contains("pub async fn ship_order("));
        assert!(register.contains("mod ship_order;"));
    }

    #[tokio::test]
    async fn only_sequential_workflows_are_generated() {
        let dir = TempDir::new().unwrap();
        let file = dir.path().join("x.py").to_string_lossy().to_string();
        let global_config = GlobalConfig::default();
        let parallel = create_workflow(
            "x".to_string(),
            "parallel".to_string(),
            Some(file.clone()),
            Some("python".to_string()),
            &global_config,
        )
        .await;
        assert!(parallel.is_err());
        let sequential = create_workflow(
            "x".to_string(),
            "sequential".to_string(),
            Some(file.clone()),
            Some("python".to_string()),
            &global_config,
        )
        .await;
        assert!(sequential.is_ok());
        assert!(std::fs::read_to_string(&file)
            .unwrap()
            .contains("@workflow(name=\"x\")"));
    }
}
