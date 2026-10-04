//! ORCHER CLI entry point.

use clap::Parser;
use console::style;
use orcher_cli::cli::{Cli, Commands};
use orcher_cli::{commands, error, utils};
use std::process;
use tracing::Level;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    // Every reader of the config file finds it through ORCHER_CONFIG, so a
    // --config flag is passed on that way.
    if let Some(path) = &cli.config {
        std::env::set_var(orcher_cli::constants::CONFIG_FILE_ENV, path);
    }

    // Honor --no-color globally for all console styling (tables, status cells).
    if cli.no_color {
        console::set_colors_enabled(false);
        console::set_colors_enabled_stderr(false);
    }

    // A closed pipe (`orcher ... | head`) ends the process quietly, as for
    // any command-line tool, instead of panicking on the next write.
    #[cfg(unix)]
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }

    // Diagnostics go to stderr, so stdout carries only the command's output
    // (JSON included). They are off unless asked for with -v or RUST_LOG.
    let log_level = if cli.verbose {
        Level::DEBUG
    } else {
        Level::WARN
    };

    let subscriber = tracing_subscriber::registry().with(
        tracing_subscriber::EnvFilter::from_default_env()
            .add_directive(format!("orcher_cli={}", log_level).parse().unwrap()),
    );

    if cli.output == "json" {
        subscriber
            .with(
                tracing_subscriber::fmt::layer()
                    .json()
                    .with_target(false)
                    .with_writer(std::io::stderr),
            )
            .init();
    } else {
        subscriber
            .with(
                tracing_subscriber::fmt::layer()
                    .with_target(false)
                    .compact()
                    .with_ansi(!cli.no_color)
                    .with_writer(std::io::stderr),
            )
            .init();
    }

    // Set up global configuration
    let global_config = utils::GlobalConfig {
        config_path: cli.config,
        context: cli.context,
        namespace: cli.namespace,
        profile: None, // Profile can be specified per command if needed
        output_format: cli.output,
        verbose: cli.verbose,
        quiet: cli.quiet,
        no_color: cli.no_color,
        redis_url: cli.redis_url,
        server: cli.server,
        api_url: cli.api_url,
    };

    // Execute command
    // Each command returns a CliError whose Display is already user-facing;
    // dispatch propagates it directly (no wrapping) so the message stays clean.
    let result: error::Result<()> = match cli.command {
        Commands::New { resource } => commands::new::execute(resource, &global_config).await,
        Commands::Batch(cmd) => commands::batch::execute(cmd, &global_config).await,
        Commands::Workflow(cmd) => commands::workflow::execute(cmd, &global_config).await,
        Commands::Namespace(cmd) => commands::namespace::execute(cmd, &global_config).await,
        Commands::Server(cmd) => commands::server::execute(cmd, &global_config).await,
        Commands::Queue(cmd) => commands::queue::execute(cmd, &global_config).await,
        Commands::Logs {
            resource,
            follow,
            since,
            tail,
            journal,
            tasks,
        } => {
            commands::logs::execute(
                resource,
                follow,
                since,
                tail,
                journal,
                tasks,
                &global_config,
            )
            .await
        }
        Commands::Test {
            workflow,
            dry_run,
            environment,
            watch,
        } => commands::test::execute(workflow, dry_run, environment, watch, &global_config).await,
        Commands::Run {
            workflow,
            environment,
            params,
        } => commands::run::execute(workflow, environment, params, &global_config).await,
        Commands::Config { action } => commands::config::execute(action, &global_config).await,
        Commands::Auth { action } => commands::auth::execute(action, &global_config).await,
        Commands::Status { resource_type, all } => {
            commands::status::execute(resource_type, all, &global_config).await
        }
        Commands::Development { action } => commands::dev::execute(action, &global_config).await,
        Commands::Completion { shell } => {
            commands::completion::execute(shell, &global_config).await
        }
    };

    if let Err(e) = result {
        // Print a clean, single-line error to stderr (not a tracing log line).
        // Errors are essential, so they show even in --quiet.
        eprintln!("{} {}", style("Error:").red().bold(), e);
        if global_config.verbose {
            eprintln!("{} {:#?}", style("Caused by:").dim(), e);
        }
        process::exit(1);
    }
}
