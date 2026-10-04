//! The command tree: global options and every command, shared by the binary
//! and by `orcher completion`, which generates completions from it.

use crate::commands;
use crate::types::*;
use clap::{Parser, Subcommand};

/// ORCHER CLI - Workflow orchestration made simple
#[derive(Parser)]
#[command(name = "orcher")]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(about = "Command-line interface for ORCHER workflow orchestration platform")]
#[command(
    long_about = "Start, inspect and control ORCHER workflows on a local engine, a \
    self-hosted one or ORCHER Cloud."
)]
#[command(after_help = "\
EXAMPLES:
  orcher dev start                       Run a local engine in Docker
  orcher workflow list                   List recent workflow executions
  orcher workflow list -o json | jq .    Machine-readable output for scripting
  orcher workflow get <workflow-id>      Inspect a single workflow
  orcher logs <workflow-id> --follow     Stream a workflow's logs
  orcher workflow cancel <workflow-id>   Cancel a running workflow
  orcher namespace list                  List namespaces

Run 'orcher <command> --help' for command-specific examples.")]
pub struct Cli {
    /// Global configuration file path
    #[arg(long, global = true, env = "ORCHER_CONFIG")]
    pub config: Option<String>,

    /// Specify the context to use
    #[arg(long, global = true, env = "ORCHER_CONTEXT")]
    pub context: Option<String>,

    /// Specify the namespace
    #[arg(short, long, global = true, env = "ORCHER_NAMESPACE")]
    pub namespace: Option<String>,

    /// Output format (table, json, yaml, name)
    #[arg(short, long, global = true, default_value = "table")]
    pub output: String,

    /// Enable verbose logging
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Enable quiet mode (suppress non-essential output)
    #[arg(short, long, global = true)]
    pub quiet: bool,

    /// Disable colored output
    #[arg(long, global = true)]
    pub no_color: bool,

    /// No longer used: the engine does not use Redis
    #[arg(long, global = true, env = "ORCHER_REDIS_URL", hide = true)]
    pub redis_url: Option<String>,

    /// Server URL (gRPC endpoint) - overrides context config
    #[arg(long, global = true, env = "ORCHER_SERVER")]
    pub server: Option<String>,

    /// Gateway URL (HTTP, for a cloud deployment) - overrides context config
    #[arg(long, global = true, env = "ORCHER_API_URL")]
    pub api_url: Option<String>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Create new projects, workflows, or tasks
    #[command(aliases = ["n"])]
    New {
        #[command(subcommand)]
        resource: NewCommands,
    },

    /// Manage workflow executions
    #[command(aliases = ["wf"])]
    Workflow(commands::workflow::WorkflowCommand),

    /// Manage namespaces (admin)
    #[command(aliases = ["ns"])]
    Namespace(commands::namespace::NamespaceCommand),

    /// Perform bulk operations on matching workflows
    Batch(commands::batch::BatchCommand),

    /// Run the engine locally in Docker, and check any engine's health
    Server(commands::server::ServerCommand),

    /// View and manage task queues
    #[command(aliases = ["q"])]
    Queue(commands::queue::QueueCommand),

    /// Show logs for workflows or tasks
    Logs {
        /// Resource type and name (e.g., workflow/my-workflow)
        resource: String,

        /// Follow log output
        #[arg(short = 'f', long = "follow")]
        follow: bool,

        /// Show logs since time/duration
        #[arg(long)]
        since: Option<String>,

        /// Number of lines to show from end
        #[arg(long, default_value = "100")]
        tail: i32,

        /// Show event journal (workflow execution history)
        #[arg(long, conflicts_with = "tasks")]
        journal: bool,

        /// Show task execution details
        #[arg(long, conflicts_with = "journal")]
        tasks: bool,
    },

    /// Check a project builds, or run its workflows end to end
    Test {
        /// Project directory to test
        #[arg(default_value = ".")]
        workflow: String,

        /// Only check that the project builds; run nothing
        #[arg(long)]
        dry_run: bool,

        /// Namespace to test in
        #[arg(short = 'e', long = "env")]
        environment: Option<String>,

        /// Watch for file changes and re-test
        #[arg(short = 'w', long = "watch")]
        watch: bool,
    },

    /// Run a workflow on the orchestrator
    #[command(after_help = "\
EXAMPLES:
  orcher run my-workflow                         Run a registered workflow
  orcher run my-workflow -p key=value            Pass a parameter
  orcher run my-workflow -e staging -p n=5       Namespace + parameters")]
    Run {
        /// Workflow file or name
        workflow: String,

        /// Namespace to run in
        #[arg(short = 'e', long = "env")]
        environment: Option<String>,

        /// Parameters to pass to workflow
        #[arg(short = 'p', long = "param")]
        params: Vec<String>,
    },

    /// Manage CLI configuration
    Config {
        #[command(subcommand)]
        action: ConfigCommands,
    },

    /// Manage authentication
    Auth {
        #[command(subcommand)]
        action: AuthCommands,
    },

    /// Show status of resources
    Status {
        /// Resource type (optional)
        resource_type: Option<String>,

        /// Show all resources
        #[arg(short = 'A', long = "all")]
        all: bool,
    },

    /// Run a local engine in Docker: the quick local loop
    #[command(name = "dev")]
    Development {
        #[command(subcommand)]
        action: DevCommands,
    },

    /// Generate shell completion scripts
    Completion {
        /// Shell to generate completions for (bash, zsh, fish, powershell, elvish)
        shell: Option<String>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    /// Catches clashing flags and other mistakes clap only finds at run time,
    /// such as a subcommand's `-n` shadowing the global `--namespace`.
    #[test]
    fn the_command_tree_is_valid() {
        Cli::command().debug_assert();
    }

    /// A subcommand flag sharing an id with a global one parses as the wrong
    /// type and panics once the global is set: `server start --config` did.
    #[test]
    fn global_flags_and_subcommand_flags_do_not_collide() {
        let parsed = Cli::try_parse_from([
            "orcher",
            "--config",
            "/tmp/cli.yaml",
            "server",
            "start",
            "--engine-config",
            "/tmp/engine.toml",
        ]);
        assert!(parsed.is_ok());

        // `new workflow` once had an `output` field, which took the global
        // --output's value ("table") as the file to write.
        let parsed = Cli::try_parse_from(["orcher", "new", "workflow", "ship-order"]).unwrap();
        match parsed.command {
            Commands::New {
                resource: NewCommands::Workflow { file, .. },
            } => assert_eq!(file, None),
            _ => panic!("parsed as another command"),
        }
    }
}
