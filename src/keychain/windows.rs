//! Windows credential storage via Credential Manager API.

use crate::error::{CliError, Result};
use std::collections::HashMap;
use std::ffi::OsString;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use windows::core::PWSTR;
use windows::Win32::Security::Credentials::{
    CredDeleteW, CredEnumerateW, CredReadW, CredWriteW, CREDENTIALW, CREDENTIAL_ATTRIBUTEW,
    CRED_ENUMERATE_ALL_CREDENTIALS, CRED_MAX_CREDENTIAL_BLOB_SIZE, CRED_PERSIST_LOCAL_MACHINE,
    CRED_TYPE_GENERIC,
};

/// Store a credential.
pub async fn store_credential(service: &str, account: &str, data: &[u8]) -> Result<()> {
    tokio::task::spawn_blocking({
        let service = service.to_string();
        let account = account.to_string();
        let data = data.to_vec();

        move || {
            if data.len() > CRED_MAX_CREDENTIAL_BLOB_SIZE as usize {
                return Err(CliError::Keychain {
                    source: None,
                    message: format!(
                        "Credential data too large: {} bytes (max: {} bytes)",
                        data.len(),
                        CRED_MAX_CREDENTIAL_BLOB_SIZE
                    ),
                });
            }

            // Create the target name (service:account format)
            let target_name = format!("{}:{}", service, account);
            let target_name_wide = to_wide_string(&target_name);

            // Create the comment
            let comment = format!("ORCHER CLI credential for {}", account);
            let comment_wide = to_wide_string(&comment);

            // Create credential structure
            let mut credential = CREDENTIALW {
                Flags: 0,
                Type: CRED_TYPE_GENERIC,
                TargetName: PWSTR(target_name_wide.as_ptr() as *mut u16),
                Comment: PWSTR(comment_wide.as_ptr() as *mut u16),
                LastWritten: Default::default(),
                CredentialBlobSize: data.len() as u32,
                CredentialBlob: data.as_ptr() as *mut u8,
                Persist: CRED_PERSIST_LOCAL_MACHINE,
                AttributeCount: 0,
                Attributes: std::ptr::null_mut(),
                TargetAlias: PWSTR::null(),
                UserName: PWSTR::null(),
            };

            unsafe {
                CredWriteW(&credential, 0).map_err(|e| CliError::Keychain {
                    source: None,
                    message: format!(
                        "Failed to store credential in Windows Credential Manager: {}",
                        e
                    ),
                })
            }
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
            let target_name = format!("{}:{}", service, account);
            let target_name_wide = to_wide_string(&target_name);

            let mut credential_ptr: *mut CREDENTIALW = std::ptr::null_mut();

            unsafe {
                match CredReadW(
                    PWSTR(target_name_wide.as_ptr() as *mut u16),
                    CRED_TYPE_GENERIC,
                    0,
                    &mut credential_ptr,
                ) {
                    Ok(_) => {
                        if credential_ptr.is_null() {
                            return Ok(None);
                        }

                        let credential = &*credential_ptr;
                        let blob_size = credential.CredentialBlobSize as usize;

                        if blob_size == 0 {
                            windows::Win32::Security::Credentials::CredFree(
                                credential_ptr as *const std::ffi::c_void,
                            );
                            return Ok(Some(Vec::new()));
                        }

                        let blob_ptr = credential.CredentialBlob;
                        let data = std::slice::from_raw_parts(blob_ptr, blob_size).to_vec();

                        // Free the credential structure
                        windows::Win32::Security::Credentials::CredFree(
                            credential_ptr as *const std::ffi::c_void,
                        );

                        Ok(Some(data))
                    }
                    Err(e) => {
                        let error_code = e.code().0;
                        // ERROR_NOT_FOUND = 0x80070490
                        if error_code == 0x80070490_u32 as i32 {
                            Ok(None)
                        } else {
                            Err(CliError::Keychain { source: None,
                                message: format!(
                                    "Failed to retrieve credential from Windows Credential Manager: {}",
                                    e
                                ),

                            })
                        }
                    }
                }
            }
        }
    })
    .await
    .map_err(|e| CliError::Keychain { source: None,
        message: format!("Task join error: {}", e),

    })?
}

/// Delete a credential. No-op if not found.
pub async fn delete_credential(service: &str, account: &str) -> Result<()> {
    tokio::task::spawn_blocking({
        let service = service.to_string();
        let account = account.to_string();

        move || {
            let target_name = format!("{}:{}", service, account);
            let target_name_wide = to_wide_string(&target_name);

            unsafe {
                match CredDeleteW(
                    PWSTR(target_name_wide.as_ptr() as *mut u16),
                    CRED_TYPE_GENERIC,
                    0,
                ) {
                    Ok(_) => Ok(()),
                    Err(e) => {
                        let error_code = e.code().0;
                        // ERROR_NOT_FOUND = 0x80070490 - acceptable for delete operations
                        if error_code == 0x80070490_u32 as i32 {
                            Ok(())
                        } else {
                            Err(CliError::Keychain { source: None,
                                message: format!(
                                    "Failed to delete credential from Windows Credential Manager: {}",
                                    e
                                ),

                            })
                        }
                    }
                }
            }
        }
    })
    .await
    .map_err(|e| CliError::Keychain { source: None,
        message: format!("Task join error: {}", e),

    })?
}

/// List all accounts for a service.
pub async fn list_accounts(service: &str) -> Result<Vec<String>> {
    tokio::task::spawn_blocking({
        let service = service.to_string();

        move || {
            let mut credential_count = 0u32;
            let mut credentials_ptr: *mut *mut CREDENTIALW = std::ptr::null_mut();

            unsafe {
                match CredEnumerateW(
                    PWSTR::null(),
                    CRED_ENUMERATE_ALL_CREDENTIALS,
                    &mut credential_count,
                    &mut credentials_ptr,
                ) {
                    Ok(_) => {
                        if credentials_ptr.is_null() || credential_count == 0 {
                            return Ok(Vec::new());
                        }

                        let mut accounts = Vec::new();
                        let service_prefix = format!("{}:", service);

                        for i in 0..credential_count {
                            let credential_ptr = *credentials_ptr.add(i as usize);
                            if credential_ptr.is_null() {
                                continue;
                            }

                            let credential = &*credential_ptr;

                            // Check if this credential belongs to our service
                            if credential.TargetName.is_null() {
                                continue;
                            }

                            let target_name = from_wide_string(credential.TargetName);
                            if target_name.starts_with(&service_prefix) {
                                // Extract account name (everything after "service:")
                                if let Some(account) = target_name.strip_prefix(&service_prefix) {
                                    accounts.push(account.to_string());
                                }
                            }
                        }

                        // Free the credentials array
                        windows::Win32::Security::Credentials::CredFree(
                            credentials_ptr as *const std::ffi::c_void,
                        );

                        // Remove duplicates and sort
                        accounts.sort();
                        accounts.dedup();

                        Ok(accounts)
                    }
                    Err(e) => {
                        let error_code = e.code().0;
                        // ERROR_NOT_FOUND = 0x80070490 - no credentials found
                        if error_code == 0x80070490_u32 as i32 {
                            Ok(Vec::new())
                        } else {
                            Err(CliError::Keychain { source: None,
                                message: format!(
                                    "Failed to enumerate credentials in Windows Credential Manager: {}",
                                    e
                                ),

                            })
                        }
                    }
                }
            }
        }
    })
    .await
    .map_err(|e| CliError::Keychain { source: None,
        message: format!("Task join error: {}", e),

    })?
}

/// Check if Credential Manager is accessible.
pub fn is_credential_manager_available() -> bool {
    // Windows Credential Manager is always available on Windows systems
    // We can test this by trying to enumerate credentials
    let mut credential_count = 0u32;
    let mut credentials_ptr: *mut *mut CREDENTIALW = std::ptr::null_mut();

    unsafe {
        match CredEnumerateW(
            PWSTR::null(),
            CRED_ENUMERATE_ALL_CREDENTIALS,
            &mut credential_count,
            &mut credentials_ptr,
        ) {
            Ok(_) => {
                if !credentials_ptr.is_null() {
                    windows::Win32::Security::Credentials::CredFree(
                        credentials_ptr as *const std::ffi::c_void,
                    );
                }
                true
            }
            Err(_) => false,
        }
    }
}

/// Return a human-readable Credential Manager status string.
pub fn get_credential_manager_status() -> Result<String> {
    let mut credential_count = 0u32;
    let mut credentials_ptr: *mut *mut CREDENTIALW = std::ptr::null_mut();

    unsafe {
        match CredEnumerateW(
            PWSTR::null(),
            CRED_ENUMERATE_ALL_CREDENTIALS,
            &mut credential_count,
            &mut credentials_ptr,
        ) {
            Ok(_) => {
                if !credentials_ptr.is_null() {
                    windows::Win32::Security::Credentials::CredFree(
                        credentials_ptr as *const std::ffi::c_void,
                    );
                }
                Ok(format!(
                    "Windows Credential Manager - Available ({} total credentials)",
                    credential_count
                ))
            }
            Err(e) => Ok(format!(
                "Windows Credential Manager - Available (enumeration failed: {})",
                e
            )),
        }
    }
}

fn to_wide_string(s: &str) -> Vec<u16> {
    OsString::from(s).encode_wide().chain(Some(0)).collect()
}

fn from_wide_string(pwstr: PWSTR) -> String {
    if pwstr.is_null() {
        return String::new();
    }

    unsafe {
        let mut len = 0;
        let mut ptr = pwstr.0;

        // Find the length of the null-terminated wide string
        while *ptr != 0 {
            len += 1;
            ptr = ptr.add(1);
        }

        if len == 0 {
            return String::new();
        }

        // Convert to Rust string
        let slice = std::slice::from_raw_parts(pwstr.0, len);
        String::from_utf16_lossy(slice)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_credential_manager_availability() {
        // This test only checks if the function runs without panicking
        let available = is_credential_manager_available();
        println!("Credential Manager available: {}", available);
    }

    #[tokio::test]
    async fn test_credential_operations() {
        // Only run this test if Credential Manager is available
        if !is_credential_manager_available() {
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

    #[test]
    fn test_credential_manager_status() {
        if is_credential_manager_available() {
            let status = get_credential_manager_status();
            assert!(
                status.is_ok(),
                "Failed to get Credential Manager status: {:?}",
                status
            );
        }
    }

    #[test]
    fn test_wide_string_conversion() {
        let test_string = "Hello, World! 🌍";
        let wide = to_wide_string(test_string);

        // The wide string should be null-terminated
        assert_eq!(wide[wide.len() - 1], 0);

        // Test conversion back (simulating PWSTR)
        let pwstr = PWSTR(wide.as_ptr() as *mut u16);
        let converted_back = from_wide_string(pwstr);
        assert_eq!(converted_back, test_string);
    }

    #[test]
    fn test_empty_string_conversion() {
        let empty = "";
        let wide = to_wide_string(empty);
        assert_eq!(wide, vec![0]);

        let pwstr = PWSTR(wide.as_ptr() as *mut u16);
        let converted_back = from_wide_string(pwstr);
        assert_eq!(converted_back, empty);
    }
}
