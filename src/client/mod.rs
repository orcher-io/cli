//! API client layer. Prefers gRPC (lower latency, streaming); falls back to HTTP.

#![allow(dead_code)]

pub mod auth;
pub mod config;
pub mod connection;
pub mod device_flow;
pub mod grpc_client;
pub mod http_client;

pub use auth::*;
#[cfg(test)]
pub use config::*;
pub use http_client::*;

use crate::error::{CliError, Result};
use crate::time::Timestamptz;
use reqwest::StatusCode;
use serde::Deserialize;
use std::time::Duration;

/// Default timeout for API requests
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);

/// Default connection timeout
pub const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Default retry attempts
pub const DEFAULT_RETRY_ATTEMPTS: usize = 3;

/// Default retry delay
pub const DEFAULT_RETRY_DELAY: Duration = Duration::from_millis(1000);

/// Health check response from ORCHER API
#[derive(Debug, Deserialize)]
pub struct HealthCheck {
    pub status: String,
    pub version: String,
    pub timestamp: Timestamptz,
}

/// API error response
#[derive(Debug, Deserialize)]
pub struct ApiError {
    pub error: String,
    pub message: String,
    pub code: Option<String>,
    pub details: Option<serde_json::Value>,
}

/// Convert HTTP status codes to appropriate CLI errors
/// Sends the credential the gRPC client would (ORCHER_TOKEN, ORCHER_API_KEY,
/// or the context's stored login) with a request to a gateway, which checks
/// it like the engine does.
pub fn with_gateway_auth(
    request: reqwest::RequestBuilder,
    global_config: &crate::utils::GlobalConfig,
) -> reqwest::RequestBuilder {
    match grpc_client::AuthInterceptor::from_env_and_storage(global_config.context.as_deref())
        .token()
    {
        Some(token) => request.bearer_auth(token),
        None => request,
    }
}

pub fn status_to_error(status: StatusCode, message: String) -> CliError {
    match status {
        StatusCode::UNAUTHORIZED => CliError::Unauthorized,
        StatusCode::FORBIDDEN => CliError::Permission {
            message: "Insufficient permissions".to_string(),
            path: None,
        },
        StatusCode::NOT_FOUND => CliError::NotFound {
            resource_type: "resource".to_string(),
            name: "unknown".to_string(),
            namespace: None,
        },
        StatusCode::CONFLICT => CliError::AlreadyExists {
            resource_type: "resource".to_string(),
            name: "unknown".to_string(),
            namespace: None,
        },
        StatusCode::TOO_MANY_REQUESTS => CliError::Network {
            message: "Rate limit exceeded".to_string(),
            source: None,
        },
        StatusCode::REQUEST_TIMEOUT | StatusCode::GATEWAY_TIMEOUT => CliError::Timeout {
            operation: "API request".to_string(),
            duration: DEFAULT_TIMEOUT,
        },
        StatusCode::INTERNAL_SERVER_ERROR => CliError::Internal {
            message: "Server internal error".to_string(),
            context: vec![],
        },
        StatusCode::BAD_GATEWAY | StatusCode::SERVICE_UNAVAILABLE => CliError::Network {
            message: "Service unavailable".to_string(),
            source: None,
        },
        _ => CliError::Api {
            status: status.as_u16(),
            message,
            endpoint: None,
        },
    }
}

/// Check if an error is retryable
pub fn is_retryable_error(error: &CliError) -> bool {
    match error {
        CliError::Network { .. } => true,
        CliError::Timeout { .. } => true,
        CliError::Api { status, .. } => {
            // Retry on server errors but not client errors
            *status >= 500
        }
        _ => false,
    }
}

/// Retry configuration for API calls
#[derive(Debug, Clone)]
pub struct RetryConfig {
    pub max_attempts: usize,
    pub initial_delay: Duration,
    pub max_delay: Duration,
    pub multiplier: f64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: DEFAULT_RETRY_ATTEMPTS,
            initial_delay: DEFAULT_RETRY_DELAY,
            max_delay: Duration::from_secs(30),
            multiplier: 2.0,
        }
    }
}

/// Execute a function with exponential backoff retry logic
pub async fn retry_with_backoff<F, Fut, T>(mut operation: F, config: RetryConfig) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T>>,
{
    let mut delay = config.initial_delay;
    let mut last_error = None;

    for attempt in 0..config.max_attempts {
        match operation().await {
            Ok(result) => return Ok(result),
            Err(error) if is_retryable_error(&error) && attempt < config.max_attempts - 1 => {
                tracing::debug!(
                    "Attempt {} failed, retrying in {:?}: {}",
                    attempt + 1,
                    delay,
                    error
                );

                tokio::time::sleep(delay).await;
                delay = std::cmp::min(
                    Duration::from_secs_f64(delay.as_secs_f64() * config.multiplier),
                    config.max_delay,
                );
                last_error = Some(error);
            }
            Err(error) => return Err(error),
        }
    }

    Err(last_error.unwrap_or_else(|| CliError::Internal {
        message: "Retry loop completed without error or result".to_string(),
        context: vec!["retry_with_backoff".to_string()],
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_config_default() {
        let config = ClientConfig::default();
        assert_eq!(config.http.timeout, DEFAULT_TIMEOUT);
        assert!(config.tls.verify_certificates);
    }

    #[test]
    fn test_status_to_error() {
        let error = status_to_error(StatusCode::UNAUTHORIZED, "Unauthorized".to_string());
        matches!(error, CliError::Unauthorized);

        let error = status_to_error(StatusCode::NOT_FOUND, "Not found".to_string());
        matches!(error, CliError::NotFound { .. });
    }

    #[test]
    fn test_is_retryable_error() {
        assert!(is_retryable_error(&CliError::Network {
            message: "Connection failed".to_string(),
            source: None,
        }));

        assert!(is_retryable_error(&CliError::Api {
            status: 500,
            message: "Internal server error".to_string(),
            endpoint: None,
        }));

        assert!(!is_retryable_error(&CliError::Api {
            status: 400,
            message: "Bad request".to_string(),
            endpoint: None,
        }));

        assert!(!is_retryable_error(&CliError::Unauthorized));
    }

    #[tokio::test]
    async fn test_retry_with_backoff_success() {
        let mut attempt_count = 0;
        let result = retry_with_backoff(
            || {
                attempt_count += 1;
                async move {
                    if attempt_count < 2 {
                        Err(CliError::Network {
                            message: "Connection failed".to_string(),
                            source: None,
                        })
                    } else {
                        Ok("success")
                    }
                }
            },
            RetryConfig::default(),
        )
        .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "success");
        assert_eq!(attempt_count, 2);
    }

    #[tokio::test]
    async fn test_retry_with_backoff_non_retryable() {
        let mut attempt_count = 0;
        let result: std::result::Result<&str, _> = retry_with_backoff(
            || {
                attempt_count += 1;
                async move { Err(CliError::Unauthorized) }
            },
            RetryConfig::default(),
        )
        .await;

        assert!(result.is_err());
        assert_eq!(attempt_count, 1); // Should not retry
    }
}
