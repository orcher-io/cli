//! `orcher dev`: the quick local loop. Runs the engine in Docker with the
//! defaults a developer wants; `orcher server` manages the same engine with
//! more control.

use crate::error::Result;
use crate::local_engine::{self, EngineOptions};
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
            };
            start(&opts, global_config).await
        }
        DevCommands::Stop { delete_data } => {
            let outcome = local_engine::stop(delete_data, None)?;
            if !global_config.quiet {
                println!("{}", outcome.message());
            }
            Ok(())
        }
        DevCommands::Status => status(global_config),
        DevCommands::Logs {
            follow,
            tail,
            postgres,
        } => local_engine::logs(follow, &tail, postgres),
    }
}

/// Starts the engine and reports where it is, unless quiet.
pub async fn start(opts: &EngineOptions, global_config: &GlobalConfig) -> Result<()> {
    let structured = global_config.is_structured_output();
    local_engine::start(opts, global_config.quiet || structured).await?;
    if global_config.quiet {
        return Ok(());
    }
    status(global_config)?;
    if !structured {
        local_engine::print_started(opts);
    }
    Ok(())
}

/// Prints the local engine's state: a summary, or JSON/YAML.
pub fn status(global_config: &GlobalConfig) -> Result<()> {
    let (engine, postgres) = local_engine::status()?;
    let value = local_engine::status_json(engine.as_ref(), postgres.as_ref());
    match global_config.output_format.as_str() {
        "json" => println!("{}", serde_json::to_string_pretty(&value)?),
        "yaml" => print!("{}", serde_yaml::to_string(&value)?),
        "name" => {
            if engine.as_ref().is_some_and(|e| e.running) {
                println!("{}", local_engine::ENGINE);
            }
        }
        _ => local_engine::print_status(engine.as_ref(), postgres.as_ref()),
    }
    Ok(())
}
