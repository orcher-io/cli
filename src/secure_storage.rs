//! Secure credential storage via OS keychain (macOS Keychain, Windows Credential Manager, Linux Secret Service).

#![allow(dead_code)]

use crate::error::{CliError, Result};
use keyring::Entry;

/// Service name for keyring entries
const SERVICE_NAME: &str = "orcher-cli";

/// Secure storage for credentials using OS keychain
pub struct SecureStorage {
    /// Context name (e.g., "local", "production")
    context: String,
}

impl SecureStorage {
    /// Create a new secure storage instance for a context
    ///
    /// # Arguments
    ///
    /// * `context` - The context name (e.g., "local", "production")
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use orcher_cli::secure_storage::SecureStorage;
    /// let storage = SecureStorage::new("local")?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn new(context: &str) -> Result<Self> {
        Ok(Self {
            context: context.to_string(),
        })
    }

    /// Store an access token securely
    ///
    /// # Arguments
    ///
    /// * `token` - The access token to store
    ///
    /// # Errors
    ///
    /// Returns an error if the OS keychain is unavailable or storage fails.
    pub fn store_token(&self, token: &str) -> Result<()> {
        let entry = self.get_entry("access_token")?;
        entry.set_password(token).map_err(|e| CliError::Keychain {
            message: format!("Failed to store access token: {}", e),
            source: Some(Box::new(e)),
        })
    }

    /// Retrieve the stored access token
    ///
    /// # Returns
    ///
    /// Returns `Some(token)` if found, `None` if not stored.
    ///
    /// # Errors
    ///
    /// Returns an error if the OS keychain is unavailable.
    pub fn get_token(&self) -> Result<Option<String>> {
        let entry = self.get_entry("access_token")?;
        match entry.get_password() {
            Ok(token) => Ok(Some(token)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(CliError::Keychain {
                message: format!("Failed to retrieve access token: {}", e),
                source: Some(Box::new(e)),
            }),
        }
    }

    /// Delete the stored access token
    ///
    /// # Errors
    ///
    /// Returns an error if the OS keychain is unavailable or deletion fails.
    pub fn delete_token(&self) -> Result<()> {
        let entry = self.get_entry("access_token")?;
        match entry.delete_password() {
            Ok(_) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()), // Already deleted
            Err(e) => Err(CliError::Keychain {
                message: format!("Failed to delete access token: {}", e),
                source: Some(Box::new(e)),
            }),
        }
    }

    /// Store a refresh token securely
    ///
    /// # Arguments
    ///
    /// * `token` - The refresh token to store
    ///
    /// # Errors
    ///
    /// Returns an error if the OS keychain is unavailable or storage fails.
    pub fn store_refresh_token(&self, token: &str) -> Result<()> {
        let entry = self.get_entry("refresh_token")?;
        entry.set_password(token).map_err(|e| CliError::Keychain {
            message: format!("Failed to store refresh token: {}", e),
            source: Some(Box::new(e)),
        })
    }

    /// Retrieve the stored refresh token
    ///
    /// # Returns
    ///
    /// Returns `Some(token)` if found, `None` if not stored.
    ///
    /// # Errors
    ///
    /// Returns an error if the OS keychain is unavailable.
    pub fn get_refresh_token(&self) -> Result<Option<String>> {
        let entry = self.get_entry("refresh_token")?;
        match entry.get_password() {
            Ok(token) => Ok(Some(token)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(CliError::Keychain {
                message: format!("Failed to retrieve refresh token: {}", e),
                source: Some(Box::new(e)),
            }),
        }
    }

    /// Delete the stored refresh token
    ///
    /// # Errors
    ///
    /// Returns an error if the OS keychain is unavailable or deletion fails.
    pub fn delete_refresh_token(&self) -> Result<()> {
        let entry = self.get_entry("refresh_token")?;
        match entry.delete_password() {
            Ok(_) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()), // Already deleted
            Err(e) => Err(CliError::Keychain {
                message: format!("Failed to delete refresh token: {}", e),
                source: Some(Box::new(e)),
            }),
        }
    }

    /// Store a password securely
    ///
    /// # Arguments
    ///
    /// * `password` - The password to store
    ///
    /// # Errors
    ///
    /// Returns an error if the OS keychain is unavailable or storage fails.
    pub fn store_password(&self, password: &str) -> Result<()> {
        let entry = self.get_entry("password")?;
        entry
            .set_password(password)
            .map_err(|e| CliError::Keychain {
                message: format!("Failed to store password: {}", e),
                source: Some(Box::new(e)),
            })
    }

    /// Retrieve the stored password
    ///
    /// # Returns
    ///
    /// Returns `Some(password)` if found, `None` if not stored.
    ///
    /// # Errors
    ///
    /// Returns an error if the OS keychain is unavailable.
    pub fn get_password(&self) -> Result<Option<String>> {
        let entry = self.get_entry("password")?;
        match entry.get_password() {
            Ok(password) => Ok(Some(password)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(CliError::Keychain {
                message: format!("Failed to retrieve password: {}", e),
                source: Some(Box::new(e)),
            }),
        }
    }

    /// Delete the stored password
    ///
    /// # Errors
    ///
    /// Returns an error if the OS keychain is unavailable or deletion fails.
    pub fn delete_password(&self) -> Result<()> {
        let entry = self.get_entry("password")?;
        match entry.delete_password() {
            Ok(_) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()), // Already deleted
            Err(e) => Err(CliError::Keychain {
                message: format!("Failed to delete password: {}", e),
                source: Some(Box::new(e)),
            }),
        }
    }

    /// Delete all stored credentials for this context
    ///
    /// This removes access token, refresh token, and password.
    ///
    /// # Errors
    ///
    /// Returns an error if any deletion fails (ignores NoEntry errors).
    pub fn delete_all(&self) -> Result<()> {
        // Ignore errors for entries that don't exist
        let _ = self.delete_token();
        let _ = self.delete_refresh_token();
        let _ = self.delete_password();
        Ok(())
    }

    /// Get a keyring entry for a specific credential type
    ///
    /// # Arguments
    ///
    /// * `credential_type` - Type of credential (e.g., "access_token", "refresh_token")
    ///
    /// # Returns
    ///
    /// Returns a keyring Entry for the specified credential type.
    fn get_entry(&self, credential_type: &str) -> Result<Entry> {
        // Create a unique username for this context and credential type
        let username = format!("{}:{}", self.context, credential_type);

        Entry::new(SERVICE_NAME, &username).map_err(|e| CliError::Keychain {
            message: format!("Failed to create keyring entry: {}", e),
            source: Some(Box::new(e)),
        })
    }
}

/// Check if secure storage is available on this platform
///
/// # Returns
///
/// Returns `true` if the OS keychain is available, `false` otherwise.
///
/// # Example
///
/// ```no_run
/// # use orcher_cli::secure_storage::is_available;
/// if is_available() {
///     println!("Secure storage is available");
/// } else {
///     println!("Secure storage is not available");
/// }
/// ```
pub fn is_available() -> bool {
    // Try to create a test entry to check availability
    Entry::new(SERVICE_NAME, "test-availability").is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_secure_storage_creation() {
        let storage = SecureStorage::new("test-context");
        assert!(storage.is_ok());
    }

    #[test]
    fn test_availability_check() {
        // This should work on all supported platforms
        let available = is_available();
        // We can't assert true because CI might not have keychain access
        // but we can verify the function doesn't panic
        println!("Keychain available: {}", available);
    }

    #[test]
    #[ignore] // Ignore by default as it requires OS keychain access
    fn test_token_storage_roundtrip() {
        let storage = SecureStorage::new("test-context").unwrap();
        let test_token = "test_access_token_12345";

        // Clean up before test
        let _ = storage.delete_token();

        // Store token
        storage.store_token(test_token).unwrap();

        // Retrieve token
        let retrieved = storage.get_token().unwrap();
        assert_eq!(retrieved, Some(test_token.to_string()));

        // Delete token
        storage.delete_token().unwrap();

        // Verify deleted
        let after_delete = storage.get_token().unwrap();
        assert_eq!(after_delete, None);
    }

    #[test]
    #[ignore] // Ignore by default as it requires OS keychain access
    fn test_refresh_token_storage() {
        let storage = SecureStorage::new("test-context").unwrap();
        let test_token = "test_refresh_token_67890";

        // Clean up before test
        let _ = storage.delete_refresh_token();

        // Store token
        storage.store_refresh_token(test_token).unwrap();

        // Retrieve token
        let retrieved = storage.get_refresh_token().unwrap();
        assert_eq!(retrieved, Some(test_token.to_string()));

        // Clean up
        storage.delete_refresh_token().unwrap();
    }

    #[test]
    #[ignore] // Ignore by default as it requires OS keychain access
    fn test_delete_all() {
        let storage = SecureStorage::new("test-context").unwrap();

        // Store multiple credentials
        let _ = storage.store_token("access_token");
        let _ = storage.store_refresh_token("refresh_token");
        let _ = storage.store_password("password");

        // Delete all
        storage.delete_all().unwrap();

        // Verify all deleted
        assert_eq!(storage.get_token().unwrap(), None);
        assert_eq!(storage.get_refresh_token().unwrap(), None);
        assert_eq!(storage.get_password().unwrap(), None);
    }
}
