//! Implementation of the 'auth' command for managing authentication

use crate::client::auth::{AuthManager, LoginRequest};
use crate::client::config::ClientConfig;
use crate::client::device_flow::{DeviceFlowAuthenticator, DeviceTokenResponse};
use crate::client::{AuthProvider, OrcherClient};
use crate::config::{AuthConfig, AuthType, Config, Context};
use crate::error::{CliError, Result};

use crate::types::AuthCommands;
use crate::utils::GlobalConfig;
use dialoguer::{Input, Password};
use indicatif::{ProgressBar, ProgressStyle};
use std::time::Duration;
use url::Url;

pub async fn execute(action: AuthCommands, global_config: &GlobalConfig) -> Result<()> {
    match action {
        AuthCommands::Login {
            server,
            token,
            device,
            provider,
            username,
        } => {
            login(
                server.as_deref(),
                token,
                device,
                provider.as_deref(),
                username.as_deref(),
                global_config,
            )
            .await
        }
        AuthCommands::Logout => logout(global_config).await,
        AuthCommands::Status => status(global_config).await,
    }
}

/// Perform login authentication
async fn login(
    server: Option<&str>,
    use_token: bool,
    use_device: bool,
    provider: Option<&str>,
    username: Option<&str>,
    global_config: &GlobalConfig,
) -> Result<()> {
    let config = load_or_create_config()?;

    // Determine server URL
    let server_url = if let Some(server) = server {
        Url::parse(server).map_err(|e| CliError::Config {
            message: format!("Invalid server URL '{}': {}", server, e),
            source: Some(Box::new(e)),
        })?
    } else {
        let current_context = config
            .contexts
            .get(&config.current_context)
            .ok_or_else(|| CliError::NoCurrentContext)?;
        Url::parse(&current_context.server).map_err(|e| CliError::Config {
            message: format!("Invalid server URL in context: {}", e),
            source: Some(Box::new(e)),
        })?
    };

    if use_device {
        // Device flow authentication (for CLI/headless)
        let provider = provider.unwrap_or("github");
        login_with_device_flow(&server_url, provider, global_config).await
    } else if use_token {
        // Token-based authentication
        login_with_token(&server_url, global_config).await
    } else {
        // Username/password authentication
        login_with_credentials(&server_url, username, global_config).await
    }
}

/// Login with device flow (OAuth 2.0 Device Authorization Grant)
async fn login_with_device_flow(
    server_url: &Url,
    provider: &str,
    _global_config: &GlobalConfig,
) -> Result<()> {
    println!("🔐 Authenticating with {} using device flow...\n", provider);

    let authenticator = DeviceFlowAuthenticator::new(server_url.clone());

    // Request device code
    let device_response = authenticator
        .request_device_code(provider)
        .await
        .map_err(|e| CliError::Auth {
            message: format!("Failed to initiate device flow: {}", e),
            error_code: None,
        })?;

    let device_code = device_response.device_code.clone();

    // Display instructions to user
    println!("📋 Please visit the following URL in your browser:");
    println!("   {}\n", device_response.verification_uri);
    println!("🔢 Enter this code:");
    println!("   {}\n", device_response.user_code);

    // Open browser automatically if possible
    if let Some(complete_uri) = &device_response.verification_uri_complete {
        if open::that(complete_uri).is_ok() {
            println!("✓ Browser opened automatically\n");
        }
    }

    println!("⏱  Code expires in {} seconds", device_response.expires_in);
    println!("⏳ Waiting for authorization...\n");

    // Poll for authorization with progress bar
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .unwrap()
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
    );
    spinner.set_message("Waiting for authorization...");
    spinner.enable_steady_tick(Duration::from_millis(100));

    let mut interval = device_response.interval.max(5) as u64;
    let mut attempts = 0;
    const MAX_ATTEMPTS: usize = 120;

    loop {
        if attempts >= MAX_ATTEMPTS {
            spinner.finish_with_message("❌ Timeout waiting for authorization");
            return Err(CliError::Timeout {
                operation: "device authorization".to_string(),
                duration: Duration::from_secs(interval * MAX_ATTEMPTS as u64),
            });
        }

        attempts += 1;

        // Wait before polling (except first attempt)
        if attempts > 1 {
            tokio::time::sleep(Duration::from_secs(interval)).await;
        }

        match authenticator.poll_once(provider, &device_code).await {
            Ok(DeviceTokenResponse::Success {
                access_token,
                refresh_token,
                user,
                ..
            }) => {
                spinner.finish_with_message("✓ Authorization successful!");
                println!("\n✓ Logged in as: {}", user.email);
                if let Some(name) = &user.name {
                    println!("  Name: {}", name);
                }
                println!("  Provider: {}", user.oauth_provider);

                // Save tokens to secure storage and update configuration
                save_device_tokens_to_config(
                    &access_token,
                    &refresh_token,
                    &user.email,
                    server_url,
                )?;

                return Ok(());
            }
            Ok(DeviceTokenResponse::Pending { .. }) => {
                // Continue polling
                continue;
            }
            Ok(DeviceTokenResponse::SlowDown {
                interval: new_interval,
            }) => {
                interval = new_interval.max(interval as i32) as u64;
                spinner.set_message(format!("Slowing down polling to {} seconds...", interval));
                continue;
            }
            Ok(DeviceTokenResponse::Denied) => {
                spinner.finish_with_message("❌ Authorization denied");
                return Err(CliError::Auth {
                    message: "Authorization was denied by the user".to_string(),
                    error_code: Some("access_denied".to_string()),
                });
            }
            Ok(DeviceTokenResponse::Expired) => {
                spinner.finish_with_message("❌ Code expired");
                return Err(CliError::Auth {
                    message: "Device code has expired. Please try again.".to_string(),
                    error_code: Some("expired_token".to_string()),
                });
            }
            Err(e) => {
                spinner.finish_with_message("❌ Error during polling");
                return Err(e);
            }
        }
    }
}

/// Login with token authentication
async fn login_with_token(server_url: &Url, _global_config: &GlobalConfig) -> Result<()> {
    let token: String = Input::new()
        .with_prompt("Enter authentication token")
        .interact_text()
        .map_err(|e| CliError::Config {
            message: format!("Failed to read token: {}", e),
            source: Some(Box::new(e)),
        })?;

    // Validate token by making a test request
    let client_config = ClientConfig::new(server_url.clone());
    let client = OrcherClient::new(client_config)?;

    let auth = AuthProvider::Token(crate::client::auth::TokenAuth::new(
        token.clone(),
        None, // No refresh token
        None, // No expiration
        server_url.clone(),
    ));

    let client = client.with_auth(auth);

    // Test the authentication
    match client.health_check().await {
        Ok(_) => {
            println!("✓ Authentication successful");

            // Save token to configuration
            save_token_to_config(&token, server_url)?;
            Ok(())
        }
        Err(e) => Err(CliError::Auth {
            message: format!("Authentication failed: {}", e),
            error_code: None,
        }),
    }
}

/// Login with username and password
async fn login_with_credentials(
    server_url: &Url,
    username: Option<&str>,
    _global_config: &GlobalConfig,
) -> Result<()> {
    let username = if let Some(user) = username {
        user.to_string()
    } else {
        Input::new()
            .with_prompt("Username")
            .interact_text()
            .map_err(|e| CliError::Config {
                message: format!("Failed to read username: {}", e),
                source: Some(Box::new(e)),
            })?
    };

    let password = Password::new()
        .with_prompt("Password")
        .interact()
        .map_err(|e| CliError::Config {
            message: format!("Failed to read password: {}", e),
            source: Some(Box::new(e)),
        })?;

    let auth_manager = AuthManager::new();
    let login_request = LoginRequest {
        username: username.clone(),
        password,
        scopes: vec!["read".to_string(), "write".to_string()],
    };

    match auth_manager.login(server_url, login_request).await {
        Ok(token_auth) => {
            println!("✓ Login successful for user: {}", username);

            // Save authentication to configuration
            save_credentials_to_config(&token_auth, server_url)?;
            Ok(())
        }
        Err(e) => Err(CliError::Auth {
            message: format!("Login failed: {}", e),
            error_code: None,
        }),
    }
}

/// Logout and clear stored authentication
async fn logout(_global_config: &GlobalConfig) -> Result<()> {
    let mut config = load_or_create_config()?;

    // Get current context
    let current_context_name = &config.current_context.clone();
    if let Some(context) = config.contexts.get_mut(current_context_name) {
        // Clear authentication information
        context.auth = AuthConfig {
            auth_type: AuthType::None,
            token_ref: None,
            apikey_ref: None,
            username: None,
        };

        config.save()?;

        println!("✓ Logged out from context '{}'", current_context_name);
    } else {
        return Err(CliError::NoCurrentContext);
    }

    // Clear credentials from secure storage
    let storage = crate::secure_storage::SecureStorage::new(&config.current_context)?;
    if let Err(e) = storage.delete_all() {
        eprintln!(
            "Warning: Failed to clear credentials from secure storage: {}",
            e
        );
    }

    Ok(())
}

/// Display current authentication status
async fn status(_global_config: &GlobalConfig) -> Result<()> {
    let config = load_or_create_config()?;

    let current_context = config
        .contexts
        .get(&config.current_context)
        .ok_or_else(|| CliError::NoCurrentContext)?;

    println!("Authentication Status");
    println!("=====================\n");

    println!("Current context: {}", config.current_context);
    println!("Server: {}", current_context.server);
    println!();

    // Check environment variable (highest priority)
    let env_token = std::env::var("ORCHER_TOKEN").ok();

    if env_token.is_some() {
        println!("Environment:");
        println!("  ORCHER_TOKEN: [set] (takes priority)");
        println!();
    }

    // Check secure storage
    let has_stored_token =
        if let Ok(storage) = crate::secure_storage::SecureStorage::new(&config.current_context) {
            let has_token = storage.get_token().ok().flatten().is_some();
            if has_token {
                println!(
                    "Secure Storage: Token stored for context '{}'",
                    config.current_context
                );
                println!();
            }
            has_token
        } else {
            false
        };

    println!("Context Configuration:");
    match &current_context.auth.auth_type {
        AuthType::None => {
            println!("  Type: None (not logged in)");
        }
        AuthType::Token => {
            println!("  Type: Token");
            if let Some(token_ref) = &current_context.auth.token_ref {
                println!("  Token reference: {}", token_ref);
            }
        }
        AuthType::ApiKey => {
            // API keys are for services only, show note
            println!("  Type: API Key (note: API keys are for services, use 'orcher auth login' for CLI)");
        }
        AuthType::Basic => {
            println!("  Type: Basic");
            if let Some(username) = &current_context.auth.username {
                println!("  Username: {}", username);
            }
        }
    }

    // Determine effective authentication
    println!();
    let effective_auth = if env_token.is_some() {
        "ORCHER_TOKEN environment variable"
    } else if has_stored_token {
        "Token from secure storage"
    } else {
        "None"
    };
    println!("Effective authentication: {}", effective_auth);

    // Try to get user info from server if authenticated
    if env_token.is_some() || has_stored_token {
        match get_user_info_from_server(current_context).await {
            Ok(user_info) => {
                println!("User: {}", user_info);
            }
            Err(e) => {
                println!("Note: Could not retrieve user information: {}", e);
            }
        }
    }

    Ok(())
}

/// Save device flow tokens to configuration and secure storage
fn save_device_tokens_to_config(
    access_token: &str,
    refresh_token: &str,
    username: &str,
    server_url: &Url,
) -> Result<()> {
    let mut config = load_or_create_config()?;

    // Find or create context for this server
    let context_name = find_or_create_context_for_server(&mut config, server_url)?;

    if let Some(context) = config.contexts.get_mut(&context_name) {
        context.auth = AuthConfig {
            auth_type: AuthType::Token,
            token_ref: Some(context_name.clone()),
            apikey_ref: None,
            username: Some(username.to_string()),
        };
    }

    config.save()?;

    // Store tokens securely in keychain
    let storage = crate::secure_storage::SecureStorage::new(&context_name)?;
    storage.store_token(access_token)?;
    storage.store_refresh_token(refresh_token)?;

    Ok(())
}

/// Save token authentication to configuration and secure storage
fn save_token_to_config(token: &str, server_url: &Url) -> Result<()> {
    let mut config = load_or_create_config()?;

    // Find or create context for this server
    let context_name = find_or_create_context_for_server(&mut config, server_url)?;

    if let Some(context) = config.contexts.get_mut(&context_name) {
        context.auth = AuthConfig {
            auth_type: AuthType::Token,
            token_ref: Some(context_name.clone()),
            apikey_ref: None,
            username: None,
        };
    }

    config.save()?;

    // Store token securely in keychain
    let storage = crate::secure_storage::SecureStorage::new(&context_name)?;
    storage.store_token(token)?;

    Ok(())
}

/// Save credentials to configuration and secure storage
fn save_credentials_to_config(
    token_auth: &crate::client::auth::TokenAuth,
    server_url: &Url,
) -> Result<()> {
    let mut config = load_or_create_config()?;

    // Find or create context for this server
    let context_name = find_or_create_context_for_server(&mut config, server_url)?;

    if let Some(context) = config.contexts.get_mut(&context_name) {
        context.auth = AuthConfig {
            auth_type: AuthType::Token,
            token_ref: Some(context_name.clone()),
            apikey_ref: None,
            username: None,
        };
    }

    config.save()?;

    // Store tokens securely in keychain
    let storage = crate::secure_storage::SecureStorage::new(&context_name)?;
    storage.store_token(&token_auth.access_token)?;
    if let Some(ref refresh_token) = token_auth.refresh_token {
        storage.store_refresh_token(refresh_token)?;
    }

    Ok(())
}

/// Find existing context for server or create a new one
fn find_or_create_context_for_server(config: &mut Config, server_url: &Url) -> Result<String> {
    // Normalize the incoming URL (remove trailing slash for comparison)
    let normalized_url = server_url.as_str().trim_end_matches('/');

    // Try to find existing context with same server
    for (name, context) in &config.contexts {
        let normalized_context_server = context.server.trim_end_matches('/');
        if normalized_context_server == normalized_url {
            return Ok(name.clone());
        }
    }

    // Create new context
    let context_name = format!("auto-{}", server_url.host_str().unwrap_or("unknown"));
    let context = Context {
        server: server_url.to_string(),
        api: None,
        auth: AuthConfig {
            auth_type: AuthType::None,
            token_ref: None,
            apikey_ref: None,
            username: None,
        },
        tls: None,
        timeout: None,
        namespace: None,
    };

    config.contexts.insert(context_name.clone(), context);
    Ok(context_name)
}

/// Get user information from server
async fn get_user_info_from_server(context: &Context) -> Result<String> {
    // Check if username is cached in config
    if let Some(username) = &context.auth.username {
        return Ok(username.clone());
    }

    // Try to get token from secure storage
    if let Some(token_ref) = &context.auth.token_ref {
        let storage = crate::secure_storage::SecureStorage::new(token_ref)?;
        if let Some(token) = storage.get_token()? {
            return fetch_user_info_from_api(context, &token).await;
        }
    }

    Ok("Unknown user".to_string())
}

/// Fetch user information from API
async fn fetch_user_info_from_api(context: &Context, token: &str) -> Result<String> {
    let client = reqwest::Client::new();
    let url = format!("{}/api/v1/auth/me", context.server.trim_end_matches('/'));

    match client
        .get(&url)
        .header("Authorization", format!("Bearer {}", token))
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
    {
        Ok(response) => {
            if response.status().is_success() {
                if let Ok(user_info) = response.json::<serde_json::Value>().await {
                    if let Some(email) = user_info.get("email").and_then(|e| e.as_str()) {
                        // Cache the email in config
                        let mut config = load_or_create_config()?;
                        if let Some(context_mut) = config.contexts.get_mut(&config.current_context)
                        {
                            context_mut.auth.username = Some(email.to_string());
                            let _ = config.save();
                        }
                        return Ok(email.to_string());
                    }
                }
            }
            Ok("Unknown user".to_string())
        }
        Err(_) => {
            // API call failed, return placeholder
            Ok("Unknown user".to_string())
        }
    }
}

/// Load existing configuration or create a default one
fn load_or_create_config() -> Result<Config> {
    match Config::load() {
        Ok(config) => Ok(config),
        Err(_) => {
            let config = Config::default();
            config.save()?;
            Ok(config)
        }
    }
}
