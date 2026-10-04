//! CLI constants (version, paths, defaults).

#![allow(dead_code)]

/// Version of the CLI
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// User agent string for HTTP requests
pub const USER_AGENT: &str = concat!(env!("CARGO_PKG_NAME"), "/", env!("CARGO_PKG_VERSION"));

/// Default configuration directory name
pub const CONFIG_DIR: &str = ".orcher";

/// Default configuration file name
pub const CONFIG_FILE: &str = "config.yaml";

/// Default credentials file name
pub const CREDENTIALS_FILE: &str = "credentials";

/// Default templates directory name
pub const TEMPLATES_DIR: &str = "templates";

/// Default cache directory name
pub const CACHE_DIR: &str = "cache";

/// Default logs directory name
pub const LOGS_DIR: &str = "logs";

/// Maximum configuration file size (in bytes)
pub const MAX_CONFIG_FILE_SIZE: usize = 1024 * 1024; // 1MB

/// Maximum number of contexts to store
pub const MAX_CONTEXTS: usize = 100;

/// Default API timeout in seconds
pub const DEFAULT_API_TIMEOUT: u64 = 30;

/// Default connection timeout in seconds
pub const DEFAULT_CONNECTION_TIMEOUT: u64 = 10;

/// Default retry attempts for API calls
pub const DEFAULT_RETRY_ATTEMPTS: usize = 3;

/// Default retry delay in milliseconds
pub const DEFAULT_RETRY_DELAY: u64 = 1000;

/// Maximum log file size (in bytes)
pub const MAX_LOG_FILE_SIZE: usize = 10 * 1024 * 1024; // 10MB

/// Maximum number of log files to keep
pub const MAX_LOG_FILES: usize = 5;

/// Default output format
pub const DEFAULT_OUTPUT_FORMAT: &str = "table";

/// Default namespace
pub const DEFAULT_NAMESPACE: &str = "default";

/// Default profile name
pub const DEFAULT_PROFILE: &str = "default";

/// Default context name
pub const DEFAULT_CONTEXT: &str = "local";

/// Default local server URL (orchestrator gRPC endpoint)
pub const DEFAULT_LOCAL_SERVER: &str = "http://localhost:50051";

/// ORCHER Cloud: the one address its gateway (REST: login, batch operations,
/// log streaming) and its engine (gRPC) are both served from. Override it with
/// `ORCHER_CLOUD_URL`, for a staging or private deployment.
pub const DEFAULT_CLOUD_URL: &str = "https://api.orcher.io";

/// Environment variable overriding [`DEFAULT_CLOUD_URL`].
pub const CLOUD_URL_ENV: &str = "ORCHER_CLOUD_URL";

/// Environment variable naming the HTTP gateway explicitly.
pub const API_URL_ENV: &str = "ORCHER_API_URL";

/// The ORCHER Cloud address in use: `ORCHER_CLOUD_URL`, or the default.
pub fn cloud_url() -> String {
    std::env::var(CLOUD_URL_ENV)
        .ok()
        .map(|v| v.trim().trim_end_matches('/').to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| DEFAULT_CLOUD_URL.to_string())
}

/// Application name for system integration
pub const APP_NAME: &str = "orcher-cli";

/// Application display name
pub const APP_DISPLAY_NAME: &str = "ORCHER CLI";

/// Application description
pub const APP_DESCRIPTION: &str =
    "Command-line interface for ORCHER workflow orchestration platform";

/// Minimum supported API version
pub const MIN_API_VERSION: &str = "v1";

/// Current API version
pub const CURRENT_API_VERSION: &str = "v1";

/// Supported workflow API versions
pub const SUPPORTED_WORKFLOW_VERSIONS: &[&str] = &["orcher.io/v1"];

/// Built-in template names
pub const BUILTIN_TEMPLATES: &[&str] = &[
    "basic-workflow",
    "microservices",
    "data-pipeline",
    "batch-processing",
];

/// Supported runtime types for tasks
pub const SUPPORTED_RUNTIMES: &[&str] = &["docker", "shell", "http", "python", "nodejs"];

/// Supported workflow types
pub const SUPPORTED_WORKFLOW_TYPES: &[&str] = &["sequential", "parallel", "dag", "conditional"];

/// Supported output formats
pub const SUPPORTED_OUTPUT_FORMATS: &[&str] = &["table", "json", "yaml", "name", "wide"];

/// Environment variable for configuration file override
pub const CONFIG_FILE_ENV: &str = "ORCHER_CONFIG";

/// Environment variable for context override
pub const CONTEXT_ENV: &str = "ORCHER_CONTEXT";

/// Environment variable for namespace override
pub const NAMESPACE_ENV: &str = "ORCHER_NAMESPACE";

/// Environment variable for server URL override
pub const SERVER_URL_ENV: &str = "ORCHER_SERVER";

/// Environment variable for API token
pub const API_TOKEN_ENV: &str = "ORCHER_TOKEN";

/// Environment variable for disabling colors
pub const NO_COLOR_ENV: &str = "NO_COLOR";

/// Environment variable for verbose output
pub const VERBOSE_ENV: &str = "ORCHER_VERBOSE";

/// Environment variable for quiet mode
pub const QUIET_ENV: &str = "ORCHER_QUIET";
