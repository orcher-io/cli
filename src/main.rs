//! `orcher`, the ORCHER command-line interface.

mod client;
mod commands;
mod error;
mod render;
mod settings;

use clap::{Parser, Subcommand};
use console::style;
use settings::GlobalArgs;

#[derive(Parser)]
#[command(
    name = "orcher",
    version,
    about = "Start and inspect ORCHER workflows",
    long_about = "Start and inspect ORCHER workflows.\n\nCommands talk to the engine at --server \
                  (default http://localhost:50051) over gRPC.",
    propagate_version = true,
    after_help = "\
Examples:
  orcher dev start                         Run a local engine in Docker
  orcher workflow list                     Recent workflow executions
  orcher workflow list -o json | jq .      The same, for scripts
  orcher workflow describe order-1001      One workflow in detail
  orcher logs order-1001 --follow          Its log, as it runs
  orcher namespace list                    Namespaces on the engine"
)]
pub struct Cli {
    #[command(flatten)]
    global: GlobalArgs,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run a local engine in Docker for development
    Dev(commands::dev::DevCommand),

    /// Inspect and control workflow executions
    #[command(alias = "wf")]
    Workflow(commands::workflow::WorkflowCommand),

    /// Show a workflow's log
    Logs(commands::logs::LogsCommand),

    /// Manage namespaces
    #[command(alias = "ns")]
    Namespace(commands::namespace::NamespaceCommand),

    /// Write a shell completion script to stdout
    Completion(commands::completion::CompletionCommand),
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    if cli.global.no_color {
        console::set_colors_enabled(false);
        console::set_colors_enabled_stderr(false);
    }

    let result = match cli.command {
        Command::Dev(cmd) => commands::dev::run(cmd, &cli.global).await,
        Command::Workflow(cmd) => commands::workflow::run(cmd, &cli.global).await,
        Command::Logs(cmd) => commands::logs::run(cmd, &cli.global).await,
        Command::Namespace(cmd) => commands::namespace::run(cmd, &cli.global).await,
        Command::Completion(cmd) => {
            commands::completion::run(cmd);
            Ok(())
        }
    };

    if let Err(err) = result {
        eprintln!("{} {err}", style("Error:").red().bold());
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    /// Catches clashing flags and other mistakes clap only finds at run time.
    #[test]
    fn the_command_tree_is_valid() {
        Cli::command().debug_assert();
    }
}
