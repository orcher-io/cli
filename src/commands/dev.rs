//! `orcher dev`: the quick local loop. Runs the engine in Docker with the
//! defaults a developer wants; `orcher server` manages the same engine with
//! more control.

use crate::error::Result;
use crate::local_engine::{self, Container, EngineOptions};
use crate::types::DevCommands;
use crate::utils::GlobalConfig;
use std::time::Duration;

/// Execute the dev command
pub async fn execute(action: DevCommands, global_config: &GlobalConfig) -> Result<()> {
    match action {
        DevCommands::Start {
            port,
            http_port,
            host,
            engine_version,
            image,
            timeout,
            open,
            no_ui,
        } => {
            let opts = EngineOptions {
                image: EngineOptions::image_for(&engine_version, image.as_deref()),
                grpc_port: port,
                http_port,
                bind_host: host,
                log_level: "info".to_string(),
                timeout: Duration::from_secs(timeout),
                database_url: None,
                config_file: None,
                ui: !no_ui,
            };
            start(&opts, global_config, open).await
        }
        DevCommands::Stop { delete_data } => {
            let outcome = local_engine::stop(delete_data, None)?;
            if !global_config.quiet {
                println!("{}", outcome.message());
            }
            Ok(())
        }
        DevCommands::Status => status(global_config).await,
        DevCommands::Logs {
            follow,
            tail,
            postgres,
        } => local_engine::logs(follow, &tail, postgres),
    }
}

/// Starts the engine and reports where it is, unless quiet; with `open`, opens
/// its dashboard in a browser.
pub async fn start(opts: &EngineOptions, global_config: &GlobalConfig, open: bool) -> Result<()> {
    let structured = global_config.is_structured_output();
    local_engine::start(opts, global_config.quiet || structured).await?;
    let (engine, postgres) = local_engine::status()?;
    let dashboard = dashboard_of(engine.as_ref()).await;
    if !global_config.quiet {
        report(
            global_config,
            engine.as_ref(),
            postgres.as_ref(),
            dashboard.as_deref(),
        )?;
        if !structured {
            local_engine::print_started(opts, dashboard.as_deref());
        }
    }
    if open {
        match &dashboard {
            Some(url) => {
                if open::that(url).is_err() {
                    eprintln!("Could not open a browser; the dashboard is at {url}");
                }
            }
            None => eprintln!("There is no dashboard to open."),
        }
    }
    Ok(())
}

/// Prints the local engine's state: a summary, or JSON/YAML.
pub async fn status(global_config: &GlobalConfig) -> Result<()> {
    let (engine, postgres) = local_engine::status()?;
    let dashboard = dashboard_of(engine.as_ref()).await;
    report(
        global_config,
        engine.as_ref(),
        postgres.as_ref(),
        dashboard.as_deref(),
    )
}

async fn dashboard_of(engine: Option<&Container>) -> Option<String> {
    match engine {
        Some(engine) => engine.dashboard().await,
        None => None,
    }
}

fn report(
    global_config: &GlobalConfig,
    engine: Option<&Container>,
    postgres: Option<&Container>,
    dashboard: Option<&str>,
) -> Result<()> {
    match global_config.output_format.as_str() {
        "json" => println!(
            "{}",
            serde_json::to_string_pretty(&local_engine::status_json(engine, postgres, dashboard))?
        ),
        "yaml" => print!(
            "{}",
            serde_yaml::to_string(&local_engine::status_json(engine, postgres, dashboard))?
        ),
        "name" => {
            if engine.is_some_and(|e| e.running) {
                println!("{}", local_engine::ENGINE);
            }
        }
        _ => local_engine::print_status(engine, postgres, dashboard),
    }
    Ok(())
}
