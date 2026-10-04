//! Cross-platform credential storage via OS keychain.

#![allow(dead_code)]

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

use crate::error::{CliError, Result};
use crate::time::Timestamptz;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Service name for ORCHER CLI credentials
pub const ORCHER_SERVICE_NAME: &str = "orcher-cli";

/// Credential entry for secure storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialEntry {
    /// E.g. "token", "api_key".
    pub credential_type: String,
    /// Serialized credential data.
    pub data: Vec<u8>,
    pub created_at: Timestamptz,
    pub accessed_at: Option<Timestamptz>,
    /// Associated server URL.
    pub server: String,
    pub metadata: HashMap<String, String>,
}

/// Cross-platform credential storage via OS keychain.
pub struct KeychainManager {
    service_name: String,
}

impl KeychainManager {
    /// Create a new keychain manager with the default service name.
    pub fn new() -> Self {
        Self {
            service_name: ORCHER_SERVICE_NAME.to_string(),
        }
    }

    /// Create a keychain manager with a custom service name.
    pub fn with_service_name(service_name: String) -> Self {
        Self { service_name }
    }

    /// Store a credential in the keychain
    pub async fn store_credential(
        &self,
        account: &str,
        credential: &CredentialEntry,
    ) -> Result<()> {
        let serialized = serde_json::to_vec(credential).map_err(|e| CliError::Keychain {
            source: None,
            message: format!("Failed to serialize credential: {}", e),
        })?;

        self.store_raw(&self.service_name, account, &serialized)
            .await
    }

    /// Retrieve a credential from the keychain
    pub async fn get_credential(&self, account: &str) -> Result<Option<CredentialEntry>> {
        match self.get_raw(&self.service_name, account).await? {
            Some(data) => {
                let mut credential: CredentialEntry =
                    serde_json::from_slice(&data).map_err(|e| CliError::Keychain {
                        source: None,
                        message: format!("Failed to deserialize credential: {}", e),
                    })?;

                // Update access time
                credential.accessed_at = Some(Timestamptz::now());
                self.store_credential(account, &credential).await?;

                Ok(Some(credential))
            }
            None => Ok(None),
        }
    }

    /// Delete a credential from the keychain
    pub async fn delete_credential(&self, account: &str) -> Result<()> {
        self.delete_raw(&self.service_name, account).await
    }

    /// List all credential accounts for this service
    pub async fn list_accounts(&self) -> Result<Vec<String>> {
        self.list_raw(&self.service_name).await
    }

    /// Check if a credential exists
    pub async fn has_credential(&self, account: &str) -> Result<bool> {
        Ok(self.get_raw(&self.service_name, account).await?.is_some())
    }

    /// Store a token credential
    pub async fn store_token(
        &self,
        account: &str,
        access_token: &str,
        refresh_token: Option<&str>,
        server: &str,
    ) -> Result<()> {
        let mut data = HashMap::new();
        data.insert("access_token".to_string(), access_token.to_string());
        if let Some(refresh) = refresh_token {
            data.insert("refresh_token".to_string(), refresh.to_string());
        }

        let credential_data = serde_json::to_vec(&data).map_err(|e| CliError::Keychain {
            source: None,
            message: format!("Failed to serialize token data: {}", e),
        })?;

        let credential = CredentialEntry {
            credential_type: "token".to_string(),
            data: credential_data,
            created_at: Timestamptz::now(),
            accessed_at: None,
            server: server.to_string(),
            metadata: HashMap::new(),
        };

        self.store_credential(account, &credential).await
    }

    /// Get a token credential
    pub async fn get_token(&self, account: &str) -> Result<Option<(String, Option<String>)>> {
        if let Some(credential) = self.get_credential(account).await? {
            if credential.credential_type != "token" {
                return Err(CliError::Keychain {
                    source: None,
                    message: "Credential is not a token".to_string(),
                });
            }

            let data: HashMap<String, String> =
                serde_json::from_slice(&credential.data).map_err(|e| CliError::Keychain {
                    source: None,
                    message: format!("Failed to deserialize token data: {}", e),
                })?;

            let access_token = data
                .get("access_token")
                .ok_or_else(|| CliError::Keychain {
                    source: None,
                    message: "Access token not found in credential".to_string(),
                })?
                .clone();

            let refresh_token = data.get("refresh_token").cloned();

            Ok(Some((access_token, refresh_token)))
        } else {
            Ok(None)
        }
    }

    /// Store an API key credential
    pub async fn store_api_key(
        &self,
        account: &str,
        api_key: &str,
        server: &str,
        scopes: Vec<String>,
    ) -> Result<()> {
        let mut data = HashMap::new();
        data.insert("api_key".to_string(), api_key.to_string());
        data.insert("scopes".to_string(), scopes.join(","));

        let credential_data = serde_json::to_vec(&data).map_err(|e| CliError::Keychain {
            source: None,
            message: format!("Failed to serialize API key data: {}", e),
        })?;

        let credential = CredentialEntry {
            credential_type: "api_key".to_string(),
            data: credential_data,
            created_at: Timestamptz::now(),
            accessed_at: None,
            server: server.to_string(),
            metadata: HashMap::new(),
        };

        self.store_credential(account, &credential).await
    }

    /// Get an API key credential
    pub async fn get_api_key(&self, account: &str) -> Result<Option<(String, Vec<String>)>> {
        if let Some(credential) = self.get_credential(account).await? {
            if credential.credential_type != "api_key" {
                return Err(CliError::Keychain {
                    source: None,
                    message: "Credential is not an API key".to_string(),
                });
            }

            let data: HashMap<String, String> =
                serde_json::from_slice(&credential.data).map_err(|e| CliError::Keychain {
                    source: None,
                    message: format!("Failed to deserialize API key data: {}", e),
                })?;

            let api_key = data
                .get("api_key")
                .ok_or_else(|| CliError::Keychain {
                    source: None,
                    message: "API key not found in credential".to_string(),
                })?
                .clone();

            let scopes = data
                .get("scopes")
                .map(|s| s.split(',').map(|scope| scope.to_string()).collect())
                .unwrap_or_default();

            Ok(Some((api_key, scopes)))
        } else {
            Ok(None)
        }
    }

    async fn store_raw(&self, service: &str, account: &str, data: &[u8]) -> Result<()> {
        #[cfg(target_os = "macos")]
        return macos::store_credential(service, account, data).await;

        #[cfg(target_os = "linux")]
        return linux::store_credential(service, account, data).await;

        #[cfg(target_os = "windows")]
        return windows::store_credential(service, account, data).await;

        #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
        {
            Err(CliError::Keychain {
                source: None,
                message: "Keychain not supported on this platform".to_string(),
            })
        }
    }

    async fn get_raw(&self, service: &str, account: &str) -> Result<Option<Vec<u8>>> {
        #[cfg(target_os = "macos")]
        return macos::get_credential(service, account).await;

        #[cfg(target_os = "linux")]
        return linux::get_credential(service, account).await;

        #[cfg(target_os = "windows")]
        return windows::get_credential(service, account).await;

        #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
        {
            Err(CliError::Keychain {
                source: None,
                message: "Keychain not supported on this platform".to_string(),
            })
        }
    }

    async fn delete_raw(&self, service: &str, account: &str) -> Result<()> {
        #[cfg(target_os = "macos")]
        return macos::delete_credential(service, account).await;

        #[cfg(target_os = "linux")]
        return linux::delete_credential(service, account).await;

        #[cfg(target_os = "windows")]
        return windows::delete_credential(service, account).await;

        #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
        {
            Err(CliError::Keychain {
                source: None,
                message: "Keychain not supported on this platform".to_string(),
            })
        }
    }

    async fn list_raw(&self, service: &str) -> Result<Vec<String>> {
        #[cfg(target_os = "macos")]
        return macos::list_accounts(service).await;

        #[cfg(target_os = "linux")]
        return linux::list_accounts(service).await;

        #[cfg(target_os = "windows")]
        return windows::list_accounts(service).await;

        #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
        {
            Err(CliError::Keychain {
                source: None,
                message: "Keychain not supported on this platform".to_string(),
            })
        }
    }
}

impl Default for KeychainManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_keychain_manager_creation() {
        let manager = KeychainManager::new();
        assert_eq!(manager.service_name, ORCHER_SERVICE_NAME);

        let custom_manager = KeychainManager::with_service_name("test-service".to_string());
        assert_eq!(custom_manager.service_name, "test-service");
    }

    #[test]
    fn test_credential_entry_serialization() {
        let entry = CredentialEntry {
            credential_type: "token".to_string(),
            data: b"test-data".to_vec(),
            created_at: Timestamptz::now(),
            accessed_at: None,
            server: "https://api.example.com".to_string(),
            metadata: HashMap::new(),
        };

        let serialized = serde_json::to_vec(&entry).unwrap();
        let deserialized: CredentialEntry = serde_json::from_slice(&serialized).unwrap();

        assert_eq!(entry.credential_type, deserialized.credential_type);
        assert_eq!(entry.data, deserialized.data);
        assert_eq!(entry.server, deserialized.server);
    }
}
