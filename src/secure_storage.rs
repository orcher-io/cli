//! Credential storage: the OS keychain (macOS Keychain, Windows Credential
//! Manager, Linux Secret Service), or, where there is none (a server, a
//! container, CI) or `ORCHER_CREDENTIAL_STORE=file` asks for it, a
//! `credentials` file beside the config file that only its owner can read.

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
    /// # Errors
    ///
    /// Returns an error if neither the OS keychain nor the credentials file
    /// can be written.
    pub fn store_token(&self, token: &str) -> Result<()> {
        self.store("access_token", token)
    }

    /// Retrieve the stored access token, or `None` if none is stored.
    pub fn get_token(&self) -> Result<Option<String>> {
        self.get("access_token")
    }

    /// Delete the stored access token
    pub fn delete_token(&self) -> Result<()> {
        self.delete("access_token")
    }

    /// Store a refresh token securely
    pub fn store_refresh_token(&self, token: &str) -> Result<()> {
        self.store("refresh_token", token)
    }

    /// Retrieve the stored refresh token, or `None` if none is stored.
    pub fn get_refresh_token(&self) -> Result<Option<String>> {
        self.get("refresh_token")
    }

    /// Delete the stored refresh token
    pub fn delete_refresh_token(&self) -> Result<()> {
        self.delete("refresh_token")
    }

    /// Store a password securely
    pub fn store_password(&self, password: &str) -> Result<()> {
        self.store("password", password)
    }

    /// Retrieve the stored password, or `None` if none is stored.
    pub fn get_password(&self) -> Result<Option<String>> {
        self.get("password")
    }

    /// Delete the stored password
    pub fn delete_password(&self) -> Result<()> {
        self.delete("password")
    }

    /// Delete all stored credentials for this context
    ///
    /// This removes access token, refresh token, and password.
    pub fn delete_all(&self) -> Result<()> {
        let _ = self.delete_token();
        let _ = self.delete_refresh_token();
        let _ = self.delete_password();
        Ok(())
    }

    /// Store a credential in the keychain, or in the credentials file when
    /// there is no keychain to use.
    fn store(&self, kind: &str, value: &str) -> Result<()> {
        if !file_store_forced() {
            match self.get_entry(kind)?.set_password(value) {
                Ok(()) => return Ok(()),
                Err(e) => warn_falling_back(&e),
            }
        }
        CredentialsFile::update(|creds| {
            creds
                .entry(self.context.clone())
                .or_default()
                .insert(kind.to_string(), value.to_string());
        })
    }

    /// A credential from the keychain, else from the credentials file.
    fn get(&self, kind: &str) -> Result<Option<String>> {
        if !file_store_forced() {
            match self.get_entry(kind)?.get_password() {
                Ok(value) => return Ok(Some(value)),
                // Not in the keychain: it may have been saved to the file
                // when the keychain could not be used.
                Err(keyring::Error::NoEntry) => {}
                Err(e) => tracing::debug!("keychain unavailable ({}); reading the file", e),
            }
        }
        Ok(CredentialsFile::read()?
            .get(&self.context)
            .and_then(|c| c.get(kind))
            .cloned())
    }

    /// Delete a credential wherever it is stored.
    fn delete(&self, kind: &str) -> Result<()> {
        if !file_store_forced() {
            match self.get_entry(kind)?.delete_password() {
                Ok(_) | Err(keyring::Error::NoEntry) => {}
                Err(e) => tracing::debug!("keychain unavailable ({}); using the file", e),
            }
        }
        CredentialsFile::update(|creds| {
            if let Some(c) = creds.get_mut(&self.context) {
                c.remove(kind);
                if c.is_empty() {
                    creds.remove(&self.context);
                }
            }
        })
    }

    /// Get a keyring entry for a specific credential type
    fn get_entry(&self, credential_type: &str) -> Result<Entry> {
        // A unique username for this context and credential type
        let username = format!("{}:{}", self.context, credential_type);

        Entry::new(SERVICE_NAME, &username).map_err(|e| CliError::Keychain {
            message: format!("Failed to create keyring entry: {}", e),
            source: Some(Box::new(e)),
        })
    }
}

/// `ORCHER_CREDENTIAL_STORE=file` keeps credentials in the file even where a
/// keychain exists: for CI, containers and tests.
fn file_store_forced() -> bool {
    std::env::var(CREDENTIAL_STORE_ENV).is_ok_and(|v| v.eq_ignore_ascii_case("file"))
}

fn warn_falling_back(e: &keyring::Error) {
    eprintln!(
        "Note: no OS keychain to use ({}); keeping credentials in {} (readable by you only).",
        e,
        CredentialsFile::path()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| "the credentials file".to_string())
    );
}

/// Environment variable choosing the credential store.
pub const CREDENTIAL_STORE_ENV: &str = "ORCHER_CREDENTIAL_STORE";

/// Credentials per context, as kept in the credentials file.
type Credentials = std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>>;

/// The credentials file: `credentials` beside the config file, readable and
/// writable by its owner only.
struct CredentialsFile;

impl CredentialsFile {
    fn path() -> Result<std::path::PathBuf> {
        Ok(
            crate::config::Config::config_path()?
                .with_file_name(crate::constants::CREDENTIALS_FILE),
        )
    }

    fn read() -> Result<Credentials> {
        let path = Self::path()?;
        match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).map_err(|e| CliError::Keychain {
                message: format!("Cannot read {}: {}", path.display(), e),
                source: Some(Box::new(e)),
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Credentials::new()),
            Err(e) => Err(CliError::io_with_path(
                "Failed to read credentials",
                path.display().to_string(),
                e,
            )),
        }
    }

    fn update(change: impl FnOnce(&mut Credentials)) -> Result<()> {
        let path = Self::path()?;
        let mut creds = Self::read()?;
        change(&mut creds);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| {
                CliError::io_with_path("Failed to create directory", dir.display().to_string(), e)
            })?;
        }
        let text = serde_json::to_string_pretty(&creds)?;
        write_private(&path, text.as_bytes()).map_err(|e| {
            CliError::io_with_path("Failed to write credentials", path.display().to_string(), e)
        })
    }
}

/// Write a file that only its owner can read.
fn write_private(path: &std::path::Path, contents: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(contents)
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

    /// Runs `body` with credentials kept in a file of its own.
    fn with_file_store(body: impl FnOnce(&std::path::Path)) {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var(
            crate::constants::CONFIG_FILE_ENV,
            dir.path().join("config.yaml"),
        );
        std::env::set_var(CREDENTIAL_STORE_ENV, "file");
        body(dir.path());
        std::env::remove_var(CREDENTIAL_STORE_ENV);
        std::env::remove_var(crate::constants::CONFIG_FILE_ENV);
    }

    #[test]
    fn credentials_round_trip_through_the_file() {
        with_file_store(|dir| {
            let storage = SecureStorage::new("cloud").unwrap();
            assert_eq!(storage.get_token().unwrap(), None);
            storage.store_token("access-1").unwrap();
            storage.store_refresh_token("refresh-1").unwrap();
            SecureStorage::new("other")
                .unwrap()
                .store_token("access-2")
                .unwrap();
            assert_eq!(storage.get_token().unwrap().as_deref(), Some("access-1"));
            assert_eq!(
                storage.get_refresh_token().unwrap().as_deref(),
                Some("refresh-1")
            );

            let file = dir.join("credentials");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
                assert_eq!(mode, 0o600, "only the owner may read credentials");
            }

            storage.delete_all().unwrap();
            assert_eq!(storage.get_token().unwrap(), None);
            let left = std::fs::read_to_string(&file).unwrap();
            assert!(left.contains("access-2") && !left.contains("access-1"));
        });
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
