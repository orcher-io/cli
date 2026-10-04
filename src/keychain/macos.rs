//! macOS credential storage via Security Framework.

use crate::error::{CliError, Result};
use security_framework::os::macos::keychain::SecKeychain;
use security_framework::passwords::{
    delete_generic_password, get_generic_password, set_generic_password,
};

/// Store a credential.
pub async fn store_credential(service: &str, account: &str, data: &[u8]) -> Result<()> {
    tokio::task::spawn_blocking({
        let service = service.to_string();
        let account = account.to_string();
        let data = data.to_vec();

        move || {
            set_generic_password(&service, &account, &data).map_err(|e| CliError::Keychain {
                source: None,
                message: format!("Failed to store credential in macOS keychain: {}", e),
            })
        }
    })
    .await
    .map_err(|e| CliError::Keychain {
        source: None,
        message: format!("Task join error: {}", e),
    })?
}

/// Retrieve a credential, or `None` if not found.
pub async fn get_credential(service: &str, account: &str) -> Result<Option<Vec<u8>>> {
    tokio::task::spawn_blocking({
        let service = service.to_string();
        let account = account.to_string();

        move || {
            match get_generic_password(&service, &account) {
                Ok(data) => Ok(Some(data)),
                Err(e) => {
                    // Check if the error is "item not found" which is expected
                    let error_str = e.to_string().to_lowercase();
                    if error_str.contains("itemnotfound")
                        || error_str.contains("item not found")
                        || error_str.contains("-25300")
                        || error_str.contains("errSecItemNotFound".to_lowercase().as_str())
                        || e.code() == -25300
                    {
                        Ok(None)
                    } else {
                        Err(CliError::Keychain {
                            source: None,
                            message: format!(
                                "Failed to retrieve credential from macOS keychain: {}",
                                e
                            ),
                        })
                    }
                }
            }
        }
    })
    .await
    .map_err(|e| CliError::Keychain {
        source: None,
        message: format!("Task join error: {}", e),
    })?
}

/// Delete a credential. No-op if not found.
pub async fn delete_credential(service: &str, account: &str) -> Result<()> {
    tokio::task::spawn_blocking({
        let service = service.to_string();
        let account = account.to_string();

        move || {
            match delete_generic_password(&service, &account) {
                Ok(_) => Ok(()),
                Err(e) => {
                    // Check if the error is "item not found" which is acceptable for delete
                    let error_str = e.to_string();
                    if error_str.contains("errSecItemNotFound") || error_str.contains("-25300") {
                        Ok(())
                    } else {
                        Err(CliError::Keychain {
                            source: None,
                            message: format!(
                                "Failed to delete credential from macOS keychain: {}",
                                e
                            ),
                        })
                    }
                }
            }
        }
    })
    .await
    .map_err(|e| CliError::Keychain {
        source: None,
        message: format!("Task join error: {}", e),
    })?
}

/// List all accounts for a service (stub — returns empty).
pub async fn list_accounts(service: &str) -> Result<Vec<String>> {
    tokio::task::spawn_blocking({
        let _service = service.to_string();

        move || {
            // security-framework has no call that lists a service's items;
            // that takes SecItemCopyMatching. Nothing in the CLI lists
            // accounts, so this reports none.
            Ok(Vec::new())
        }
    })
    .await
    .map_err(|e| CliError::Keychain {
        source: None,
        message: format!("Task join error: {}", e),
    })?
}

/// Check if the default keychain is accessible.
#[allow(dead_code)]
pub fn is_keychain_available() -> bool {
    SecKeychain::default().is_ok()
}

/// Return a human-readable keychain status string.
#[allow(dead_code)]
pub fn get_keychain_status() -> Result<String> {
    match SecKeychain::default() {
        Ok(_keychain) => {
            // Get basic keychain information
            // SecKeychain doesn't have a public status() method
            // Just return basic availability information
            Ok("macOS Keychain - Available".to_string())
        }
        Err(e) => Err(CliError::Keychain {
            source: None,
            message: format!("Failed to access default keychain: {}", e),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_keychain_availability() {
        // This test only checks if the function runs without panicking
        // The actual result depends on the system configuration
        let _ = is_keychain_available();
    }

    /// Writes to, reads from and deletes an item in the login keychain of
    /// whoever runs it, so it is opt-in rather than part of every test run:
    /// a CI runner running as a background service cannot unlock that
    /// keychain, and a routine test run should not touch a person's own.
    /// Run it with `cargo test -p orcher-cli -- --ignored keychain` from a
    /// desktop session.
    #[tokio::test]
    #[ignore = "writes to the login keychain; run with --ignored from a desktop session"]
    async fn test_credential_operations() {
        // Only run this test if we're actually on macOS and keychain is available
        if !is_keychain_available() {
            return;
        }

        let service = "orcher-cli-test";
        let account = "test-account";
        let test_data = b"test-credential-data";

        // Clean up any existing test data
        let _ = delete_credential(service, account).await;

        // Test store
        let result = store_credential(service, account, test_data).await;
        assert!(result.is_ok(), "Failed to store credential: {:?}", result);

        // Test get
        let retrieved = get_credential(service, account).await;
        assert!(
            retrieved.is_ok(),
            "Failed to get credential: {:?}",
            retrieved
        );

        if let Ok(Some(data)) = retrieved {
            assert_eq!(data, test_data, "Retrieved data doesn't match stored data");
        }

        // Test delete
        let delete_result = delete_credential(service, account).await;
        assert!(
            delete_result.is_ok(),
            "Failed to delete credential: {:?}",
            delete_result
        );

        // Verify deletion
        let after_delete = get_credential(service, account).await;
        assert!(after_delete.is_ok(), "Error checking deleted credential");
        assert!(
            after_delete.unwrap().is_none(),
            "Credential was not deleted"
        );
    }

    #[test]
    fn test_keychain_status() {
        if is_keychain_available() {
            let status = get_keychain_status();
            assert!(
                status.is_ok(),
                "Failed to get keychain status: {:?}",
                status
            );
        }
    }
}
