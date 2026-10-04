//! Implementation of the 'config' command for managing CLI configuration

use crate::client::connection::ConnectionManager;
use crate::config::{AuthConfig, AuthType, Config, Context};
use crate::error::{CliError, Result};
use crate::types::ConfigCommands;
use crate::utils::GlobalConfig;
use console::style;
use url::Url;

pub async fn execute(action: ConfigCommands, global_config: &GlobalConfig) -> Result<()> {
    match action {
        ConfigCommands::View { raw } => view_config(raw, global_config).await,
        ConfigCommands::GetContexts => get_contexts(global_config).await,
        ConfigCommands::UseContext { context } => use_context(&context, global_config).await,
        ConfigCommands::Set { key, value } => {
            set_config_property(&key, &value, global_config).await
        }
        ConfigCommands::Unset { key } => unset_config_property(&key, global_config).await,
        ConfigCommands::DeleteContext { context } => delete_context(&context, global_config).await,
        ConfigCommands::SetContext {
            context,
            server,
            namespace,
            timeout,
        } => {
            set_context(
                &context,
                server.as_deref(),
                namespace.as_deref(),
                timeout.as_deref(),
                global_config,
            )
            .await
        }
        ConfigCommands::TestConnection { context, server } => {
            test_connection(context.as_deref(), server.as_deref(), global_config).await
        }
    }
}

/// View the current configuration
async fn view_config(raw: bool, _global_config: &GlobalConfig) -> Result<()> {
    let config = load_or_create_config()?;

    if raw {
        // Display raw YAML configuration
        let yaml = serde_yaml::to_string(&config).map_err(|e| CliError::Config {
            message: format!("Failed to serialize config: {}", e),
            source: Some(Box::new(e)),
        })?;
        println!("{}", yaml);
    } else {
        // Display formatted configuration
        println!("Current configuration:");
        println!("  Current Context: {}", config.current_context);
        println!("  Current Profile: {}", config.current_profile);
        println!();

        if config.contexts.is_empty() {
            println!("No contexts configured");
        } else {
            println!("Contexts:");
            for (name, context) in &config.contexts {
                let marker = if name == &config.current_context {
                    "*"
                } else {
                    " "
                };
                println!("{}  {}", marker, name);
                println!("     Server: {}", context.server);
                if let Some(ref namespace) = context.namespace {
                    println!("     Namespace: {}", namespace);
                }
                println!("     Auth Type: {:?}", context.auth.auth_type);
                if let Some(ref timeout) = context.timeout {
                    println!("     Timeout: {}", timeout);
                }
                println!();
            }
        }

        if !config.profiles.is_empty() {
            println!("Profiles:");
            for (name, profile) in &config.profiles {
                let marker = if name == &config.current_profile {
                    "*"
                } else {
                    " "
                };
                println!("{}  {}", marker, name);
                println!("     Output: {:?}", profile.output);
                if let Some(ref namespace) = profile.namespace {
                    println!("     Default Namespace: {}", namespace);
                }
                if let Some(ref log_level) = profile.log_level {
                    println!("     Log Level: {}", log_level);
                }
                println!();
            }
        }
    }

    Ok(())
}

/// Get and display all contexts
async fn get_contexts(_global_config: &GlobalConfig) -> Result<()> {
    let config = load_or_create_config()?;

    if config.contexts.is_empty() {
        println!("No contexts configured");
        return Ok(());
    }

    println!("Available contexts:");
    for (name, context) in &config.contexts {
        let marker = if name == &config.current_context {
            "*"
        } else {
            " "
        };
        println!("{}  {} ({})", marker, name, context.server);
    }

    Ok(())
}

/// Switch to a different context
async fn use_context(context_name: &str, _global_config: &GlobalConfig) -> Result<()> {
    let mut config = load_or_create_config()?;

    if !config.contexts.contains_key(context_name) {
        return Err(CliError::ContextNotFound {
            name: context_name.to_string(),
        });
    }

    config.current_context = context_name.to_string();
    config.save()?;

    println!("Switched to context '{}'", context_name);
    Ok(())
}

/// Set a configuration property
async fn set_config_property(key: &str, value: &str, _global_config: &GlobalConfig) -> Result<()> {
    let mut config = load_or_create_config()?;

    // Handle dot-notation for nested properties
    if key.starts_with("contexts.") {
        let parts: Vec<&str> = key.splitn(3, '.').collect();
        if parts.len() < 3 {
            return Err(CliError::Config {
                message: format!(
                    "Invalid context property: {}. Expected format: contexts.<name>.<property>",
                    key
                ),
                source: None,
            });
        }

        let context_name = parts[1];
        let property = parts[2];

        // Get or create context
        let mut context = config
            .contexts
            .get(context_name)
            .cloned()
            .unwrap_or_else(|| Context {
                server: "http://localhost:50051".to_string(),
                api: None,
                auth: AuthConfig {
                    auth_type: AuthType::None,
                    token_ref: None,
                    apikey_ref: None,
                    username: None,
                },
                tls: None,
                timeout: None,
                namespace: None,
            });

        // Set property
        match property {
            "server" => {
                Url::parse(value).map_err(|e| CliError::Config {
                    message: format!("Invalid server URL '{}': {}", value, e),
                    source: Some(Box::new(e)),
                })?;
                context.server = value.to_string();
            }
            "namespace" => {
                context.namespace = Some(value.to_string());
            }
            "timeout" => {
                value.parse::<u64>().map_err(|e| CliError::Config {
                    message: format!("Invalid timeout value '{}': {}", value, e),
                    source: Some(Box::new(e)),
                })?;
                context.timeout = Some(value.to_string());
            }
            _ => {
                return Err(CliError::Config {
                    message: format!(
                        "Unknown context property: {}. Valid properties: server, namespace, timeout",
                        property
                    ),
                    source: None,
                });
            }
        }

        config.contexts.insert(context_name.to_string(), context);
        config.save()?;
        println!(
            "Set contexts.{} = '{}' for context '{}'",
            property, value, context_name
        );
        return Ok(());
    }

    // Handle top-level properties
    match key {
        "current-context" => {
            if !config.contexts.contains_key(value) {
                return Err(CliError::ContextNotFound {
                    name: value.to_string(),
                });
            }
            config.current_context = value.to_string();
        }
        "current-profile" => {
            if !config.profiles.contains_key(value) {
                return Err(CliError::Config {
                    message: format!("Profile '{}' does not exist", value),
                    source: None,
                });
            }
            config.current_profile = value.to_string();
        }
        _ => {
            return Err(CliError::Config {
                message: format!("Unknown configuration property: {}", key),
                source: None,
            });
        }
    }

    config.save()?;
    println!("Configuration property '{}' set to '{}'", key, value);
    Ok(())
}

/// Unset a configuration property
async fn unset_config_property(key: &str, _global_config: &GlobalConfig) -> Result<()> {
    let mut config = load_or_create_config()?;

    match key {
        "current-context" => {
            config.current_context = crate::constants::DEFAULT_CONTEXT.to_string();
        }
        "current-profile" => {
            config.current_profile = crate::constants::DEFAULT_PROFILE.to_string();
        }
        _ => {
            return Err(CliError::Config {
                message: format!("Unknown configuration property: {}", key),
                source: None,
            });
        }
    }

    config.save()?;
    println!("Configuration property '{}' unset", key);
    Ok(())
}

/// Delete a context
async fn delete_context(context_name: &str, _global_config: &GlobalConfig) -> Result<()> {
    let mut config = load_or_create_config()?;

    if !config.contexts.contains_key(context_name) {
        return Err(CliError::ContextNotFound {
            name: context_name.to_string(),
        });
    }

    if context_name == config.current_context {
        return Err(CliError::Config {
            message: "Cannot delete the current context. Switch to another context first."
                .to_string(),
            source: None,
        });
    }

    config.contexts.remove(context_name);
    config.save()?;

    println!("Context '{}' deleted", context_name);
    Ok(())
}

/// Set or update a context
async fn set_context(
    context_name: &str,
    server: Option<&str>,
    namespace: Option<&str>,
    timeout: Option<&str>,
    _global_config: &GlobalConfig,
) -> Result<()> {
    let mut config = load_or_create_config()?;

    // Get existing context or create a new one
    let mut context = config
        .contexts
        .get(context_name)
        .cloned()
        .unwrap_or_else(|| Context {
            server: "http://localhost:50051".to_string(),
            api: None,
            auth: AuthConfig {
                auth_type: AuthType::None,
                token_ref: None,
                apikey_ref: None,
                username: None,
            },
            tls: None,
            timeout: None,
            namespace: None,
        });

    // Update context properties
    if let Some(server_url) = server {
        // Validate URL
        Url::parse(server_url).map_err(|e| CliError::Config {
            message: format!("Invalid server URL '{}': {}", server_url, e),
            source: Some(Box::new(e)),
        })?;
        context.server = server_url.to_string();
    }

    if let Some(ns) = namespace {
        context.namespace = Some(ns.to_string());
    }

    if let Some(timeout_str) = timeout {
        // Validate timeout format
        timeout_str.parse::<u64>().map_err(|e| CliError::Config {
            message: format!("Invalid timeout value '{}': {}", timeout_str, e),
            source: Some(Box::new(e)),
        })?;
        context.timeout = Some(timeout_str.to_string());
    }

    config.contexts.insert(context_name.to_string(), context);
    config.save()?;

    println!("Context '{}' configured", context_name);
    Ok(())
}

/// Load existing configuration or create a default one
fn load_or_create_config() -> Result<Config> {
    match Config::load() {
        Ok(config) => Ok(config),
        Err(_) => {
            // Create default configuration
            let config = Config::default();
            config.save()?;
            Ok(config)
        }
    }
}

/// Test connection to ORCHER server
async fn test_connection(
    context: Option<&str>,
    server: Option<&str>,
    global_config: &GlobalConfig,
) -> Result<()> {
    // Build a modified config for connection testing
    let mut test_config = global_config.clone();

    // Override server if provided
    if let Some(server_url) = server {
        test_config.server = Some(server_url.to_string());
    }

    // Override context if provided
    if let Some(ctx) = context {
        test_config.context = Some(ctx.to_string());
    }

    // Create connection manager with the test config
    let manager = ConnectionManager::from_config(&test_config);

    println!("Testing connection to ORCHER server...\n");

    // Show what we're testing
    println!("Configuration:");
    if let Some(ctx) = &test_config.context {
        println!("  Context: {}", ctx);
    }
    println!("  gRPC endpoint: {}", manager.grpc_addr());
    println!("  HTTP endpoint: {}", manager.http_addr());
    println!();

    // Test both endpoints
    println!("Checking endpoints...\n");

    let (grpc_status, http_status) = manager.detect_servers().await;

    // Display gRPC status
    print!("  gRPC [{}] ", manager.grpc_addr());
    if grpc_status.available {
        println!(
            "{} ({}ms)",
            style("OK").green().bold(),
            grpc_status.latency_ms.unwrap_or(0)
        );
    } else {
        println!(
            "{} - {}",
            style("FAILED").red().bold(),
            grpc_status
                .error
                .unwrap_or_else(|| "Unknown error".to_string())
        );
    }

    // Display HTTP status
    print!("  HTTP [{}] ", manager.http_addr());
    if http_status.available {
        println!(
            "{} ({}ms)",
            style("OK").green().bold(),
            http_status.latency_ms.unwrap_or(0)
        );
    } else {
        println!(
            "{} - {}",
            style("FAILED").red().bold(),
            http_status
                .error
                .unwrap_or_else(|| "Unknown error".to_string())
        );
    }

    println!();

    // Summary
    if grpc_status.available || http_status.available {
        let preferred = if grpc_status.available {
            "gRPC"
        } else {
            "HTTP"
        };
        println!(
            "{}  Connection successful! Using {} protocol.",
            style("✓").green(),
            preferred
        );
        Ok(())
    } else {
        println!(
            "{}  Connection failed. Server may be offline or unreachable.",
            style("✗").red()
        );
        println!();
        println!("Troubleshooting:");
        println!("  1. Ensure the ORCHER server is running");
        println!("  2. Check the server URL in your config");
        println!("  3. Verify network connectivity and firewall rules");
        println!("  4. Try: orcher server start (for local development)");

        Err(CliError::Network {
            message: "Unable to connect to ORCHER server".to_string(),
            source: None,
        })
    }
}
