//! HTTP client for the ORCHER REST API.
//!
//! Scope is deliberately narrow: the CLI performs all *operations* over gRPC
//! (see [`crate::client::grpc_client`]). The only thing that still needs HTTP
//! is authentication — `orcher auth login` validates connectivity against the
//! gateway's liveness endpoint. So this client exposes just enough surface to
//! build an authenticated client and health-check it.

use crate::client::{auth::AuthProvider, config::ClientConfig};
use crate::error::{CliError, Result};
use reqwest::{
    header::{HeaderMap, HeaderName, HeaderValue},
    Client, Method, RequestBuilder, Response,
};
use serde::Deserialize;

use crate::time::Timestamptz;
use tracing::{debug, trace};
use url::Url;

/// Minimal HTTP client for ORCHER — used by `orcher auth` for login validation.
#[derive(Debug)]
pub struct OrcherClient {
    /// HTTP client
    client: Client,
    /// Client configuration
    config: ClientConfig,
    /// Authentication provider
    auth: Option<AuthProvider>,
    /// Default request headers
    default_headers: HeaderMap,
}

impl OrcherClient {
    /// Create a new ORCHER client
    pub fn new(config: ClientConfig) -> Result<Self> {
        config.validate()?;

        let mut client_builder = Client::builder()
            .timeout(config.http.timeout)
            .connect_timeout(config.http.pool.connect_timeout)
            .pool_idle_timeout(config.http.pool.keep_alive_timeout)
            .pool_max_idle_per_host(config.http.pool.max_idle_per_host)
            .user_agent(&config.http.user_agent);

        // Configure compression
        if !config.http.gzip {
            client_builder = client_builder.no_gzip();
        }
        if !config.http.brotli {
            client_builder = client_builder.no_brotli();
        }

        // Configure TLS
        if config.base_url.scheme() == "https" {
            if !config.tls.verify_certificates {
                client_builder = client_builder.danger_accept_invalid_certs(true);
            }

            if config.tls.accept_invalid_hostnames {
                client_builder = client_builder.danger_accept_invalid_certs(true);
            }

            // Load custom CA certificate if provided
            if let Some(ca_cert) = config.tls.load_ca_certificate()? {
                client_builder = client_builder.add_root_certificate(ca_cert);
            }
        }

        // Configure redirects
        match &config.http.redirect_policy {
            crate::client::config::RedirectPolicy::None => {
                client_builder = client_builder.redirect(reqwest::redirect::Policy::none());
            }
            crate::client::config::RedirectPolicy::Limited(max) => {
                client_builder = client_builder.redirect(reqwest::redirect::Policy::limited(*max));
            }
            crate::client::config::RedirectPolicy::All => {
                client_builder = client_builder.redirect(reqwest::redirect::Policy::default());
            }
        }

        let client = client_builder.build().map_err(|e| CliError::Config {
            message: format!("Failed to create HTTP client: {}", e),
            source: Some(Box::new(e)),
        })?;

        // Build default headers
        let mut default_headers = HeaderMap::new();
        for (key, value) in &config.http.default_headers {
            let header_name: HeaderName = key.parse().map_err(|e| CliError::Config {
                message: format!("Invalid header name '{}': {}", key, e),
                source: Some(Box::new(e)),
            })?;
            let header_value: HeaderValue = value.parse().map_err(|e| CliError::Config {
                message: format!("Invalid header value '{}': {}", value, e),
                source: Some(Box::new(e)),
            })?;
            default_headers.insert(header_name, header_value);
        }

        Ok(Self {
            client,
            config,
            auth: None,
            default_headers,
        })
    }

    /// Set authentication provider
    pub fn with_auth(mut self, auth: AuthProvider) -> Self {
        self.auth = Some(auth);
        self
    }

    /// Get the current authentication provider
    pub fn auth(&self) -> Option<&AuthProvider> {
        self.auth.as_ref()
    }

    /// Check if the client is authenticated
    pub fn is_authenticated(&self) -> bool {
        self.auth.is_some()
    }

    /// Perform a health check against the ORCHER API
    pub async fn health_check(&self) -> Result<HealthCheckResponse> {
        // `api_url` prefixes /api/v1; the gateway's liveness route is /health/live.
        let url = self.config.api_url("/health/live")?;
        let response = self.get(url).send().await?;
        self.handle_response(response).await
    }

    /// Build a GET request
    fn get(&self, url: Url) -> RequestBuilder {
        self.build_request(Method::GET, url)
    }

    /// Build a request with common configuration
    fn build_request(&self, method: Method, url: Url) -> RequestBuilder {
        self.client
            .request(method, url)
            .headers(self.default_headers.clone())
    }

    /// Handle API response and convert to typed result
    async fn handle_response<T>(&self, response: Response) -> Result<T>
    where
        T: for<'de> Deserialize<'de>,
    {
        let status = response.status();

        if self.config.debug {
            debug!("Response status: {}", status);
        }

        if status.is_success() {
            let body = response.text().await.map_err(|e| CliError::Network {
                message: format!("Failed to read response body: {}", e),
                source: Some(e),
            })?;

            if self.config.debug {
                trace!("Response body: {}", body);
            }

            serde_json::from_str(&body).map_err(|e| CliError::Config {
                message: format!("Failed to parse response JSON: {}", e),
                source: Some(Box::new(e)),
            })
        } else {
            let error_body = response.text().await.unwrap_or_default();

            // Try to parse as API error first
            if let Ok(api_error) = serde_json::from_str::<crate::client::ApiError>(&error_body) {
                Err(CliError::Api {
                    status: status.as_u16(),
                    message: format!("{}: {}", api_error.error, api_error.message),
                    endpoint: None,
                })
            } else {
                Err(crate::client::status_to_error(status, error_body))
            }
        }
    }
}

/// Health check response
#[derive(Debug, Deserialize)]
pub struct HealthCheckResponse {
    pub status: String,
    pub version: String,
    pub timestamp: Timestamptz,
    pub uptime: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::config::ClientConfig;

    #[test]
    fn test_client_creation() {
        let config = ClientConfig::default();
        let client = OrcherClient::new(config);
        assert!(client.is_ok());
    }

    #[test]
    fn test_client_with_auth() {
        let config = ClientConfig::default();
        let client = OrcherClient::new(config).unwrap();
        assert!(!client.is_authenticated());

        let auth = AuthProvider::None;
        let client = client.with_auth(auth);
        assert!(client.is_authenticated());
    }
}
