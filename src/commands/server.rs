//! `orcher server`: run and check an ORCHER engine.
//!
//! `start`, `stop` and `restart` manage the local engine in Docker, the same
//! one `orcher dev` runs, with more control: a config file, an existing
//! database, the log level, a graceful stop. `status` and `version` check any
//! engine by address, local or remote, and a cloud gateway when one is
//! configured.

use crate::client::connection::{
    probe_grpc, probe_http, ConnectionManager, GATEWAY_HEALTH_PATH, ORCHESTRATOR_HEALTH_PATH,
};
use crate::error::{CliError, Result};
use crate::local_engine::{self, EngineOptions};
use crate::utils::GlobalConfig;
use clap::{Args, Subcommand};
use std::path::PathBuf;
use std::time::Duration;
use tracing::info;

/// Server management command
#[derive(Args)]
pub struct ServerCommand {
    #[command(subcommand)]
    pub action: ServerCommands,
}

#[derive(Subcommand)]
pub enum ServerCommands {
    /// Start the engine in Docker, with Postgres beside it unless --database-url is given
    Start {
        /// Engine config file (TOML), for settings such as authentication.
        /// (`--config` is the CLI's own config file.)
        #[arg(short = 'c', long = "engine-config")]
        engine_config: Option<PathBuf>,

        /// Host port for gRPC (default: 50051)
        #[arg(long = "grpc-port", default_value = "50051")]
        grpc_port: u16,

        /// Host port for HTTP health and metrics (default: 8080)
        #[arg(long = "http-port", default_value = "8080")]
        http_port: u16,

        /// Host address to publish the ports on. The engine runs without
        /// authentication unless its config turns it on, so the default keeps
        /// it on this machine.
        #[arg(long = "host", default_value = "127.0.0.1")]
        host: String,

        /// Stay attached and print the engine's log until interrupted
        #[arg(short = 'f', long = "foreground")]
        foreground: bool,

        /// No longer used: the engine serves gRPC and HTTP together
        #[arg(long = "grpc-only", hide = true, conflicts_with = "http_only")]
        grpc_only: bool,

        /// No longer used: the engine serves gRPC and HTTP together
        #[arg(long = "http-only", hide = true, conflicts_with = "grpc_only")]
        http_only: bool,

        /// Use this PostgreSQL database instead of starting one
        #[arg(long = "database-url", env = "DATABASE_URL")]
        database_url: Option<String>,

        /// No longer used: the engine does not use Redis
        #[arg(long = "redis-url", hide = true)]
        redis_url: Option<String>,

        /// Log level (trace, debug, info, warn, error)
        #[arg(long = "log-level", default_value = "info")]
        log_level: String,

        /// Engine release to run
        #[arg(long, env = "ORCHER_ENGINE_VERSION", default_value = local_engine::DEFAULT_ENGINE_VERSION)]
        engine_version: String,

        /// Engine image to run, overriding --engine-version (for a mirror, say)
        #[arg(long, env = "ORCHER_ENGINE_IMAGE")]
        image: Option<String>,

        /// Seconds to wait for the engine to become ready
        #[arg(long, default_value = "120")]
        timeout: u64,
    },

    /// Stop the local engine
    Stop {
        /// No longer used: the engine serves gRPC and HTTP together
        #[arg(long = "grpc-only", hide = true, conflicts_with = "http_only")]
        grpc_only: bool,

        /// No longer used: the engine serves gRPC and HTTP together
        #[arg(long = "http-only", hide = true, conflicts_with = "grpc_only")]
        http_only: bool,

        /// Stop at once, without a graceful shutdown
        #[arg(short = 'f', long = "force")]
        force: bool,

        /// Seconds to allow for a graceful shutdown
        #[arg(long = "timeout", default_value = "30")]
        timeout: u32,

        /// Also delete the database: every workflow the engine ran
        #[arg(long)]
        delete_data: bool,
    },

    /// Check that an engine, and a gateway if one is configured, answer
    Status {
        /// Show detailed status information
        #[arg(short = 'v', long = "verbose")]
        verbose: bool,

        /// Output format (table, json, yaml)
        #[arg(short = 'o', long = "output", default_value = "table")]
        output: String,

        /// Engine gRPC address (default: the --server in use)
        #[arg(long = "grpc-addr")]
        grpc_addr: Option<String>,

        /// Engine HTTP health address (default: the local engine's, or
        /// http://localhost:8080)
        #[arg(long = "orchestrator-http")]
        orchestrator_http_addr: Option<String>,

        /// Gateway HTTP address (default: the context's API URL, if any)
        #[arg(long = "gateway-http")]
        gateway_http_addr: Option<String>,
    },

    /// Restart the local engine with the settings it was started with
    Restart {
        /// No longer used: the engine serves gRPC and HTTP together
        #[arg(long = "grpc-only", hide = true, conflicts_with = "http_only")]
        grpc_only: bool,

        /// No longer used: the engine serves gRPC and HTTP together
        #[arg(long = "http-only", hide = true, conflicts_with = "grpc_only")]
        http_only: bool,

        /// Restart at once, without a graceful shutdown
        #[arg(short = 'f', long = "force")]
        force: bool,
    },

    /// Show the CLI's version and the engine's
    Version {
        /// Engine HTTP health address (default: the local engine's, or
        /// http://localhost:8080)
        #[arg(long = "orchestrator-http")]
        orchestrator_http_addr: Option<String>,

        /// Gateway HTTP address (default: the context's API URL, if any)
        #[arg(long = "gateway-http")]
        gateway_http_addr: Option<String>,
    },
}

/// Execute server management command
pub async fn execute(cmd: ServerCommand, global_config: &GlobalConfig) -> Result<()> {
    match cmd.action {
        ServerCommands::Start {
            engine_config,
            grpc_port,
            http_port,
            host,
            foreground,
            grpc_only,
            http_only,
            database_url,
            redis_url,
            log_level,
            engine_version,
            image,
            timeout,
        } => {
            warn_unused(grpc_only || http_only, redis_url.is_some());
            if let (Some(url), false) = (&database_url, global_config.quiet) {
                eprintln!("Using the database at {}", mask_url_password(url));
            }
            let opts = EngineOptions {
                image: EngineOptions::image_for(&engine_version, image.as_deref()),
                grpc_port,
                http_port,
                bind_host: host,
                log_level,
                timeout: Duration::from_secs(timeout),
                database_url,
                config_file: engine_config,
            };
            crate::commands::dev::start(&opts, global_config).await?;
            if foreground {
                local_engine::logs(true, "0", false)?;
            }
            Ok(())
        }
        ServerCommands::Stop {
            grpc_only,
            http_only,
            force,
            timeout,
            delete_data,
        } => {
            warn_unused(grpc_only || http_only, false);
            let graceful = (!force).then(|| Duration::from_secs(timeout.into()));
            let outcome = local_engine::stop(delete_data, graceful)?;
            if !global_config.quiet {
                println!("{}", outcome.message());
            }
            Ok(())
        }
        ServerCommands::Status {
            verbose,
            output,
            grpc_addr,
            orchestrator_http_addr,
            gateway_http_addr,
        } => {
            let manager = ConnectionManager::from_config(global_config);
            let grpc_addr = grpc_addr.unwrap_or_else(|| manager.grpc_addr().to_string());
            let gateway = gateway_http_addr.or_else(|| manager.configured_http_addr());
            server_status(
                verbose,
                output,
                grpc_addr,
                orchestrator_http_addr.unwrap_or_else(local_health_addr),
                gateway,
                global_config,
            )
            .await
        }
        ServerCommands::Restart {
            grpc_only,
            http_only,
            force,
        } => {
            warn_unused(grpc_only || http_only, false);
            restart_server(force, global_config).await
        }
        ServerCommands::Version {
            orchestrator_http_addr,
            gateway_http_addr,
        } => {
            let manager = ConnectionManager::from_config(global_config);
            let gateway = gateway_http_addr.or_else(|| manager.configured_http_addr());
            server_version(
                orchestrator_http_addr.unwrap_or_else(local_health_addr),
                gateway,
                global_config,
            )
            .await
        }
    }
}

/// The local engine's HTTP address when it is running, else the default.
fn local_health_addr() -> String {
    let port = local_engine::status()
        .ok()
        .and_then(|(engine, _)| engine)
        .filter(|e| e.running)
        .and_then(|e| e.http_port)
        .unwrap_or(local_engine::ENGINE_HTTP_PORT);
    format!("http://localhost:{}", port)
}

/// Says so when a flag kept for compatibility has no effect any more.
fn warn_unused(service_selection: bool, redis: bool) {
    if service_selection {
        eprintln!("Note: --grpc-only and --http-only no longer apply; the engine serves both.");
    }
    if redis {
        eprintln!("Note: --redis-url no longer applies; the engine does not use Redis.");
    }
}

/// Service status information
#[derive(Debug, Clone)]
struct ServiceStatus {
    name: String,
    address: String,
    healthy: bool,
    error: Option<String>,
}

/// Check if a service is healthy via its HTTP health endpoint.
///
/// The health path differs by service: the orchestrator serves `/health` at
/// its root, while the gateway nests routes under `/api/v1` (liveness at
/// `/api/v1/health/live`) — so callers pass the correct path per service.
async fn check_service_health(name: &str, address: &str, health_path: &str) -> ServiceStatus {
    let (healthy, error) = match probe_http(address, health_path).await {
        Ok(()) => (true, None),
        Err(e) => (false, Some(e)),
    };
    ServiceStatus {
        name: name.to_string(),
        address: address.to_string(),
        healthy,
        error,
    }
}

/// Check if gRPC service is healthy via tonic
async fn check_grpc_health(address: &str) -> ServiceStatus {
    let (healthy, error) = match probe_grpc(address).await {
        Ok(()) => (true, None),
        Err(e) => (false, Some(e)),
    };
    ServiceStatus {
        name: "Orchestrator (gRPC)".to_string(),
        address: address.to_string(),
        healthy,
        error,
    }
}

/// Checks the engine at `grpc_addr` and `orchestrator_http_addr`, and the
/// gateway when one is configured.
async fn server_status(
    verbose: bool,
    output: String,
    grpc_addr: String,
    orchestrator_http_addr: String,
    gateway_http_addr: Option<String>,
    global_config: &GlobalConfig,
) -> Result<()> {
    info!("Checking ORCHER services status");

    let gateway_check = async {
        match &gateway_http_addr {
            Some(addr) => {
                Some(check_service_health("Gateway (HTTP)", addr, GATEWAY_HEALTH_PATH).await)
            }
            None => None,
        }
    };
    let (grpc_status, orchestrator_http_status, gateway_status) = tokio::join!(
        check_grpc_health(&grpc_addr),
        check_service_health(
            "Orchestrator (Health)",
            &orchestrator_http_addr,
            ORCHESTRATOR_HEALTH_PATH,
        ),
        gateway_check
    );

    // The engine is up if either its gRPC port or its health endpoint answers.
    let orchestrator_healthy = grpc_status.healthy || orchestrator_http_status.healthy;
    // A gateway that is not configured does not count against the total.
    let gateway_healthy = gateway_status.as_ref().map_or(true, |g| g.healthy);
    let all_healthy = orchestrator_healthy && gateway_healthy;

    let gateway_json = match &gateway_status {
        Some(g) => serde_json::json!({
            "configured": true,
            "address": g.address,
            "healthy": g.healthy,
            "error": g.error
        }),
        None => serde_json::json!({ "configured": false }),
    };
    let status = serde_json::json!({
        "services": {
            "orchestrator": {
                "grpc": {
                    "address": grpc_status.address,
                    "healthy": grpc_status.healthy,
                    "error": grpc_status.error
                },
                "http": {
                    "address": orchestrator_http_status.address,
                    "healthy": orchestrator_http_status.healthy,
                    "error": orchestrator_http_status.error
                },
                "overall_healthy": orchestrator_healthy
            },
            "gateway": gateway_json
        },
        "all_healthy": all_healthy
    });

    match output.as_str() {
        "json" => println!("{}", serde_json::to_string_pretty(&status)?),
        "yaml" => print!("{}", serde_yaml::to_string(&status)?),
        _ => {
            println!("ORCHER Services Status");
            println!("======================");
            println!();
            println!("Orchestrator:");
            print_service_status_inline("  gRPC", &grpc_status, verbose, global_config);
            print_service_status_inline(
                "  Health",
                &orchestrator_http_status,
                verbose,
                global_config,
            );
            if orchestrator_healthy {
                if global_config.use_colors() {
                    println!("  {} Overall: Healthy", console::style("✓").green());
                } else {
                    println!("  [OK] Overall: Healthy");
                }
            } else if global_config.use_colors() {
                println!("  {} Overall: Offline", console::style("✗").red());
            } else {
                println!("  [OFFLINE] Overall: Offline");
            }
            println!();
            println!("Gateway:");
            match &gateway_status {
                Some(g) => print_service_status_inline("  HTTP", g, verbose, global_config),
                None => {
                    println!("  not configured (pass --gateway-http, or log in to ORCHER Cloud)")
                }
            }
            println!();
            if all_healthy {
                if global_config.use_colors() {
                    println!("{} All services healthy", console::style("✓").green());
                } else {
                    println!("[OK] All services healthy");
                }
            } else if orchestrator_healthy {
                if global_config.use_colors() {
                    println!("{} Some services unhealthy", console::style("!").yellow());
                } else {
                    println!("[WARN] Some services unhealthy");
                }
            } else {
                if global_config.use_colors() {
                    println!("{} All services offline", console::style("✗").red());
                } else {
                    println!("[ERROR] All services offline");
                }
                println!();
                println!("Start a local engine with: orcher server start");
            }
        }
    }

    Ok(())
}

/// Print status for a single service (block format)
#[allow(dead_code)]
fn print_service_status(status: &ServiceStatus, verbose: bool, global_config: &GlobalConfig) {
    let indicator = if status.healthy {
        if global_config.use_colors() {
            console::style("●").green().to_string()
        } else {
            "[OK]".to_string()
        }
    } else if global_config.use_colors() {
        console::style("●").red().to_string()
    } else {
        "[OFFLINE]".to_string()
    };

    println!("{} {}", indicator, status.name);
    println!("  Address: {}", status.address);
    println!(
        "  Status:  {}",
        if status.healthy { "Healthy" } else { "Offline" }
    );

    if let Some(error) = &status.error {
        if verbose || !status.healthy {
            println!("  Error:   {}", error);
        }
    }
    println!();
}

/// Print status for a single service (inline format for grouped display)
fn print_service_status_inline(
    prefix: &str,
    status: &ServiceStatus,
    verbose: bool,
    global_config: &GlobalConfig,
) {
    let indicator = if status.healthy {
        if global_config.use_colors() {
            console::style("✓").green().to_string()
        } else {
            "[OK]".to_string()
        }
    } else if global_config.use_colors() {
        console::style("✗").red().to_string()
    } else {
        "[OFFLINE]".to_string()
    };

    let status_text = if status.healthy { "Healthy" } else { "Offline" };

    println!(
        "{} {} {} ({})",
        prefix, indicator, status_text, status.address
    );

    if let Some(error) = &status.error {
        if verbose || !status.healthy {
            println!("{}   Error: {}", prefix, error);
        }
    }
}

/// Restarts the local engine with the image, ports and settings it has.
async fn restart_server(force: bool, global_config: &GlobalConfig) -> Result<()> {
    let opts = local_engine::running_options()?.ok_or_else(|| {
        CliError::local_engine("No local engine is running. Start one with `orcher server start`.")
    })?;
    let graceful = (!force).then(|| Duration::from_secs(30));
    local_engine::stop(false, graceful)?;
    crate::commands::dev::start(&opts, global_config).await
}

/// Prints the CLI's version, the local engine's, and whether the engine and
/// gateway answer.
async fn server_version(
    orchestrator_http_addr: String,
    gateway_http_addr: Option<String>,
    global_config: &GlobalConfig,
) -> Result<()> {
    let engine_version = local_engine::status()
        .ok()
        .and_then(|(engine, _)| engine)
        .and_then(|e| e.version);
    let orchestrator_status = check_service_health(
        "Orchestrator",
        &orchestrator_http_addr,
        ORCHESTRATOR_HEALTH_PATH,
    )
    .await;
    let gateway_status = match &gateway_http_addr {
        Some(addr) => Some(check_service_health("Gateway", addr, GATEWAY_HEALTH_PATH).await),
        None => None,
    };

    if global_config.is_structured_output() {
        let value = serde_json::json!({
            "cli": env!("CARGO_PKG_VERSION"),
            "engine": engine_version,
            "orchestrator": { "address": orchestrator_http_addr, "running": orchestrator_status.healthy },
            "gateway": gateway_status.as_ref().map(|g| serde_json::json!({ "address": g.address, "running": g.healthy })),
        });
        match global_config.output_format.as_str() {
            "yaml" => print!("{}", serde_yaml::to_string(&value)?),
            _ => println!("{}", serde_json::to_string_pretty(&value)?),
        }
        return Ok(());
    }

    println!("ORCHER Version Information");
    println!("==========================");
    println!();
    println!("CLI Version:    {}", env!("CARGO_PKG_VERSION"));
    println!(
        "Engine Version: {}",
        engine_version
            .as_deref()
            .unwrap_or("unknown (no local engine)")
    );
    println!();
    println!("Services:");
    println!(
        "  Orchestrator ({}): {}",
        orchestrator_http_addr,
        if orchestrator_status.healthy {
            "Running"
        } else {
            "Offline"
        }
    );
    match &gateway_status {
        Some(g) => println!(
            "  Gateway ({}): {}",
            g.address,
            if g.healthy { "Running" } else { "Offline" }
        ),
        None => println!("  Gateway: not configured"),
    }
    Ok(())
}

/// Mask password in URL for display
fn mask_url_password(url: &str) -> String {
    if let Ok(mut parsed) = url::Url::parse(url) {
        if parsed.password().is_some() {
            let _ = parsed.set_password(Some("****"));
        }
        parsed.to_string()
    } else {
        url.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mask_url_password() {
        let url = "postgresql://user:secret@localhost:5432/db";
        let masked = mask_url_password(url);
        assert!(masked.contains("****"));
        assert!(!masked.contains("secret"));

        let url_no_pass = "postgresql://user@localhost:5432/db";
        let masked = mask_url_password(url_no_pass);
        assert!(!masked.contains("****"));
    }

    #[tokio::test]
    async fn test_check_service_health_offline() {
        let status = check_service_health("Test", "http://localhost:59999", "/health").await;
        assert!(!status.healthy);
        assert!(status.error.is_some());
    }

    #[tokio::test]
    async fn test_check_grpc_health_offline() {
        let status = check_grpc_health("http://localhost:59999").await;
        assert!(!status.healthy);
        assert!(status.error.is_some());
    }

    #[tokio::test]
    async fn test_server_status_command() {
        let global_config = GlobalConfig::default();
        // This will fail to connect but should not panic
        let result = server_status(
            false,
            "table".to_string(),
            "http://localhost:59999".to_string(),
            "http://localhost:59998".to_string(),
            Some("http://localhost:59997".to_string()),
            &global_config,
        )
        .await;
        assert!(result.is_ok());
    }
}
