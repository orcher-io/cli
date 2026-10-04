//! Connection auto-detection: tries gRPC, falls back to HTTP gateway.

use crate::client::grpc_client::{non_empty_env, OrcherGrpcClient};
use crate::config::Config;
use crate::error::{CliError, Result};
use crate::utils::GlobalConfig;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::OnceCell;
use tracing::{debug, info, warn};

/// Default addresses for ORCHER services
pub const DEFAULT_GRPC_ADDR: &str = "http://localhost:50051";
pub const DEFAULT_HTTP_ADDR: &str = "http://localhost:8000";

/// Health endpoint paths. These differ by service: the gateway nests its REST
/// routes under `/api/v1`, while the orchestrator serves a root `/health`.
/// Single source of truth so the CLI's health checks can't diverge.
pub const GATEWAY_HEALTH_PATH: &str = "/api/v1/health/live";
pub const ORCHESTRATOR_HEALTH_PATH: &str = "/health";

/// Connection timeout for health checks
const HEALTH_CHECK_TIMEOUT: Duration = Duration::from_secs(2);

/// Detected server type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerType {
    /// gRPC orchestrator (preferred)
    Grpc,
    /// HTTP gateway (fallback)
    Http,
}

impl std::fmt::Display for ServerType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ServerType::Grpc => write!(f, "gRPC"),
            ServerType::Http => write!(f, "HTTP"),
        }
    }
}

/// Server availability status
#[derive(Debug, Clone)]
pub struct ServerStatus {
    pub server_type: ServerType,
    pub address: String,
    pub available: bool,
    pub latency_ms: Option<u64>,
    pub error: Option<String>,
}

/// Connection manager with auto-detection and caching
pub struct ConnectionManager {
    /// Cached gRPC client
    grpc_client: OnceCell<Arc<OrcherGrpcClient>>,
    /// gRPC address
    grpc_addr: String,
    /// HTTP gateway address
    http_addr: String,
    /// Whether the gateway address was configured rather than derived
    http_configured: bool,
    /// Default namespace
    namespace: String,
    /// Context name for authentication
    context: Option<String>,
}

impl ConnectionManager {
    /// Create a new connection manager from global config
    ///
    /// Server URL resolution priority:
    /// 1. `--server` flag (config.server)
    /// 2. `ORCHER_SERVER` environment variable
    /// 3. Context config from ~/.orcher/config.yaml
    /// 4. Default: http://localhost:50051
    pub fn from_config(config: &GlobalConfig) -> Self {
        // Priority 1: --server flag
        let grpc_addr = if let Some(server) = &config.server {
            debug!("Using server from --server flag: {}", server);
            normalize_server_url(server)
        }
        // Priority 2: ORCHER_SERVER env var
        else if let Ok(server) = std::env::var("ORCHER_SERVER") {
            debug!("Using server from ORCHER_SERVER env var: {}", server);
            normalize_server_url(&server)
        }
        // Priority 3: Context config
        else if let Some(server) = Self::load_server_from_context(config) {
            debug!("Using server from context config: {}", server);
            server
        }
        // Priority 4: Default
        else {
            debug!("Using default server: {}", DEFAULT_GRPC_ADDR);
            DEFAULT_GRPC_ADDR.to_string()
        };

        // The gateway: --api-url / ORCHER_API_URL, then the context's `api`,
        // else derived from the gRPC address.
        let configured_http = config
            .api_url
            .clone()
            .or_else(|| non_empty_env(crate::constants::API_URL_ENV))
            .or_else(|| Self::load_api_from_context(config))
            .map(|url| normalize_server_url(&url).trim_end_matches('/').to_string());
        let http_configured = configured_http.is_some();
        let http_addr = configured_http.unwrap_or_else(|| derive_http_addr(&grpc_addr));

        // Get namespace with similar priority
        let namespace = config
            .namespace
            .clone()
            .or_else(|| std::env::var("ORCHER_NAMESPACE").ok())
            .or_else(|| Self::load_namespace_from_context(config))
            .unwrap_or_else(|| "default".to_string());

        // Get context name for authentication
        let context = config
            .context
            .clone()
            .or_else(|| Config::load().ok().map(|c| c.current_context));

        Self {
            grpc_client: OnceCell::new(),
            grpc_addr,
            http_addr,
            http_configured,
            namespace,
            context,
        }
    }

    /// Load the gateway URL from the context config file
    fn load_api_from_context(config: &GlobalConfig) -> Option<String> {
        let cli_config = Config::load().ok()?;
        let context_name = config
            .context
            .as_deref()
            .unwrap_or(&cli_config.current_context);
        cli_config.contexts.get(context_name)?.api.clone()
    }

    /// The gateway address, only when one was configured: a flag, the
    /// environment or the context. A derived guess is not reported.
    pub fn configured_http_addr(&self) -> Option<String> {
        self.http_configured.then(|| self.http_addr.clone())
    }

    /// Load server URL from context config file
    fn load_server_from_context(config: &GlobalConfig) -> Option<String> {
        let cli_config = match Config::load() {
            Ok(c) => c,
            Err(e) => {
                debug!("Could not load config file: {}", e);
                return None;
            }
        };

        // Use context from --context flag or current context
        let context_name = config
            .context
            .as_deref()
            .unwrap_or(&cli_config.current_context);

        match cli_config.contexts.get(context_name) {
            Some(ctx) => {
                info!(
                    "Using context '{}' with server: {}",
                    context_name, ctx.server
                );
                Some(normalize_server_url(&ctx.server))
            }
            None => {
                warn!("Context '{}' not found in config", context_name);
                None
            }
        }
    }

    /// Load namespace from context config file
    fn load_namespace_from_context(config: &GlobalConfig) -> Option<String> {
        let cli_config = Config::load().ok()?;
        let context_name = config
            .context
            .as_deref()
            .unwrap_or(&cli_config.current_context);
        cli_config.contexts.get(context_name)?.namespace.clone()
    }

    /// Create a new connection manager with explicit addresses
    pub fn new(grpc_addr: &str, http_addr: &str, namespace: &str) -> Self {
        Self {
            grpc_client: OnceCell::new(),
            grpc_addr: grpc_addr.to_string(),
            http_addr: http_addr.to_string(),
            http_configured: true,
            namespace: namespace.to_string(),
            context: None,
        }
    }

    /// Create a new connection manager with explicit addresses and context
    pub fn new_with_context(
        grpc_addr: &str,
        http_addr: &str,
        namespace: &str,
        context: &str,
    ) -> Self {
        Self {
            grpc_client: OnceCell::new(),
            grpc_addr: grpc_addr.to_string(),
            http_addr: http_addr.to_string(),
            http_configured: true,
            namespace: namespace.to_string(),
            context: Some(context.to_string()),
        }
    }

    /// Check if gRPC server is available
    pub async fn check_grpc(&self) -> ServerStatus {
        let start = std::time::Instant::now();

        match probe_grpc(&self.grpc_addr).await {
            Ok(()) => ServerStatus {
                server_type: ServerType::Grpc,
                address: self.grpc_addr.clone(),
                available: true,
                latency_ms: Some(start.elapsed().as_millis() as u64),
                error: None,
            },
            Err(e) => ServerStatus {
                server_type: ServerType::Grpc,
                address: self.grpc_addr.clone(),
                available: false,
                latency_ms: Some(start.elapsed().as_millis() as u64),
                error: Some(e),
            },
        }
    }

    /// Check if HTTP gateway is available
    pub async fn check_http(&self) -> ServerStatus {
        let start = std::time::Instant::now();

        match probe_http(&self.http_addr, GATEWAY_HEALTH_PATH).await {
            Ok(()) => ServerStatus {
                server_type: ServerType::Http,
                address: self.http_addr.clone(),
                available: true,
                latency_ms: Some(start.elapsed().as_millis() as u64),
                error: None,
            },
            Err(e) => ServerStatus {
                server_type: ServerType::Http,
                address: self.http_addr.clone(),
                available: false,
                latency_ms: Some(start.elapsed().as_millis() as u64),
                error: Some(e),
            },
        }
    }

    /// Detect which servers are available
    pub async fn detect_servers(&self) -> (ServerStatus, ServerStatus) {
        // Check both in parallel
        tokio::join!(self.check_grpc(), self.check_http())
    }

    /// Get the best available server type
    ///
    /// Priority:
    /// 1. Prefer gRPC (if available)
    /// 2. Fall back to the HTTP gateway
    /// 3. Return error if neither is available
    pub async fn detect_best_server(&self) -> Result<ServerType> {
        // Check both servers in parallel
        let (grpc_status, http_status) = self.detect_servers().await;

        if grpc_status.available {
            debug!(
                "Using gRPC orchestrator at {} (latency: {}ms)",
                grpc_status.address,
                grpc_status.latency_ms.unwrap_or(0)
            );
            Ok(ServerType::Grpc)
        } else if http_status.available {
            info!(
                "gRPC unavailable, falling back to HTTP gateway at {}",
                http_status.address
            );
            Ok(ServerType::Http)
        } else {
            Err(CliError::Network {
                message: format!(
                    "No ORCHER servers available.\n\
                     gRPC ({}): {}\n\
                     HTTP ({}): {}\n\n\
                     Start the server with: orcher server start",
                    self.grpc_addr,
                    grpc_status
                        .error
                        .unwrap_or_else(|| "Unknown error".to_string()),
                    self.http_addr,
                    http_status
                        .error
                        .unwrap_or_else(|| "Unknown error".to_string()),
                ),
                source: None,
            })
        }
    }

    /// Get or create a gRPC client (with caching and authentication)
    ///
    /// Authentication is loaded from:
    /// 1. `ORCHER_TOKEN` environment variable
    /// 2. `ORCHER_API_KEY` environment variable
    /// 3. Token from secure storage for the current context
    pub async fn get_grpc_client(&self) -> Result<Arc<OrcherGrpcClient>> {
        let context = self.context.clone();
        self.grpc_client
            .get_or_try_init(|| async {
                let client = OrcherGrpcClient::connect_with_auth(
                    &self.grpc_addr,
                    &self.namespace,
                    Duration::from_secs(30),
                    context.as_deref(),
                )
                .await?;
                Ok(Arc::new(client))
            })
            .await
            .cloned()
    }

    /// The namespace in use
    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    /// Get the context name
    pub fn context(&self) -> Option<&str> {
        self.context.as_deref()
    }

    /// Get HTTP address for fallback operations
    pub fn http_addr(&self) -> &str {
        &self.http_addr
    }

    /// Get gRPC address
    pub fn grpc_addr(&self) -> &str {
        &self.grpc_addr
    }

    /// Connect to the gRPC orchestrator, or return an actionable error if it
    /// isn't reachable.
    ///
    /// The CLI runs all operations over gRPC — there is no HTTP operation
    /// fallback — so a down orchestrator yields a clear "start the server"
    /// message instead of a confusing "not implemented".
    pub async fn connect_grpc(&self) -> Result<OrcherGrpcClient> {
        let namespace = self.namespace.clone();
        self.connect_grpc_in(&namespace).await
    }

    /// Like [`connect_grpc`](Self::connect_grpc) but targeting an explicit
    /// namespace (e.g. `run`'s `--env`) rather than the resolved default.
    pub async fn connect_grpc_in(&self, namespace: &str) -> Result<OrcherGrpcClient> {
        let status = self.check_grpc().await;
        if !status.available {
            return Err(CliError::Network {
                message: format!(
                    "Cannot reach the ORCHER orchestrator at {} (gRPC): {}\n\n\
                     Start it with: orcher server start",
                    self.grpc_addr,
                    status.error.as_deref().unwrap_or("unavailable"),
                ),
                source: None,
            });
        }

        OrcherGrpcClient::connect_with_auth(
            &self.grpc_addr,
            namespace,
            Duration::from_secs(30),
            self.context.as_deref(),
        )
        .await
    }
}

/// Connect to the gRPC orchestrator using settings resolved from the global
/// config (`--server`/`ORCHER_SERVER`/context). Shared by every operation
/// command in place of per-command connect helpers.
pub async fn connect_grpc(global_config: &GlobalConfig) -> Result<OrcherGrpcClient> {
    ConnectionManager::from_config(global_config)
        .connect_grpc()
        .await
}

/// Connect targeting an explicit namespace (see [`ConnectionManager::connect_grpc_in`]).
pub async fn connect_grpc_in(
    global_config: &GlobalConfig,
    namespace: &str,
) -> Result<OrcherGrpcClient> {
    ConnectionManager::from_config(global_config)
        .connect_grpc_in(namespace)
        .await
}

/// Check if gRPC server is available by attempting to connect
/// Probe a gRPC endpoint by opening a channel. `Ok(())` means reachable.
/// Shared with the `orcher server` health checks.
///
/// Uses the same endpoint builder as the operation client, so `https://`
/// (cloud) addresses are probed over TLS with the system trust store. Probing
/// a private-CA cloud still works for reachability; a custom CA only matters
/// once the operation client makes an authenticated call.
pub async fn probe_grpc(address: &str) -> std::result::Result<(), String> {
    let endpoint = crate::client::grpc_client::build_grpc_endpoint(
        address,
        HEALTH_CHECK_TIMEOUT,
        HEALTH_CHECK_TIMEOUT,
        None,
    )
    .map_err(|e| e.to_string())?;

    match tokio::time::timeout(HEALTH_CHECK_TIMEOUT, endpoint.connect()).await {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(e)) => Err(format!("Connection failed: {}", e)),
        Err(_) => Err("Connection timeout".to_string()),
    }
}

/// Check if HTTP gateway is available by calling health endpoint
/// Probe an HTTP health endpoint at `address + health_path`. `Ok(())` means
/// healthy; `Err` carries a human-readable reason.
///
/// Single shared implementation used by both the connection manager and the
/// `orcher server` health checks, so the probe mechanics live in one place.
pub async fn probe_http(address: &str, health_path: &str) -> std::result::Result<(), String> {
    let client = reqwest::Client::builder()
        .timeout(HEALTH_CHECK_TIMEOUT)
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    let health_url = format!("{}{}", address, health_path);

    match client.get(&health_url).send().await {
        Ok(response) => {
            if response.status().is_success() {
                Ok(())
            } else {
                Err(format!(
                    "Health check returned status: {}",
                    response.status()
                ))
            }
        }
        Err(e) => {
            if e.is_connect() {
                Err("Connection refused".to_string())
            } else if e.is_timeout() {
                Err("Connection timeout".to_string())
            } else {
                Err(format!("Request failed: {}", e))
            }
        }
    }
}

/// Normalize server URL to ensure it has a scheme
fn normalize_server_url(url: &str) -> String {
    let trimmed = url.trim();
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        trimmed.to_string()
    } else {
        // Default to http:// for gRPC connections
        format!("http://{}", trimmed)
    }
}

/// Derive the gateway address from the gRPC address, when none is configured.
///
/// An `https://` address without a port is a cloud deployment, which serves
/// the gateway from the same origin. Otherwise the gateway is assumed to
/// listen on port 8000 of the same host, as a self-hosted one does.
fn derive_http_addr(grpc_addr: &str) -> String {
    // If it's the default gRPC address, use default HTTP address
    if grpc_addr == DEFAULT_GRPC_ADDR || grpc_addr == "localhost:50051" {
        return DEFAULT_HTTP_ADDR.to_string();
    }

    // Try to parse and replace port
    if let Ok(url) = url::Url::parse(grpc_addr) {
        if let Some(host) = url.host_str() {
            let scheme = if url.scheme() == "https" {
                "https"
            } else {
                "http"
            };
            if scheme == "https" && url.port().is_none() {
                return format!("https://{}", host);
            }
            return format!("{}://{}:8000", scheme, host);
        }
    }

    // Fallback to default
    DEFAULT_HTTP_ADDR.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_server_url() {
        assert_eq!(
            normalize_server_url("localhost:50051"),
            "http://localhost:50051"
        );
        assert_eq!(
            normalize_server_url("http://localhost:50051"),
            "http://localhost:50051"
        );
        assert_eq!(
            normalize_server_url("https://orcher.example.com"),
            "https://orcher.example.com"
        );
        assert_eq!(
            normalize_server_url("  api.orcher.io:443  "),
            "http://api.orcher.io:443"
        );
    }

    #[test]
    fn test_derive_http_addr() {
        assert_eq!(derive_http_addr(DEFAULT_GRPC_ADDR), DEFAULT_HTTP_ADDR);
        assert_eq!(derive_http_addr("localhost:50051"), DEFAULT_HTTP_ADDR);
        assert_eq!(
            derive_http_addr("http://myhost:50051"),
            "http://myhost:8000"
        );
        assert_eq!(
            derive_http_addr("http://192.168.1.1:50051"),
            "http://192.168.1.1:8000"
        );
        assert_eq!(
            derive_http_addr(crate::constants::DEFAULT_CLOUD_URL),
            crate::constants::DEFAULT_CLOUD_URL
        );
    }

    #[test]
    fn test_server_type_display() {
        assert_eq!(ServerType::Grpc.to_string(), "gRPC");
        assert_eq!(ServerType::Http.to_string(), "HTTP");
    }

    #[tokio::test]
    async fn test_connection_manager_default() {
        // Clear env vars that might interfere
        std::env::remove_var("ORCHER_SERVER");
        std::env::remove_var("ORCHER_NAMESPACE");

        let config = GlobalConfig::default();
        let manager = ConnectionManager::from_config(&config);

        // Without config file, should use defaults
        assert_eq!(manager.http_addr(), DEFAULT_HTTP_ADDR);
    }

    #[tokio::test]
    async fn test_connection_manager_with_server_flag() {
        let config = GlobalConfig {
            server: Some("https://api.orcher.io:50051".to_string()),
            ..Default::default()
        };

        let manager = ConnectionManager::from_config(&config);

        assert_eq!(manager.grpc_addr(), "https://api.orcher.io:50051");
        assert_eq!(manager.http_addr(), "https://api.orcher.io:8000");
    }

    #[tokio::test]
    async fn a_configured_gateway_wins_over_the_derived_one() {
        let derived = ConnectionManager::from_config(&GlobalConfig {
            server: Some("http://engine.internal:50051".to_string()),
            ..Default::default()
        });
        assert_eq!(derived.http_addr(), "http://engine.internal:8000");

        let configured = ConnectionManager::from_config(&GlobalConfig {
            server: Some("http://engine.internal:50051".to_string()),
            api_url: Some("https://gateway.internal/".to_string()),
            ..Default::default()
        });
        assert_eq!(configured.http_addr(), "https://gateway.internal");
        assert_eq!(
            configured.configured_http_addr().as_deref(),
            Some("https://gateway.internal")
        );
    }

    #[test]
    fn a_cloud_address_serves_its_gateway_from_the_same_origin() {
        assert_eq!(
            derive_http_addr("https://api.orcher.io"),
            "https://api.orcher.io"
        );
        assert_eq!(
            derive_http_addr("https://api.orcher.io:443"),
            "https://api.orcher.io"
        );
        assert_eq!(
            derive_http_addr("https://engine.example:50051"),
            "https://engine.example:8000"
        );
    }

    #[tokio::test]
    async fn test_check_unavailable_servers() {
        let manager = ConnectionManager::new(
            "http://localhost:59999",
            "http://localhost:59998",
            "default",
        );

        let grpc_status = manager.check_grpc().await;
        assert!(!grpc_status.available);
        assert!(grpc_status.error.is_some());

        let http_status = manager.check_http().await;
        assert!(!http_status.available);
        assert!(http_status.error.is_some());
    }
}
