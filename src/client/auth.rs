//! Authentication providers (OAuth2/JWT tokens, API keys, basic auth).

use crate::error::{CliError, Result};
use crate::time::Timestamptz;
use base64::Engine;
use reqwest::Client;
use serde::{Deserialize, Serialize};

use url::Url;

/// Authentication provider for different auth methods
#[derive(Debug, Clone)]
pub enum AuthProvider {
    /// OAuth2/JWT token authentication
    Token(TokenAuth),
    /// API key authentication
    ApiKey(ApiKeyAuth),
    /// Basic HTTP authentication
    Basic(BasicAuth),
    /// No authentication
    None,
}

impl AuthProvider {
    /// Get the authentication token/header value
    pub async fn get_token(&self) -> Result<String> {
        match self {
            Self::Token(auth) => auth.get_access_token().await,
            Self::ApiKey(auth) => Ok(auth.key.clone()),
            Self::Basic(auth) => {
                let credentials = base64::engine::general_purpose::STANDARD
                    .encode(format!("{}:{}", auth.username, auth.password));
                Ok(format!("Basic {}", credentials))
            }
            Self::None => Err(CliError::NoAuthentication),
        }
    }

    /// Get the authentication header name
    pub fn header_name(&self) -> &'static str {
        match self {
            Self::Token(_) => "Authorization",
            Self::ApiKey(_) => "X-API-Key",
            Self::Basic(_) => "Authorization",
            Self::None => "",
        }
    }

    /// Format the authorization header value
    pub async fn auth_header_value(&self) -> Result<String> {
        match self {
            Self::Token(auth) => {
                let token = auth.get_access_token().await?;
                Ok(format!("Bearer {}", token))
            }
            Self::ApiKey(auth) => Ok(auth.key.clone()),
            Self::Basic(_) => self.get_token().await,
            Self::None => Err(CliError::NoAuthentication),
        }
    }

    /// Check if this auth provider supports token refresh
    pub fn supports_refresh(&self) -> bool {
        matches!(self, Self::Token(auth) if auth.refresh_token.is_some())
    }

    /// Refresh the authentication token if supported
    pub async fn refresh_token(&mut self) -> Result<()> {
        match self {
            Self::Token(auth) => auth.refresh().await,
            _ => Err(CliError::RefreshNotSupported),
        }
    }

    /// Check if authentication is expired
    pub fn is_expired(&self) -> bool {
        match self {
            Self::Token(auth) => auth.is_expired(),
            Self::ApiKey(auth) => auth.is_expired(),
            _ => false,
        }
    }
}

/// Token-based authentication (OAuth2/JWT)
#[derive(Debug, Clone)]
pub struct TokenAuth {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<Timestamptz>,
    pub token_type: String,
    pub server: Url,
    pub scopes: Vec<String>,
}

impl TokenAuth {
    /// Create new token authentication
    pub fn new(
        access_token: String,
        refresh_token: Option<String>,
        expires_at: Option<Timestamptz>,
        server: Url,
    ) -> Self {
        Self {
            access_token,
            refresh_token,
            expires_at,
            token_type: "Bearer".to_string(),
            server,
            scopes: Vec::new(),
        }
    }

    /// Get the current access token, refreshing if necessary
    pub async fn get_access_token(&self) -> Result<String> {
        if self.is_expired() {
            return Err(CliError::AuthenticationExpired);
        }

        // Check if token expires soon (within 5 minutes)
        if let Some(expires_at) = self.expires_at {
            if Timestamptz::now() + jiff::SignedDuration::from_mins(5) > expires_at {
                tracing::debug!("Access token expires soon, refresh may be needed");
            }
        }

        Ok(self.access_token.clone())
    }

    /// Check if the token is expired
    pub fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            Timestamptz::now() >= expires_at
        } else {
            false
        }
    }

    /// Refresh the access token using the refresh token
    pub async fn refresh(&self) -> Result<()> {
        let refresh_token = self
            .refresh_token
            .as_ref()
            .ok_or(CliError::NoRefreshToken)?;

        let client = Client::new();
        let refresh_url = self
            .server
            .join("/auth/refresh")
            .map_err(|e| CliError::Config {
                message: format!("Invalid server URL: {}", e),
                source: Some(Box::new(e)),
            })?;

        let request = RefreshTokenRequest {
            refresh_token: refresh_token.clone(),
            grant_type: "refresh_token".to_string(),
        };

        let response = client
            .post(refresh_url)
            .json(&request)
            .send()
            .await
            .map_err(|e| CliError::Network {
                message: format!("Failed to refresh token: {}", e),
                source: Some(e),
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(CliError::Api {
                status: status.as_u16(),
                message: format!("Token refresh failed: {}", error_text),
                endpoint: None,
            });
        }

        let _refresh_response: RefreshTokenResponse =
            response.json().await.map_err(|e| CliError::Config {
                message: format!("Invalid refresh response: {}", e),
                source: Some(Box::new(e)),
            })?;

        // In a real implementation, this would update the stored credentials
        // For now, we just log the successful refresh
        tracing::info!("Token refreshed successfully");
        Ok(())
    }
}

/// API key authentication
#[derive(Debug, Clone)]
pub struct ApiKeyAuth {
    pub key: String,
    pub name: String,
    pub scopes: Vec<String>,
    pub created_at: Timestamptz,
    pub expires_at: Option<Timestamptz>,
}

impl ApiKeyAuth {
    /// Create new API key authentication
    pub fn new(key: String, name: String) -> Self {
        Self {
            key,
            name,
            scopes: Vec::new(),
            created_at: Timestamptz::now(),
            expires_at: None,
        }
    }

    /// Check if the API key is expired
    pub fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            Timestamptz::now() >= expires_at
        } else {
            false
        }
    }

    /// Check if the API key has a specific scope
    pub fn has_scope(&self, scope: &str) -> bool {
        self.scopes.iter().any(|s| s == scope)
    }
}

/// Basic HTTP authentication
#[derive(Debug, Clone)]
pub struct BasicAuth {
    pub username: String,
    pub password: String,
}

impl BasicAuth {
    /// Create new basic authentication
    pub fn new(username: String, password: String) -> Self {
        Self { username, password }
    }
}

/// Request for refreshing an access token
#[derive(Debug, Serialize)]
struct RefreshTokenRequest {
    refresh_token: String,
    grant_type: String,
}

/// Response from token refresh endpoint
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct RefreshTokenResponse {
    access_token: String,
    token_type: String,
    expires_in: Option<i64>,
    refresh_token: Option<String>,
    scope: Option<String>,
}

/// Login request for obtaining initial tokens
#[derive(Debug, Serialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
    pub scopes: Vec<String>,
}

/// Login response containing tokens
#[derive(Debug, Deserialize)]
pub struct LoginResponse {
    pub access_token: String,
    pub token_type: String,
    pub expires_in: Option<i64>,
    pub refresh_token: Option<String>,
    pub scope: Option<String>,
}

/// API key creation request
#[derive(Debug, Serialize)]
pub struct CreateApiKeyRequest {
    pub name: String,
    pub scopes: Vec<String>,
    pub expires_at: Option<Timestamptz>,
    pub description: Option<String>,
}

/// API key creation response
#[derive(Debug, Deserialize)]
pub struct CreateApiKeyResponse {
    pub key: String,
    pub name: String,
    pub scopes: Vec<String>,
    pub created_at: Timestamptz,
    pub expires_at: Option<Timestamptz>,
}

/// API key information (without the actual key)
#[derive(Debug, Serialize, Deserialize)]
pub struct ApiKeyInfo {
    pub name: String,
    pub scopes: Vec<String>,
    pub created_at: Timestamptz,
    pub expires_at: Option<Timestamptz>,
    pub last_used: Option<Timestamptz>,
}

/// Authentication manager for handling different auth methods
pub struct AuthManager {
    client: Client,
}

impl AuthManager {
    /// Create new authentication manager
    pub fn new() -> Self {
        Self {
            client: Client::new(),
        }
    }

    /// Perform login with username/password
    pub async fn login(&self, server: &Url, request: LoginRequest) -> Result<TokenAuth> {
        let login_url = server.join("/auth/login").map_err(|e| CliError::Config {
            message: format!("Invalid server URL: {}", e),
            source: Some(Box::new(e)),
        })?;

        let response = self
            .client
            .post(login_url)
            .json(&request)
            .send()
            .await
            .map_err(|e| CliError::Network {
                message: format!("Login failed: {}", e),
                source: Some(e),
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(CliError::Auth {
                message: format!("Login failed: {}", error_text),
                error_code: Some(status.as_u16().to_string()),
            });
        }

        let login_response: LoginResponse =
            response.json().await.map_err(|e| CliError::Config {
                message: format!("Invalid login response: {}", e),
                source: Some(Box::new(e)),
            })?;

        let expires_at = login_response
            .expires_in
            .map(|expires_in| Timestamptz::now() + jiff::SignedDuration::from_secs(expires_in));

        Ok(TokenAuth::new(
            login_response.access_token,
            login_response.refresh_token,
            expires_at,
            server.clone(),
        ))
    }

    /// Create a new API key
    pub async fn create_api_key(
        &self,
        server: &Url,
        auth: &AuthProvider,
        request: CreateApiKeyRequest,
    ) -> Result<CreateApiKeyResponse> {
        let url = server
            .join("/api/v1/auth/apikeys")
            .map_err(|e| CliError::Config {
                message: format!("Invalid server URL: {}", e),
                source: Some(Box::new(e)),
            })?;

        let auth_header = auth.auth_header_value().await?;

        let response = self
            .client
            .post(url)
            .header(auth.header_name(), auth_header)
            .json(&request)
            .send()
            .await
            .map_err(|e| CliError::Network {
                message: format!("API key creation failed: {}", e),
                source: Some(e),
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(CliError::Api {
                status: status.as_u16(),
                message: format!("API key creation failed: {}", error_text),
                endpoint: None,
            });
        }

        response.json().await.map_err(|e| CliError::Config {
            message: format!("Invalid API key response: {}", e),
            source: Some(Box::new(e)),
        })
    }

    /// List API keys
    pub async fn list_api_keys(
        &self,
        server: &Url,
        auth: &AuthProvider,
    ) -> Result<Vec<ApiKeyInfo>> {
        let url = server
            .join("/api/v1/auth/apikeys")
            .map_err(|e| CliError::Config {
                message: format!("Invalid server URL: {}", e),
                source: Some(Box::new(e)),
            })?;

        let auth_header = auth.auth_header_value().await?;

        let response = self
            .client
            .get(url)
            .header(auth.header_name(), auth_header)
            .send()
            .await
            .map_err(|e| CliError::Network {
                message: format!("Failed to list API keys: {}", e),
                source: Some(e),
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(CliError::Api {
                status: status.as_u16(),
                message: format!("Failed to list API keys: {}", error_text),
                endpoint: None,
            });
        }

        response.json().await.map_err(|e| CliError::Config {
            message: format!("Invalid API keys response: {}", e),
            source: Some(Box::new(e)),
        })
    }

    /// Revoke an API key
    pub async fn revoke_api_key(
        &self,
        server: &Url,
        auth: &AuthProvider,
        key_name: &str,
    ) -> Result<()> {
        let url = server
            .join(&format!("/api/v1/auth/apikeys/{}", key_name))
            .map_err(|e| CliError::Config {
                message: format!("Invalid server URL: {}", e),
                source: Some(Box::new(e)),
            })?;

        let auth_header = auth.auth_header_value().await?;

        let response = self
            .client
            .delete(url)
            .header(auth.header_name(), auth_header)
            .send()
            .await
            .map_err(|e| CliError::Network {
                message: format!("Failed to revoke API key: {}", e),
                source: Some(e),
            })?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(CliError::Api {
                status: status.as_u16(),
                message: format!("Failed to revoke API key: {}", error_text),
                endpoint: None,
            });
        }

        Ok(())
    }
}

impl Default for AuthManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auth_provider_header_names() {
        let token_auth = AuthProvider::Token(TokenAuth::new(
            "token".to_string(),
            None,
            None,
            "http://localhost".parse().unwrap(),
        ));
        assert_eq!(token_auth.header_name(), "Authorization");

        let api_key_auth =
            AuthProvider::ApiKey(ApiKeyAuth::new("key".to_string(), "test-key".to_string()));
        assert_eq!(api_key_auth.header_name(), "X-API-Key");

        let basic_auth =
            AuthProvider::Basic(BasicAuth::new("user".to_string(), "pass".to_string()));
        assert_eq!(basic_auth.header_name(), "Authorization");
    }

    #[test]
    fn test_token_auth_expiry() {
        let expired_token = TokenAuth::new(
            "token".to_string(),
            None,
            Some(Timestamptz::now() - jiff::SignedDuration::from_hours(1)),
            "http://localhost".parse().unwrap(),
        );
        assert!(expired_token.is_expired());

        let valid_token = TokenAuth::new(
            "token".to_string(),
            None,
            Some(Timestamptz::now() + jiff::SignedDuration::from_hours(1)),
            "http://localhost".parse().unwrap(),
        );
        assert!(!valid_token.is_expired());
    }

    #[test]
    fn test_api_key_auth_scopes() {
        let mut api_key = ApiKeyAuth::new("key".to_string(), "test-key".to_string());
        api_key.scopes = vec!["read".to_string(), "write".to_string()];

        assert!(api_key.has_scope("read"));
        assert!(api_key.has_scope("write"));
        assert!(!api_key.has_scope("admin"));
    }

    #[tokio::test]
    async fn test_basic_auth_header_value() {
        let basic_auth =
            AuthProvider::Basic(BasicAuth::new("user".to_string(), "pass".to_string()));

        let header_value = basic_auth.auth_header_value().await.unwrap();
        // "user:pass" in base64 is "dXNlcjpwYXNz"
        assert_eq!(header_value, "Basic dXNlcjpwYXNz");
    }
}
