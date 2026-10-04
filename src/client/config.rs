//! API client configuration (TLS, timeouts, connection pooling).

use crate::error::{CliError, Result};
use reqwest::Certificate;
use std::path::PathBuf;
use std::time::Duration;
use url::Url;

/// TLS configuration for API connections
#[derive(Debug, Clone)]
pub struct TlsConfig {
    /// Whether to verify TLS certificates
    pub verify_certificates: bool,
    /// Custom CA certificate path
    pub ca_cert_path: Option<PathBuf>,
    /// Client certificate path (for mutual TLS)
    pub client_cert_path: Option<PathBuf>,
    /// Client private key path (for mutual TLS)
    pub client_key_path: Option<PathBuf>,
    /// Accept invalid hostnames
    pub accept_invalid_hostnames: bool,
    /// Minimum TLS version
    pub min_tls_version: Option<TlsVersion>,
}

impl Default for TlsConfig {
    fn default() -> Self {
        Self {
            verify_certificates: true,
            ca_cert_path: None,
            client_cert_path: None,
            client_key_path: None,
            accept_invalid_hostnames: false,
            min_tls_version: Some(TlsVersion::V1_2),
        }
    }
}

impl TlsConfig {
    /// Create a new TLS configuration with secure defaults
    pub fn new() -> Self {
        Self::default()
    }

    /// Create an insecure TLS configuration (for testing)
    pub fn insecure() -> Self {
        Self {
            verify_certificates: false,
            accept_invalid_hostnames: true,
            ..Default::default()
        }
    }

    /// Set custom CA certificate path
    pub fn with_ca_cert<P: Into<PathBuf>>(mut self, path: P) -> Self {
        self.ca_cert_path = Some(path.into());
        self
    }

    /// Set client certificate and key for mutual TLS
    pub fn with_client_cert<P: Into<PathBuf>>(mut self, cert_path: P, key_path: P) -> Self {
        self.client_cert_path = Some(cert_path.into());
        self.client_key_path = Some(key_path.into());
        self
    }

    /// Load CA certificate from file
    pub fn load_ca_certificate(&self) -> Result<Option<Certificate>> {
        if let Some(ca_path) = &self.ca_cert_path {
            let cert_data = std::fs::read(ca_path).map_err(|e| CliError::Config {
                message: format!("Failed to read CA certificate from {:?}: {}", ca_path, e),
                source: Some(Box::new(e)),
            })?;

            let certificate = Certificate::from_pem(&cert_data).map_err(|e| CliError::Config {
                message: format!("Failed to parse CA certificate: {}", e),
                source: Some(Box::new(e)),
            })?;

            Ok(Some(certificate))
        } else {
            Ok(None)
        }
    }
}

/// TLS version enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TlsVersion {
    V1_0,
    V1_1,
    V1_2,
    V1_3,
}

/// Connection pool configuration
#[derive(Debug, Clone)]
pub struct PoolConfig {
    /// Maximum number of idle connections per host
    pub max_idle_per_host: usize,
    /// Connection timeout
    pub connect_timeout: Duration,
    /// Pool timeout
    pub pool_timeout: Duration,
    /// Keep-alive timeout
    pub keep_alive_timeout: Duration,
    /// Enable HTTP/2
    pub http2_prior_knowledge: bool,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            max_idle_per_host: 10,
            connect_timeout: Duration::from_secs(10),
            pool_timeout: Duration::from_secs(30),
            keep_alive_timeout: Duration::from_secs(90),
            http2_prior_knowledge: false,
        }
    }
}

/// HTTP client configuration
#[derive(Debug, Clone)]
pub struct HttpConfig {
    /// Request timeout
    pub timeout: Duration,
    /// User agent string
    pub user_agent: String,
    /// Default headers
    pub default_headers: std::collections::HashMap<String, String>,
    /// Enable gzip compression
    pub gzip: bool,
    /// Enable brotli compression
    pub brotli: bool,
    /// Redirect policy
    pub redirect_policy: RedirectPolicy,
    /// Connection pool configuration
    pub pool: PoolConfig,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(30),
            user_agent: format!("{}/{}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION")),
            default_headers: std::collections::HashMap::new(),
            gzip: true,
            brotli: true,
            redirect_policy: RedirectPolicy::Limited(10),
            pool: PoolConfig::default(),
        }
    }
}

/// Redirect policy for HTTP requests
#[derive(Debug, Clone)]
pub enum RedirectPolicy {
    /// No redirects allowed
    None,
    /// Limited number of redirects
    Limited(usize),
    /// Follow all redirects
    All,
}

/// Complete client configuration
#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// Base URL for the ORCHER API
    pub base_url: Url,
    /// TLS configuration
    pub tls: TlsConfig,
    /// HTTP configuration
    pub http: HttpConfig,
    /// API version
    pub api_version: String,
    /// Enable debug logging
    pub debug: bool,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            base_url: "http://localhost:8000".parse().unwrap(),
            tls: TlsConfig::default(),
            http: HttpConfig::default(),
            api_version: "v1".to_string(),
            debug: false,
        }
    }
}

impl ClientConfig {
    /// Create a new client configuration
    pub fn new(base_url: Url) -> Self {
        Self {
            base_url,
            ..Default::default()
        }
    }

    /// Create configuration for a secure production environment
    pub fn production(base_url: Url) -> Self {
        Self {
            base_url,
            tls: TlsConfig::new(),
            http: HttpConfig {
                timeout: Duration::from_secs(60),
                ..Default::default()
            },
            debug: false,
            ..Default::default()
        }
    }

    /// Create configuration for local development
    pub fn development(base_url: Url) -> Self {
        Self {
            base_url,
            tls: TlsConfig::insecure(),
            http: HttpConfig {
                timeout: Duration::from_secs(10),
                ..Default::default()
            },
            debug: true,
            ..Default::default()
        }
    }

    /// Set TLS configuration
    pub fn with_tls(mut self, tls: TlsConfig) -> Self {
        self.tls = tls;
        self
    }

    /// Set HTTP configuration
    pub fn with_http(mut self, http: HttpConfig) -> Self {
        self.http = http;
        self
    }

    /// Enable debug mode
    pub fn with_debug(mut self, debug: bool) -> Self {
        self.debug = debug;
        self
    }

    /// Set API version
    pub fn with_api_version<S: Into<String>>(mut self, version: S) -> Self {
        self.api_version = version.into();
        self
    }

    /// Create ClientConfig from CLI context
    pub fn from_context(context: &crate::config::Context) -> Result<Self> {
        let base_url: Url = context.server.parse().map_err(|e| CliError::Config {
            message: format!("Invalid server URL '{}': {}", context.server, e),
            source: Some(Box::new(e)),
        })?;

        let mut config = if base_url.scheme() == "https" {
            ClientConfig::production(base_url)
        } else {
            ClientConfig::development(base_url)
        };

        // Set timeout if specified
        if let Some(timeout_str) = &context.timeout {
            let timeout = crate::utils::parse_duration(timeout_str)?;
            config.http.timeout = timeout;
        }

        // Configure TLS if specified
        if let Some(tls_config) = &context.tls {
            if let Some(insecure) = tls_config.insecure {
                if insecure {
                    config.tls.verify_certificates = false;
                    config.tls.accept_invalid_hostnames = true;
                }
            }

            if let Some(ca_cert) = &tls_config.ca_cert {
                config.tls.ca_cert_path = Some(ca_cert.into());
            }

            if let Some(client_cert) = &tls_config.client_cert {
                config.tls.client_cert_path = Some(client_cert.into());
            }

            if let Some(client_key) = &tls_config.client_key {
                config.tls.client_key_path = Some(client_key.into());
            }
        }

        Ok(config)
    }

    /// Set request timeout
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.http.timeout = timeout;
        self
    }

    /// Add a default header
    pub fn with_header<K: Into<String>, V: Into<String>>(mut self, key: K, value: V) -> Self {
        self.http.default_headers.insert(key.into(), value.into());
        self
    }

    /// Build the API base path
    pub fn api_base_path(&self) -> String {
        format!("/api/{}", self.api_version)
    }

    /// Build a full API URL
    pub fn api_url<P: AsRef<str>>(&self, path: P) -> Result<Url> {
        let full_path = format!("{}{}", self.api_base_path(), path.as_ref());
        self.base_url
            .join(&full_path)
            .map_err(|e| CliError::Config {
                message: format!("Failed to build API URL: {}", e),
                source: Some(Box::new(e)),
            })
    }

    /// Validate the configuration
    pub fn validate(&self) -> Result<()> {
        // Check if base URL is valid
        if self.base_url.scheme() != "http" && self.base_url.scheme() != "https" {
            return Err(CliError::Config {
                message: "Base URL must use HTTP or HTTPS scheme".to_string(),
                source: None,
            });
        }

        // Check TLS configuration
        if self.base_url.scheme() == "https" {
            if let Some(ca_path) = &self.tls.ca_cert_path {
                if !ca_path.exists() {
                    return Err(CliError::Config {
                        message: format!("CA certificate file not found: {:?}", ca_path),
                        source: None,
                    });
                }
            }

            if let Some(cert_path) = &self.tls.client_cert_path {
                if !cert_path.exists() {
                    return Err(CliError::Config {
                        message: format!("Client certificate file not found: {:?}", cert_path),
                        source: None,
                    });
                }
            }

            if let Some(key_path) = &self.tls.client_key_path {
                if !key_path.exists() {
                    return Err(CliError::Config {
                        message: format!("Client private key file not found: {:?}", key_path),
                        source: None,
                    });
                }
            }
        }

        // Validate timeouts
        if self.http.timeout.as_secs() == 0 {
            return Err(CliError::Config {
                message: "HTTP timeout must be greater than 0".to_string(),
                source: None,
            });
        }

        if self.http.pool.connect_timeout.as_secs() == 0 {
            return Err(CliError::Config {
                message: "Connection timeout must be greater than 0".to_string(),
                source: None,
            });
        }

        Ok(())
    }
}

/// Configuration builder for fluent API
pub struct ClientConfigBuilder {
    config: ClientConfig,
}

impl ClientConfigBuilder {
    /// Create a new configuration builder
    pub fn new(base_url: Url) -> Self {
        Self {
            config: ClientConfig::new(base_url),
        }
    }

    /// Set TLS configuration
    pub fn tls(mut self, tls: TlsConfig) -> Self {
        self.config.tls = tls;
        self
    }

    /// Set HTTP timeout
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.config.http.timeout = timeout;
        self
    }

    /// Add a default header
    pub fn header<K: Into<String>, V: Into<String>>(mut self, key: K, value: V) -> Self {
        self.config
            .http
            .default_headers
            .insert(key.into(), value.into());
        self
    }

    /// Enable debug mode
    pub fn debug(mut self) -> Self {
        self.config.debug = true;
        self
    }

    /// Set API version
    pub fn api_version<S: Into<String>>(mut self, version: S) -> Self {
        self.config.api_version = version.into();
        self
    }

    /// Build the configuration
    pub fn build(self) -> Result<ClientConfig> {
        self.config.validate()?;
        Ok(self.config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_config_default() {
        let config = ClientConfig::default();
        assert_eq!(config.base_url.as_str(), "http://localhost:8000/");
        assert_eq!(config.api_version, "v1");
        assert!(config.tls.verify_certificates);
        assert!(!config.debug);
    }

    #[test]
    fn test_client_config_production() {
        let url = "https://api.orcher.example.com".parse().unwrap();
        let config = ClientConfig::production(url);
        assert_eq!(config.base_url.scheme(), "https");
        assert!(config.tls.verify_certificates);
        assert!(!config.debug);
        assert_eq!(config.http.timeout, Duration::from_secs(60));
    }

    #[test]
    fn test_client_config_development() {
        let url = "http://localhost:8080".parse().unwrap();
        let config = ClientConfig::development(url);
        assert_eq!(config.base_url.scheme(), "http");
        assert!(!config.tls.verify_certificates);
        assert!(config.debug);
        assert_eq!(config.http.timeout, Duration::from_secs(10));
    }

    #[test]
    fn test_api_url_building() {
        let config = ClientConfig::default();
        let url = config.api_url("/workflows").unwrap();
        assert_eq!(url.as_str(), "http://localhost:8000/api/v1/workflows");
    }

    #[test]
    fn test_config_validation() {
        // Valid HTTP configuration
        let config = ClientConfig::default();
        assert!(config.validate().is_ok());

        // Invalid scheme
        let config = ClientConfig {
            base_url: "ftp://example.com".parse().unwrap(),
            ..Default::default()
        };
        assert!(config.validate().is_err());

        // Zero timeout
        let mut config = ClientConfig::default();
        config.http.timeout = Duration::from_secs(0);
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_builder() {
        let url = "https://api.example.com".parse().unwrap();
        let config = ClientConfigBuilder::new(url)
            .timeout(Duration::from_secs(45))
            .header("X-Custom-Header", "value")
            .debug()
            .api_version("v2")
            .build()
            .unwrap();

        assert_eq!(config.http.timeout, Duration::from_secs(45));
        assert_eq!(
            config.http.default_headers.get("X-Custom-Header"),
            Some(&"value".to_string())
        );
        assert!(config.debug);
        assert_eq!(config.api_version, "v2");
    }

    #[test]
    fn test_tls_config() {
        let tls = TlsConfig::new();
        assert!(tls.verify_certificates);
        assert!(!tls.accept_invalid_hostnames);

        let tls_insecure = TlsConfig::insecure();
        assert!(!tls_insecure.verify_certificates);
        assert!(tls_insecure.accept_invalid_hostnames);
    }
}
