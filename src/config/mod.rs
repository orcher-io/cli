//! CLI configuration (~/.orcher/config.yaml): contexts, credentials, preferences.

#![allow(dead_code)]

use crate::constants::{CONFIG_DIR, CONFIG_FILE};
use crate::error::{CliError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Main CLI configuration structure
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Config {
    #[serde(rename = "current-context")]
    pub current_context: String,
    #[serde(rename = "current-profile")]
    pub current_profile: String,
    pub contexts: HashMap<String, Context>,
    pub profiles: HashMap<String, Profile>,
}

/// Context configuration for different ORCHER environments
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Context {
    /// The engine's gRPC address.
    pub server: String,
    /// The HTTP gateway's address, for a cloud deployment: login, batch
    /// operations and log streaming. Unset for an engine without one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api: Option<String>,
    pub auth: AuthConfig,
    pub tls: Option<TlsConfig>,
    pub timeout: Option<String>,
    pub namespace: Option<String>,
}

/// Authentication configuration for contexts
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AuthConfig {
    #[serde(rename = "type")]
    pub auth_type: AuthType,
    #[serde(rename = "token-ref")]
    pub token_ref: Option<String>,
    #[serde(rename = "apikey-ref")]
    pub apikey_ref: Option<String>,
    pub username: Option<String>,
}

/// Authentication types supported by the CLI
#[derive(Serialize, Deserialize, Debug, Clone)]
#[serde(rename_all = "lowercase")]
pub enum AuthType {
    Token,
    #[serde(rename = "apikey")]
    ApiKey,
    Basic,
    None,
}

/// TLS configuration for secure connections
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TlsConfig {
    pub insecure: Option<bool>,
    #[serde(rename = "ca-cert")]
    pub ca_cert: Option<String>,
    #[serde(rename = "client-cert")]
    pub client_cert: Option<String>,
    #[serde(rename = "client-key")]
    pub client_key: Option<String>,
}

/// Profile configuration for user preferences
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Profile {
    pub output: OutputFormat,
    pub namespace: Option<String>,
    #[serde(rename = "log-level")]
    pub log_level: Option<String>,
    #[serde(rename = "show-managed-fields")]
    pub show_managed_fields: bool,
    pub quiet: Option<bool>,
}

/// Output format options
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    #[default]
    Table,
    Json,
    Yaml,
    Name,
    Wide,
}

impl Config {
    /// Load configuration from the default location
    pub fn load() -> Result<Self> {
        let config_path = Self::config_path()?;

        if !config_path.exists() {
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(&config_path).map_err(|e| {
            CliError::io_with_path(
                "Failed to read config file",
                config_path.display().to_string(),
                e,
            )
        })?;

        let config: Config = serde_yaml::from_str(&content)
            .map_err(|e| CliError::config_with_source("Failed to parse config file", e))?;

        config.validate()?;
        Ok(config)
    }

    /// Save configuration to the default location
    pub fn save(&self) -> Result<()> {
        let config_path = Self::config_path()?;

        // Ensure directory exists
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                CliError::io_with_path(
                    "Failed to create config directory",
                    parent.display().to_string(),
                    e,
                )
            })?;
        }

        let content = serde_yaml::to_string(self)
            .map_err(|e| CliError::config_with_source("Failed to serialize config", e))?;

        std::fs::write(&config_path, content).map_err(|e| {
            CliError::io_with_path(
                "Failed to write config file",
                config_path.display().to_string(),
                e,
            )
        })?;

        // Set secure permissions (readable only by owner)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&config_path)
                .map_err(|e| {
                    CliError::io_with_path(
                        "Failed to get config file metadata",
                        config_path.display().to_string(),
                        e,
                    )
                })?
                .permissions();
            perms.set_mode(0o600);
            std::fs::set_permissions(&config_path, perms).map_err(|e| {
                CliError::io_with_path(
                    "Failed to set config file permissions",
                    config_path.display().to_string(),
                    e,
                )
            })?;
        }

        Ok(())
    }

    /// Get the configuration file path
    /// `--config` / `ORCHER_CONFIG` when given, else `~/.orcher/config.yaml`.
    pub fn config_path() -> Result<PathBuf> {
        if let Some(path) =
            std::env::var_os(crate::constants::CONFIG_FILE_ENV).filter(|p| !p.is_empty())
        {
            return Ok(PathBuf::from(path));
        }
        let home = dirs::home_dir()
            .ok_or_else(|| CliError::config("Unable to determine home directory"))?;
        Ok(home.join(CONFIG_DIR).join(CONFIG_FILE))
    }

    /// Get the current context
    pub fn get_current_context(&self) -> Result<&Context> {
        self.contexts
            .get(&self.current_context)
            .ok_or_else(|| CliError::ContextNotFound {
                name: self.current_context.clone(),
            })
    }

    /// Get the current profile
    pub fn get_current_profile(&self) -> Result<&Profile> {
        self.profiles.get(&self.current_profile).ok_or_else(|| {
            CliError::config(format!("Profile '{}' not found", self.current_profile))
        })
    }

    /// Set a context
    pub fn set_context(&mut self, name: String, context: Context) {
        self.contexts.insert(name, context);
    }

    /// Delete a context
    pub fn delete_context(&mut self, name: &str) -> Result<()> {
        if name == self.current_context {
            return Err(CliError::config("Cannot delete current context"));
        }

        self.contexts
            .remove(name)
            .ok_or_else(|| CliError::ContextNotFound {
                name: name.to_string(),
            })?;
        Ok(())
    }

    /// Switch to a different context
    pub fn use_context(&mut self, context_name: &str) -> Result<()> {
        if !self.contexts.contains_key(context_name) {
            return Err(CliError::ContextNotFound {
                name: context_name.to_string(),
            });
        }
        self.current_context = context_name.to_string();
        Ok(())
    }

    /// Validate the configuration
    fn validate(&self) -> Result<()> {
        if !self.contexts.contains_key(&self.current_context) {
            return Err(CliError::config(format!(
                "Current context '{}' not found",
                self.current_context
            )));
        }

        if !self.profiles.contains_key(&self.current_profile) {
            return Err(CliError::config(format!(
                "Current profile '{}' not found",
                self.current_profile
            )));
        }

        Ok(())
    }
}

impl Default for Config {
    fn default() -> Self {
        let mut contexts = HashMap::new();
        contexts.insert(
            "local".to_string(),
            Context {
                server: "http://localhost:50051".to_string(),
                api: None,
                auth: AuthConfig {
                    auth_type: AuthType::None,
                    token_ref: None,
                    apikey_ref: None,
                    username: None,
                },
                tls: None,
                timeout: Some("30s".to_string()),
                namespace: Some("default".to_string()),
            },
        );

        let mut profiles = HashMap::new();
        profiles.insert(
            "default".to_string(),
            Profile {
                output: OutputFormat::Table,
                namespace: Some("default".to_string()),
                log_level: Some("info".to_string()),
                show_managed_fields: false,
                quiet: None,
            },
        );

        Self {
            current_context: "local".to_string(),
            current_profile: "default".to_string(),
            contexts,
            profiles,
        }
    }
}

/// Load or create configuration from the default location (backward compatibility)
pub fn load_or_create_config() -> Result<Config> {
    Config::load()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// --config / ORCHER_CONFIG used to be accepted and then ignored.
    #[test]
    fn the_config_file_can_be_chosen() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("elsewhere.yaml");
        std::env::set_var(crate::constants::CONFIG_FILE_ENV, &file);
        assert_eq!(Config::config_path().unwrap(), file);
        std::env::remove_var(crate::constants::CONFIG_FILE_ENV);
        assert!(Config::config_path()
            .unwrap()
            .ends_with(".orcher/config.yaml"));
    }

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert_eq!(config.current_context, "local");
        assert_eq!(config.current_profile, "default");
        assert!(config.contexts.contains_key("local"));
        assert!(config.profiles.contains_key("default"));
    }

    #[test]
    fn test_config_validation() {
        let config = Config::default();
        assert!(config.validate().is_ok());

        let invalid_config = Config {
            current_context: "nonexistent".to_string(),
            ..Default::default()
        };
        assert!(invalid_config.validate().is_err());
    }

    #[test]
    fn test_context_operations() {
        let mut config = Config::default();

        // Add a new context
        let new_context = Context {
            server: "https://orcher.example.com".to_string(),
            api: None,
            auth: AuthConfig {
                auth_type: AuthType::Token,
                token_ref: Some("my-token".to_string()),
                apikey_ref: None,
                username: None,
            },
            tls: None,
            timeout: Some("60s".to_string()),
            namespace: Some("production".to_string()),
        };

        config.set_context("production".to_string(), new_context);
        assert!(config.contexts.contains_key("production"));

        // Switch to new context
        assert!(config.use_context("production").is_ok());
        assert_eq!(config.current_context, "production");

        // Try to delete current context (should fail)
        assert!(config.delete_context("production").is_err());

        // Switch back and delete
        config.use_context("local").unwrap();
        assert!(config.delete_context("production").is_ok());
        assert!(!config.contexts.contains_key("production"));
    }

    #[test]
    fn test_serialization() {
        let config = Config::default();
        let yaml = serde_yaml::to_string(&config).unwrap();
        let deserialized: Config = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(config.current_context, deserialized.current_context);
    }
}
