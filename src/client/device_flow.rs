//! OAuth 2.0 Device Authorization Grant (RFC 8628) for CLI login.

use crate::error::{CliError, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use url::Url;

/// Default polling interval in seconds
const DEFAULT_POLL_INTERVAL: u64 = 5;

/// Maximum polling attempts before giving up
const MAX_POLL_ATTEMPTS: usize = 120; // 10 minutes at 5-second intervals

/// Request to initiate device flow
#[derive(Debug, Clone, Serialize)]
struct DeviceCodeRequest {
    provider: String,
}

/// Response from device code request
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceCodeResponse {
    /// Device code for polling (must be kept by client)
    pub device_code: String,
    /// User-friendly code to display (e.g., "WDJB-MJHT")
    pub user_code: String,
    /// URL where user should enter the code
    pub verification_uri: String,
    /// Optional complete URI with code pre-filled
    pub verification_uri_complete: Option<String>,
    /// Seconds until code expires
    pub expires_in: i64,
    /// Minimum seconds between polls
    pub interval: i32,
}

/// Request to poll for device token
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DeviceTokenRequest {
    device_code: String,
    provider: String,
}

/// Response from device token polling
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum DeviceTokenResponse {
    /// Authorization is still pending
    #[serde(rename_all = "camelCase")]
    Pending {
        /// Seconds until next poll
        interval: i32,
    },
    /// Authorization successful - tokens available
    #[serde(rename_all = "camelCase")]
    Success {
        /// Access token for API requests
        access_token: String,
        /// Refresh token for getting new access tokens
        refresh_token: String,
        /// Token type (usually "Bearer")
        token_type: String,
        /// Access token expiration time in seconds
        expires_in: i64,
        /// User information
        user: UserInfo,
    },
    /// User denied the authorization request
    Denied,
    /// Device code has expired
    Expired,
    /// Client is polling too frequently (slow down)
    #[serde(rename_all = "camelCase")]
    SlowDown {
        /// New minimum interval in seconds
        interval: i32,
    },
}

/// User information from successful authentication
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserInfo {
    /// User ID
    pub id: String,
    /// User email
    pub email: String,
    /// User display name
    pub name: Option<String>,
    /// Whether this is a new user
    pub is_new: bool,
    /// OAuth provider used
    pub oauth_provider: String,
}

/// Complete authentication tokens
#[derive(Debug, Clone)]
pub struct AuthTokens {
    /// Access token for API requests
    pub access_token: String,
    /// Refresh token for getting new access tokens
    pub refresh_token: String,
    /// Token type (usually "Bearer")
    pub token_type: String,
    /// Access token expiration time in seconds
    pub expires_in: i64,
    /// User information
    pub user: UserInfo,
}

/// Device flow authenticator
pub struct DeviceFlowAuthenticator {
    server_url: Url,
    client: Client,
}

impl DeviceFlowAuthenticator {
    /// Create a new device flow authenticator
    pub fn new(server_url: Url) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_default();

        Self { server_url, client }
    }

    /// Perform device flow authentication
    ///
    /// This method orchestrates the entire device flow:
    /// 1. Request device code
    /// 2. Display code to user
    /// 3. Poll for authorization
    /// 4. Return tokens on success
    ///
    /// # Arguments
    ///
    /// * `provider` - OAuth provider name (github, google, microsoft)
    ///
    /// # Returns
    ///
    /// Returns `AuthTokens` on successful authentication
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Network request fails
    /// - User denies authorization
    /// - Device code expires
    /// - Server returns an error
    pub async fn authenticate(&self, provider: &str) -> Result<AuthTokens> {
        // Request device code
        let device_response = self.request_device_code(provider).await?;
        let device_code = device_response.device_code.clone();

        // Display instructions to user (handled by caller)
        // This method returns the response so the caller can display it

        // Poll for authorization
        self.poll_for_authorization(provider, &device_code, &device_response)
            .await
    }

    /// Request a device code from the server
    ///
    /// # Arguments
    ///
    /// * `provider` - OAuth provider name
    ///
    /// # Returns
    ///
    /// Returns `DeviceCodeResponse` with device_code, user code and verification URI
    pub async fn request_device_code(&self, provider: &str) -> Result<DeviceCodeResponse> {
        let url = self
            .server_url
            .join("/api/v1/auth/oauth/device/code")
            .map_err(|e| CliError::Config {
                message: format!("Invalid server URL: {}", e),
                source: Some(Box::new(e)),
            })?;

        let request = DeviceCodeRequest {
            provider: provider.to_string(),
        };

        let response = self
            .client
            .post(url)
            .json(&request)
            .send()
            .await
            .map_err(|e| CliError::Network {
                message: format!("Failed to request device code: {}", e),
                source: Some(e),
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());

            return Err(CliError::Api {
                status: status.as_u16(),
                message: format!("Device code request failed: {}", error_text),
                endpoint: Some("/api/v1/auth/oauth/device/code".to_string()),
            });
        }

        let device_response =
            response
                .json::<DeviceCodeResponse>()
                .await
                .map_err(|e| CliError::Config {
                    message: format!("Failed to parse device code response: {}", e),
                    source: Some(Box::new(e)),
                })?;

        Ok(device_response)
    }

    /// Poll for device authorization completion
    ///
    /// This method polls the server at the specified interval until:
    /// - User authorizes (success)
    /// - User denies (error)
    /// - Code expires (error)
    /// - Max attempts reached (error)
    ///
    /// # Arguments
    ///
    /// * `provider` - OAuth provider name
    /// * `device_response` - Device code response from initial request
    ///
    /// # Returns
    ///
    /// Returns `AuthTokens` on successful authorization
    async fn poll_for_authorization(
        &self,
        provider: &str,
        device_code: &str,
        device_response: &DeviceCodeResponse,
    ) -> Result<AuthTokens> {
        let url = self
            .server_url
            .join("/api/v1/auth/oauth/device/token")
            .map_err(|e| CliError::Config {
                message: format!("Invalid server URL: {}", e),
                source: Some(Box::new(e)),
            })?;

        let mut interval = device_response.interval.max(DEFAULT_POLL_INTERVAL as i32) as u64;
        let mut attempts = 0;

        loop {
            if attempts >= MAX_POLL_ATTEMPTS {
                return Err(CliError::Timeout {
                    operation: "device authorization".to_string(),
                    duration: Duration::from_secs(interval * MAX_POLL_ATTEMPTS as u64),
                });
            }

            attempts += 1;

            // Wait before polling (except first attempt)
            if attempts > 1 {
                tokio::time::sleep(Duration::from_secs(interval)).await;
            }

            let request = DeviceTokenRequest {
                device_code: device_code.to_string(),
                provider: provider.to_string(),
            };

            let response = self
                .client
                .post(url.clone())
                .json(&request)
                .send()
                .await
                .map_err(|e| CliError::Network {
                    message: format!("Failed to poll for token: {}", e),
                    source: Some(e),
                })?;

            if !response.status().is_success() {
                let status = response.status();
                let error_text = response
                    .text()
                    .await
                    .unwrap_or_else(|_| "Unknown error".to_string());

                return Err(CliError::Api {
                    status: status.as_u16(),
                    message: format!("Token poll failed: {}", error_text),
                    endpoint: Some("/api/v1/auth/oauth/device/token".to_string()),
                });
            }

            let token_response =
                response
                    .json::<DeviceTokenResponse>()
                    .await
                    .map_err(|e| CliError::Config {
                        message: format!("Failed to parse token response: {}", e),
                        source: Some(Box::new(e)),
                    })?;

            match token_response {
                DeviceTokenResponse::Pending {
                    interval: new_interval,
                } => {
                    // Update interval if provided
                    interval = new_interval.max(DEFAULT_POLL_INTERVAL as i32) as u64;
                    continue;
                }
                DeviceTokenResponse::Success {
                    access_token,
                    refresh_token,
                    token_type,
                    expires_in,
                    user,
                } => {
                    return Ok(AuthTokens {
                        access_token,
                        refresh_token,
                        token_type,
                        expires_in,
                        user,
                    });
                }
                DeviceTokenResponse::Denied => {
                    return Err(CliError::Auth {
                        message: "Authorization denied by user".to_string(),
                        error_code: Some("access_denied".to_string()),
                    });
                }
                DeviceTokenResponse::Expired => {
                    return Err(CliError::Auth {
                        message: "Device code has expired. Please try again.".to_string(),
                        error_code: Some("expired_token".to_string()),
                    });
                }
                DeviceTokenResponse::SlowDown {
                    interval: new_interval,
                } => {
                    // Server requests us to slow down
                    interval = new_interval.max(interval as i32) as u64;
                    continue;
                }
            }
        }
    }

    /// Get the device code response without starting polling
    ///
    /// This is useful when you want to display the code and handle
    /// polling separately (e.g., with a progress bar).
    pub async fn start_flow(&self, provider: &str) -> Result<DeviceCodeResponse> {
        self.request_device_code(provider).await
    }

    /// Poll once for device authorization
    ///
    /// This method polls the server once and returns the current status.
    /// It's useful when you want to control the polling loop yourself.
    ///
    /// # Arguments
    ///
    /// * `provider` - OAuth provider name
    /// * `device_code` - Device code from initial request
    ///
    /// # Returns
    ///
    /// Returns the current authorization status
    pub async fn poll_once(
        &self,
        provider: &str,
        device_code: &str,
    ) -> Result<DeviceTokenResponse> {
        let url = self
            .server_url
            .join("/api/v1/auth/oauth/device/token")
            .map_err(|e| CliError::Config {
                message: format!("Invalid server URL: {}", e),
                source: Some(Box::new(e)),
            })?;

        let request = DeviceTokenRequest {
            device_code: device_code.to_string(),
            provider: provider.to_string(),
        };

        let response = self
            .client
            .post(url)
            .json(&request)
            .send()
            .await
            .map_err(|e| CliError::Network {
                message: format!("Failed to poll for token: {}", e),
                source: Some(e),
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response
                .text()
                .await
                .unwrap_or_else(|_| "Unknown error".to_string());

            return Err(CliError::Api {
                status: status.as_u16(),
                message: format!("Token poll failed: {}", error_text),
                endpoint: Some("/api/v1/auth/oauth/device/token".to_string()),
            });
        }

        // Get response text for debugging
        let response_text = response.text().await.map_err(|e| CliError::Network {
            message: format!("Failed to read response body: {}", e),
            source: Some(e),
        })?;

        tracing::debug!("Device token response body: {}", response_text);

        let token_response =
            serde_json::from_str::<DeviceTokenResponse>(&response_text).map_err(|e| {
                CliError::Config {
                    message: format!(
                        "Failed to parse token response: {}. Response body: {}",
                        e, response_text
                    ),
                    source: Some(Box::new(e)),
                }
            })?;

        Ok(token_response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_code_request_serialization() {
        let request = DeviceCodeRequest {
            provider: "github".to_string(),
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("github"));
    }

    #[test]
    fn test_device_token_request_serialization() {
        let request = DeviceTokenRequest {
            device_code: "test123".to_string(),
            provider: "github".to_string(),
        };

        let json = serde_json::to_string(&request).unwrap();
        assert!(json.contains("test123"));
        assert!(json.contains("github"));
        assert!(json.contains("deviceCode")); // camelCase
    }

    #[test]
    fn test_device_token_response_pending() {
        let json = r#"{"status":"pending","interval":5}"#;
        let response: DeviceTokenResponse = serde_json::from_str(json).unwrap();

        match response {
            DeviceTokenResponse::Pending { interval } => {
                assert_eq!(interval, 5);
            }
            _ => panic!("Expected Pending variant"),
        }
    }

    #[test]
    fn test_device_token_response_denied() {
        let json = r#"{"status":"denied"}"#;
        let response: DeviceTokenResponse = serde_json::from_str(json).unwrap();

        match response {
            DeviceTokenResponse::Denied => {}
            _ => panic!("Expected Denied variant"),
        }
    }

    #[test]
    fn test_device_token_response_expired() {
        let json = r#"{"status":"expired"}"#;
        let response: DeviceTokenResponse = serde_json::from_str(json).unwrap();

        match response {
            DeviceTokenResponse::Expired => {}
            _ => panic!("Expected Expired variant"),
        }
    }

    #[test]
    fn test_user_info_deserialization() {
        let json = r#"{
            "id": "123",
            "email": "user@example.com",
            "name": "Test User",
            "isNew": false,
            "oauthProvider": "github"
        }"#;

        let user: UserInfo = serde_json::from_str(json).unwrap();
        assert_eq!(user.id, "123");
        assert_eq!(user.email, "user@example.com");
        assert_eq!(user.name, Some("Test User".to_string()));
        assert!(!user.is_new);
        assert_eq!(user.oauth_provider, "github");
    }
}
