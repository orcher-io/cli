//! Project templates for `orcher new`: a small ORCHER project in Python,
//! TypeScript or Rust, built on the published SDK for that language.
//!
//! Each template is the same project: a `hello` workflow that runs a `greet`
//! task, a worker, a starter, and a test that runs the workflow end to end
//! against an engine. The files live beside this module and are compiled into
//! the binary; `{{project_name}}` and `{{task_queue}}` are filled in when a
//! project is generated.

use crate::error::{CliError, Result};
use handlebars::Handlebars;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// A template's files: where each goes in the project, and its contents.
type Files = &'static [(&'static str, &'static str)];

const PYTHON: Files = &[
    ("README.md", include_str!("python/README.md")),
    (".gitignore", include_str!("python/gitignore")),
    ("requirements.txt", include_str!("python/requirements.txt")),
    ("settings.py", include_str!("python/settings.py")),
    ("workflows.py", include_str!("python/workflows.py")),
    ("worker.py", include_str!("python/worker.py")),
    ("start.py", include_str!("python/start.py")),
    ("test_workflow.py", include_str!("python/test_workflow.py")),
];

const TYPESCRIPT: Files = &[
    ("README.md", include_str!("typescript/README.md")),
    (".gitignore", include_str!("typescript/gitignore")),
    ("package.json", include_str!("typescript/package.json")),
    ("tsconfig.json", include_str!("typescript/tsconfig.json")),
    (
        "src/settings.ts",
        include_str!("typescript/src/settings.ts"),
    ),
    (
        "src/workflows.ts",
        include_str!("typescript/src/workflows.ts"),
    ),
    ("src/worker.ts", include_str!("typescript/src/worker.ts")),
    ("src/start.ts", include_str!("typescript/src/start.ts")),
    ("src/test.ts", include_str!("typescript/src/test.ts")),
];

const RUST: Files = &[
    ("README.md", include_str!("rust/README.md")),
    (".gitignore", include_str!("rust/gitignore")),
    ("Cargo.toml", include_str!("rust/Cargo.toml")),
    ("src/workflows.rs", include_str!("rust/src/workflows.rs")),
    ("src/main.rs", include_str!("rust/src/main.rs")),
];

/// The built-in templates: name, description, files.
const TEMPLATES: &[(&str, &str, Files)] = &[
    (
        "python",
        "Python, with the orcher-sdk package from PyPI",
        PYTHON,
    ),
    (
        "typescript",
        "TypeScript on Node.js, with @orcher/sdk from npm",
        TYPESCRIPT,
    ),
    ("rust", "Rust, with the orcher-sdk crate", RUST),
];

/// Template engine for generating projects
pub struct TemplateEngine {
    handlebars: Handlebars<'static>,
}

/// Built-in template information
#[derive(Debug, Clone, Serialize, Deserialize)]
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
        // A placeholder without a value is an error, not an empty string.
        handlebars.set_strict_mode(true);
        handlebars.register_escape_fn(handlebars::no_escape);
        Ok(Self { handlebars })
    }

    /// Check if a template exists (aliases included)
    pub fn template_exists(&self, template_name: &str) -> Result<bool> {
        Ok(self.files(template_name).is_some())
    }

    /// Generate a project from a template into `output_dir`.
    ///
    /// The context needs `project_name`; `task_queue` defaults to it.
    pub fn generate_project(
        &self,
        template_name: &str,
        context: &HashMap<String, serde_json::Value>,
        output_dir: &Path,
    ) -> Result<()> {
        let files = self
            .files(template_name)
            .ok_or_else(|| CliError::not_found("template", template_name.to_string()))?;
        let mut context = context.clone();
        if !context.contains_key("task_queue") {
            let name = context
                .get("project_name")
                .cloned()
                .ok_or_else(|| CliError::template("project_name is required"))?;
            context.insert("task_queue".to_string(), name);
        }

        for (path, contents) in files {
            let rendered = self
                .handlebars
                .render_template(contents, &context)
                .map_err(|e| {
                    CliError::template_with_name(
                        format!("Cannot render {path}: {e}"),
                        template_name,
                    )
                })?;
            let target = output_dir.join(path);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    CliError::io_with_path(
                        "Failed to create directory",
                        parent.display().to_string(),
                        e,
                    )
                })?;
            }
            std::fs::write(&target, rendered).map_err(|e| {
                CliError::io_with_path("Failed to write file", target.display().to_string(), e)
            })?;
        }
        Ok(())
    }

    /// List all available templates
    pub fn list_templates(&self) -> Result<Vec<TemplateInfo>> {
        Ok(TEMPLATES
            .iter()
            .map(|(name, description, files)| TemplateInfo {
                name: name.to_string(),
                description: description.to_string(),
                category: "language".to_string(),
                files: files.iter().map(|(path, _)| path.to_string()).collect(),
            })
            .collect())
    }

    /// Get template information (aliases included)
    pub fn get_template_info(&self, template_name: &str) -> Result<TemplateInfo> {
        let canonical = self.resolve_template_alias(template_name);
        self.list_templates()?
            .into_iter()
            .find(|t| t.name == canonical)
            .ok_or_else(|| CliError::not_found("template", template_name))
    }

    /// Resolve a template alias to its name: `py`, `ts`, `node`, `rs`, and
    /// the earlier names `basic-workflow` and `default`, which were Rust.
    pub fn resolve_template_alias(&self, template_name: &str) -> String {
        match template_name.to_lowercase().as_str() {
            "py" => "python".to_string(),
            "ts" | "node" | "nodejs" | "javascript" | "js" => "typescript".to_string(),
            "rs" | "basic-workflow" | "default" => "rust".to_string(),
            other => other.to_string(),
        }
    }

    fn files(&self, template_name: &str) -> Option<Files> {
        let canonical = self.resolve_template_alias(template_name);
        TEMPLATES
            .iter()
            .find(|(name, _, _)| *name == canonical)
            .map(|(_, _, files)| *files)
    }
}

impl Default for TemplateEngine {
    fn default() -> Self {
        Self::new().expect("Failed to create template engine")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn context(name: &str) -> HashMap<String, serde_json::Value> {
        HashMap::from([("project_name".to_string(), serde_json::json!(name))])
    }

    #[test]
    fn test_template_engine_creation() {
        assert!(TemplateEngine::new().is_ok());
    }

    #[test]
    fn test_builtin_templates() {
        let engine = TemplateEngine::new().unwrap();
        for name in [
            "python",
            "typescript",
            "rust",
            "py",
            "ts",
            "rs",
            "basic-workflow",
        ] {
            assert!(engine.template_exists(name).unwrap(), "{name}");
        }
        // The earlier templates generated code for a private workspace.
        assert!(!engine.template_exists("microservices").unwrap());
        assert!(!engine.template_exists("nonexistent").unwrap());
    }

    #[test]
    fn test_list_builtin_templates() {
        let engine = TemplateEngine::new().unwrap();
        let names: Vec<_> = engine
            .list_templates()
            .unwrap()
            .into_iter()
            .map(|t| t.name)
            .collect();
        assert_eq!(names, ["python", "typescript", "rust"]);
        assert_eq!(engine.get_template_info("ts").unwrap().name, "typescript");
    }

    #[test]
    fn every_template_renders_with_its_names_filled_in() {
        let engine = TemplateEngine::new().unwrap();
        for template in ["python", "typescript", "rust"] {
            let dir = TempDir::new().unwrap();
            engine
                .generate_project(template, &context("my-app"), dir.path())
                .unwrap();
            let info = engine.get_template_info(template).unwrap();
            for file in &info.files {
                let text = std::fs::read_to_string(dir.path().join(file)).unwrap();
                assert!(
                    !text.contains("{{"),
                    "{template}/{file} has a placeholder left"
                );
            }
            let readme = std::fs::read_to_string(dir.path().join("README.md")).unwrap();
            assert!(readme.starts_with("# my-app"), "{template}");
            assert!(dir.path().join(".gitignore").exists(), "{template}");
        }
    }

    #[test]
    fn projects_use_the_published_sdks() {
        let engine = TemplateEngine::new().unwrap();
        let dir = TempDir::new().unwrap();
        engine
            .generate_project("rust", &context("my-app"), dir.path())
            .unwrap();
        let cargo = std::fs::read_to_string(dir.path().join("Cargo.toml")).unwrap();
        assert!(cargo.contains("name = \"my-app\""));
        assert!(cargo.contains("orcher-sdk = \"0.5\""));
        assert!(!cargo.contains("path ="), "no path dependencies");

        let dir = TempDir::new().unwrap();
        engine
            .generate_project("typescript", &context("my-app"), dir.path())
            .unwrap();
        let package = std::fs::read_to_string(dir.path().join("package.json")).unwrap();
        let package: serde_json::Value = serde_json::from_str(&package).unwrap();
        assert_eq!(package["name"], "my-app");
        assert!(package["dependencies"]["@orcher/sdk"].is_string());

        let dir = TempDir::new().unwrap();
        engine
            .generate_project("python", &context("my-app"), dir.path())
            .unwrap();
        let requirements = std::fs::read_to_string(dir.path().join("requirements.txt")).unwrap();
        assert!(requirements.starts_with("orcher-sdk"));
        let settings = std::fs::read_to_string(dir.path().join("settings.py")).unwrap();
        assert!(settings.contains("TASK_QUEUE = \"my-app\""));
    }

    #[test]
    fn a_project_needs_a_name() {
        let engine = TemplateEngine::new().unwrap();
        let dir = TempDir::new().unwrap();
        assert!(engine
            .generate_project("python", &HashMap::new(), dir.path())
            .is_err());
    }
}
