//! Linux credential storage via Secret Service (GNOME Keyring, KWallet, etc.).

use crate::error::{CliError, Result};

/// Store a credential.
pub async fn store_credential(service: &str, account: &str, data: &[u8]) -> Result<()> {
    use secret_service::{EncryptionType, SecretService};
    use std::collections::HashMap;

    let ss = SecretService::connect(EncryptionType::Dh)
        .await
        .map_err(|e| CliError::Keychain {
            source: None,
            message: format!("Failed to connect to Secret Service: {}", e),
        })?;

    let collection = ss
        .get_default_collection()
        .await
        .map_err(|e| CliError::Keychain {
            source: None,
            message: format!("Failed to get default collection: {}", e),
        })?;

    // Unlock collection if needed
    if collection
        .is_locked()
        .await
        .map_err(|e| CliError::Keychain {
            source: None,
            message: format!("Failed to check collection lock status: {}", e),
        })?
    {
        collection.unlock().await.map_err(|e| CliError::Keychain {
            source: None,
            message: format!("Failed to unlock collection: {}", e),
        })?;
    }

    // Prepare attributes for the credential
    let mut attributes = HashMap::new();
    attributes.insert("service", service);
    attributes.insert("account", account);
    attributes.insert("application", "orcher-cli");

    let label = format!("ORCHER CLI credential for {} ({})", service, account);

    // Store the credential
    collection
        .create_item(
            &label,
            attributes,
            data,
            true, // replace existing
            "text/plain",
        )
        .await
        .map_err(|e| CliError::Keychain {
            source: None,
            message: format!("Failed to create item in Secret Service: {}", e),
        })?;

    Ok(())
}

/// Retrieve a credential, or `None` if not found.
pub async fn get_credential(service: &str, account: &str) -> Result<Option<Vec<u8>>> {
    use secret_service::{EncryptionType, SecretService};
    use std::collections::HashMap;

    let ss = SecretService::connect(EncryptionType::Dh)
        .await
        .map_err(|e| CliError::Keychain {
            source: None,
            message: format!("Failed to connect to Secret Service: {}", e),
        })?;

    let collection = ss
        .get_default_collection()
        .await
        .map_err(|e| CliError::Keychain {
            source: None,
            message: format!("Failed to get default collection: {}", e),
        })?;

    // Unlock collection if needed
    if collection
        .is_locked()
        .await
        .map_err(|e| CliError::Keychain {
            source: None,
            message: format!("Failed to check collection lock status: {}", e),
        })?
    {
        collection.unlock().await.map_err(|e| CliError::Keychain {
            source: None,
            message: format!("Failed to unlock collection: {}", e),
        })?;
    }

    // Prepare search attributes
    let mut attributes = HashMap::new();
    attributes.insert("service", service);
    attributes.insert("account", account);
    attributes.insert("application", "orcher-cli");

    // Search for matching items
    let items = collection
        .search_items(attributes)
        .await
        .map_err(|e| CliError::Keychain {
            source: None,
            message: format!("Failed to search items in Secret Service: {}", e),
        })?;

    if items.is_empty() {
        return Ok(None);
    }

    // Get the first matching item
    let item = &items[0];
    let secret = item.get_secret().await.map_err(|e| CliError::Keychain {
        source: None,
        message: format!("Failed to get secret from item: {}", e),
    })?;

    Ok(Some(secret))
}

/// Delete a credential (all matching items).
pub async fn delete_credential(service: &str, account: &str) -> Result<()> {
    use secret_service::{EncryptionType, SecretService};
    use std::collections::HashMap;

    let ss = SecretService::connect(EncryptionType::Dh)
        .await
        .map_err(|e| CliError::Keychain {
            source: None,
            message: format!("Failed to connect to Secret Service: {}", e),
        })?;

    let collection = ss
        .get_default_collection()
        .await
        .map_err(|e| CliError::Keychain {
            source: None,
            message: format!("Failed to get default collection: {}", e),
        })?;

    // Unlock collection if needed
    if collection
        .is_locked()
        .await
        .map_err(|e| CliError::Keychain {
            source: None,
            message: format!("Failed to check collection lock status: {}", e),
        })?
    {
        collection.unlock().await.map_err(|e| CliError::Keychain {
            source: None,
            message: format!("Failed to unlock collection: {}", e),
        })?;
    }

    // Prepare search attributes
    let mut attributes = HashMap::new();
    attributes.insert("service", service);
    attributes.insert("account", account);
    attributes.insert("application", "orcher-cli");

    // Search for matching items
    let items = collection
        .search_items(attributes)
        .await
        .map_err(|e| CliError::Keychain {
            source: None,
            message: format!("Failed to search items in Secret Service: {}", e),
        })?;

    // Delete all matching items
    for item in items {
        item.delete().await.map_err(|e| CliError::Keychain {
            source: None,
            message: format!("Failed to delete item from Secret Service: {}", e),
        })?;
    }

    Ok(())
}

/// List all accounts for a service.
pub async fn list_accounts(service: &str) -> Result<Vec<String>> {
    use secret_service::{EncryptionType, SecretService};
    use std::collections::HashMap;

    let ss = SecretService::connect(EncryptionType::Dh)
        .await
        .map_err(|e| CliError::Keychain {
            source: None,
            message: format!("Failed to connect to Secret Service: {}", e),
        })?;

    let collection = ss
        .get_default_collection()
        .await
        .map_err(|e| CliError::Keychain {
            source: None,
            message: format!("Failed to get default collection: {}", e),
        })?;

    // Unlock collection if needed
    if collection
        .is_locked()
        .await
        .map_err(|e| CliError::Keychain {
            source: None,
            message: format!("Failed to check collection lock status: {}", e),
        })?
    {
        collection.unlock().await.map_err(|e| CliError::Keychain {
            source: None,
            message: format!("Failed to unlock collection: {}", e),
        })?;
    }

    // Prepare search attributes (only service and application)
    let mut attributes = HashMap::new();
    attributes.insert("service", service);
    attributes.insert("application", "orcher-cli");

    // Search for matching items
    let items = collection
        .search_items(attributes)
        .await
        .map_err(|e| CliError::Keychain {
            source: None,
            message: format!("Failed to search items in Secret Service: {}", e),
        })?;

    // Extract account names from items
    let mut accounts = Vec::new();
    for item in items {
        let item_attributes = item
            .get_attributes()
            .await
            .map_err(|e| CliError::Keychain {
                source: None,
                message: format!("Failed to get item attributes: {}", e),
            })?;

        if let Some(account) = item_attributes.get("account") {
            accounts.push(account.clone());
        }
    }

    // Remove duplicates and sort
    accounts.sort();
    accounts.dedup();

    Ok(accounts)
}

/// Check if Secret Service is reachable.
pub async fn is_secret_service_available() -> bool {
    use secret_service::{EncryptionType, SecretService};

    SecretService::connect(EncryptionType::Dh).await.is_ok()
}

/// Return a human-readable Secret Service status string.
pub async fn get_secret_service_status() -> Result<String> {
    use secret_service::{EncryptionType, SecretService};

    let ss = SecretService::connect(EncryptionType::Dh)
        .await
        .map_err(|e| CliError::Keychain {
            source: None,
            message: format!("Failed to connect to Secret Service: {}", e),
        })?;

    // Try to get collections to verify functionality
    let collections = ss
        .get_all_collections()
        .await
        .map_err(|e| CliError::Keychain {
            source: None,
            message: format!("Failed to get collections: {}", e),
        })?;

    let collection_count = collections.len();
    let default_collection = ss.get_default_collection().await;

    let status = match default_collection {
        Ok(collection) => {
            let locked = collection.is_locked().await.unwrap_or(true);
            format!(
                "Secret Service connected - {} collections, default collection {}",
                collection_count,
                if locked { "locked" } else { "unlocked" }
            )
        }
        Err(_) => format!(
            "Secret Service connected - {} collections, no default collection",
            collection_count
        ),
    };

    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_secret_service_availability() {
        // This test only checks if the function runs without panicking
        // The actual result depends on the system configuration
        let _ = is_secret_service_available().await;
    }

    #[tokio::test]
    async fn test_credential_operations() {
        // Only run this test if Secret Service is available
        if !is_secret_service_available().await {
            return;
        }

        let service = "orcher-cli-test";
        let account = "test-account";
        let test_data = b"test-credential-data";

        // Clean up any existing test data
        let _ = delete_credential(service, account).await;

        // Test store
        let result = store_credential(service, account, test_data).await;
        if result.is_err() {
            // Skip test if we can't access the keyring (e.g., in headless environment)
            return;
        }

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

        // Test list
        let accounts = list_accounts(service).await;
        assert!(accounts.is_ok(), "Failed to list accounts: {:?}", accounts);
        assert!(
            accounts.unwrap().contains(&account.to_string()),
            "Account not found in list"
        );

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

    #[tokio::test]
    async fn test_secret_service_status() {
        if is_secret_service_available().await {
            let status = get_secret_service_status().await;
            assert!(
                status.is_ok(),
                "Failed to get Secret Service status: {:?}",
                status
            );
        }
    }
}
