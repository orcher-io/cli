//! CLI error types (`CliError`, `ValidationError`).

#![allow(dead_code)]

use std::fmt;
use thiserror::Error;

/// Result type alias for CLI operations
pub type Result<T> = std::result::Result<T, CliError>;

/// Comprehensive error types for CLI operations
#[derive(Error, Debug)]
pub enum CliError {
    /// Configuration-related errors
    #[error("Configuration error: {message}")]
    Config {
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// Authentication and authorization errors
    #[error("Authentication error: {message}")]
    Auth {
        message: String,
        error_code: Option<String>,
    },

    /// API communication errors
    #[error("API error ({status}): {message}")]
    Api {
        status: u16,
        message: String,
        endpoint: Option<String>,
    },

    /// Network connectivity errors
    #[error("Network error: {message}")]
    Network {
        message: String,
        #[source]
        source: Option<reqwest::Error>,
    },

    /// Workflow validation errors (with structured errors)
    #[error("Validation error: {message}")]
    ValidationStructured {
        message: String,
        errors: Vec<ValidationError>,
    },

    /// Workflow validation errors (with simple string errors)
    #[error("Validation error: {message}")]
    Validation {
        message: String,
        errors: Vec<String>,
    },

    /// File system and I/O errors
    #[error("I/O error: {message}")]
    Io {
        message: String,
        path: Option<String>,
        #[source]
        source: std::io::Error,
    },

    /// Template processing errors
    #[error("Template error: {message}")]
    Template {
        message: String,
        template_name: Option<String>,
    },

    /// Resource not found errors
    #[error("Resource not found: {resource_type} '{name}' not found")]
    NotFound {
        resource_type: String,
        name: String,
        namespace: Option<String>,
    },

    /// Resource already exists errors
    #[error("Resource already exists: {resource_type} '{name}' already exists")]
    AlreadyExists {
        resource_type: String,
        name: String,
        namespace: Option<String>,
    },

    /// Serialization/deserialization errors
    #[error("Serialization error: {message}")]
    Serialization {
        message: String,
        format: String,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// User input errors
    #[error("Invalid input: {message}")]
    InvalidInput {
        message: String,
        field: Option<String>,
        expected: Option<String>,
    },

    /// Command execution errors
    #[error("Command failed: {command}")]
    CommandFailed {
        command: String,
        exit_code: Option<i32>,
        stderr: Option<String>,
    },

    /// Timeout errors
    #[error("Operation timed out: {operation}")]
    Timeout {
        operation: String,
        duration: std::time::Duration,
    },

    /// Permission errors
    #[error("Permission denied: {message}")]
    Permission {
        message: String,
        path: Option<String>,
    },

    /// Keychain/credential storage errors
    #[error("Credential storage error: {message}")]
    Keychain {
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// Generic internal errors
    #[error("Internal error: {message}")]
    Internal {
        message: String,
        context: Vec<String>,
    },

    /// Specific authentication states
    #[error("Authentication token has expired")]
    AuthenticationExpired,

    #[error("No valid authentication found")]
    Unauthorized,

    #[error("No authentication configured")]
    NoAuthentication,

    #[error("Token refresh not supported for this authentication type")]
    RefreshNotSupported,

    /// Feature not yet implemented
    #[error("Not implemented: {message}")]
    NotImplemented { message: String },

    #[error("No refresh token available")]
    NoRefreshToken,

    /// Context errors
    #[error("Context '{name}' not found")]
    ContextNotFound { name: String },

    #[error("No current context set")]
    NoCurrentContext,

    /// A workflow that ended without completing: `error_message` says how,
    /// as in "ended FAILED: the reason".
    #[error("Workflow '{workflow_id}' {error_message}")]
    WorkflowFailed {
        workflow_id: String,
        error_message: String,
    },

    /// I/O errors without source (for simple path-based errors)
    #[error("I/O error: {message}")]
    IO {
        message: String,
        path: Option<std::path::PathBuf>,
    },

    /// Parse errors for file formats
    #[error("Parse error: {message}")]
    Parse {
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// Execution errors during workflow/task execution
    #[error("Execution error: {message}")]
    Execution {
        message: String,
        execution_id: Option<String>,
    },

    /// Running the local engine in Docker failed. The message says what to do.
    #[error("{message}")]
    LocalEngine { message: String },

    /// Feature not supported errors
    #[error("Not supported: {feature}")]
    NotSupported {
        feature: String,
        suggestion: Option<String>,
    },
}

/// Detailed validation error information
#[derive(Debug, Clone)]
pub struct ValidationError {
    pub path: String,
    pub message: String,
    pub severity: ValidationSeverity,
    pub rule: Option<String>,
}

/// Severity levels for validation errors
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationSeverity {
    Error,
    Warning,
    Info,
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}: {}", self.severity, self.path, self.message)
    }
}

impl fmt::Display for ValidationSeverity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ValidationSeverity::Error => write!(f, "ERROR"),
            ValidationSeverity::Warning => write!(f, "WARN"),
            ValidationSeverity::Info => write!(f, "INFO"),
        }
    }
}

impl CliError {
    /// Create a configuration error
    pub fn config<S: Into<String>>(message: S) -> Self {
        Self::Config {
            message: message.into(),
            source: None,
        }
    }

    /// Create a configuration error with source
    pub fn config_with_source<S: Into<String>, E>(message: S, source: E) -> Self
    where
        E: std::error::Error + Send + Sync + 'static,
    {
        Self::Config {
            message: message.into(),
            source: Some(Box::new(source)),
        }
    }

    /// Create an authentication error
    pub fn auth<S: Into<String>>(message: S) -> Self {
        Self::Auth {
            message: message.into(),
            error_code: None,
        }
    }

    /// Create an authentication error with code
    pub fn auth_with_code<S: Into<String>, C: Into<String>>(message: S, code: C) -> Self {
        Self::Auth {
            message: message.into(),
            error_code: Some(code.into()),
        }
    }

    /// Create an API error
    pub fn api<S: Into<String>>(status: u16, message: S) -> Self {
        Self::Api {
            status,
            message: message.into(),
            endpoint: None,
        }
    }

    /// Create an API error with endpoint
    pub fn api_with_endpoint<S: Into<String>, E: Into<String>>(
        status: u16,
        message: S,
        endpoint: E,
    ) -> Self {
        Self::Api {
            status,
            message: message.into(),
            endpoint: Some(endpoint.into()),
        }
    }

    /// Create a network error
    pub fn network<S: Into<String>>(message: S) -> Self {
        Self::Network {
            message: message.into(),
            source: None,
        }
    }

    /// Create a network error with source
    pub fn network_with_source<S: Into<String>>(message: S, source: reqwest::Error) -> Self {
        Self::Network {
            message: message.into(),
            source: Some(source),
        }
    }

    /// Create an I/O error
    pub fn io<S: Into<String>>(message: S, source: std::io::Error) -> Self {
        Self::Io {
            message: message.into(),
            path: None,
            source,
        }
    }

    /// Create an I/O error with path
    pub fn io_with_path<S: Into<String>, P: Into<String>>(
        message: S,
        path: P,
        source: std::io::Error,
    ) -> Self {
        Self::Io {
            message: message.into(),
            path: Some(path.into()),
            source,
        }
    }

    /// Create a validation error with structured errors
    pub fn validation_structured<S: Into<String>>(
        message: S,
        errors: Vec<ValidationError>,
    ) -> Self {
        Self::ValidationStructured {
            message: message.into(),
            errors,
        }
    }

    /// Create a validation error with simple string errors
    pub fn validation<S: Into<String>>(message: S, errors: Vec<String>) -> Self {
        Self::Validation {
            message: message.into(),
            errors,
        }
    }

    /// Create a template error
    pub fn template<S: Into<String>>(message: S) -> Self {
        Self::Template {
            message: message.into(),
            template_name: None,
        }
    }

    /// Create a template error with template name
    pub fn template_with_name<S: Into<String>, T: Into<String>>(message: S, template: T) -> Self {
        Self::Template {
            message: message.into(),
            template_name: Some(template.into()),
        }
    }

    /// Create a not found error
    pub fn not_found<R: Into<String>, N: Into<String>>(resource_type: R, name: N) -> Self {
        Self::NotFound {
            resource_type: resource_type.into(),
            name: name.into(),
            namespace: None,
        }
    }

    /// Create a not found error with namespace
    pub fn not_found_with_namespace<R: Into<String>, N: Into<String>, NS: Into<String>>(
        resource_type: R,
        name: N,
        namespace: NS,
    ) -> Self {
        Self::NotFound {
            resource_type: resource_type.into(),
            name: name.into(),
            namespace: Some(namespace.into()),
        }
    }

    /// Create an invalid input error
    pub fn invalid_input<S: Into<String>>(message: S) -> Self {
        Self::InvalidInput {
            message: message.into(),
            field: None,
            expected: None,
        }
    }

    /// Create an invalid input error with field and expected value
    pub fn invalid_input_with_details<S: Into<String>, F: Into<String>, E: Into<String>>(
        message: S,
        field: F,
        expected: E,
    ) -> Self {
        Self::InvalidInput {
            message: message.into(),
            field: Some(field.into()),
            expected: Some(expected.into()),
        }
    }

    /// Create an internal error
    pub fn local_engine<S: Into<String>>(message: S) -> Self {
        Self::LocalEngine {
            message: message.into(),
        }
    }

    pub fn internal<S: Into<String>>(message: S) -> Self {
        Self::Internal {
            message: message.into(),
            context: Vec::new(),
        }
    }

    /// Create a not implemented error
    pub fn not_implemented<S: Into<String>>(message: S) -> Self {
        Self::NotImplemented {
            message: message.into(),
        }
    }

    /// Create an internal error with context
    pub fn internal_with_context<S: Into<String>>(message: S, context: Vec<String>) -> Self {
        Self::Internal {
            message: message.into(),
            context,
        }
    }

    /// Create an already exists error
    pub fn already_exists<R: Into<String>, N: Into<String>>(resource_type: R, name: N) -> Self {
        Self::AlreadyExists {
            resource_type: resource_type.into(),
            name: name.into(),
            namespace: None,
        }
    }

    /// Create an already exists error with namespace
    pub fn already_exists_with_namespace<R: Into<String>, N: Into<String>, NS: Into<String>>(
        resource_type: R,
        name: N,
        namespace: NS,
    ) -> Self {
        Self::AlreadyExists {
            resource_type: resource_type.into(),
            name: name.into(),
            namespace: Some(namespace.into()),
        }
    }

    /// Create a serialization error
    pub fn serialization<S: Into<String>, F: Into<String>>(message: S, format: F) -> Self {
        let message_str = message.into();
        Self::Serialization {
            message: message_str.clone(),
            format: format.into(),
            source: Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                message_str,
            )),
        }
    }

    /// Create a command failed error
    pub fn command_failed<C: Into<String>>(
        command: C,
        exit_code: Option<i32>,
        stderr: Option<String>,
    ) -> Self {
        Self::CommandFailed {
            command: command.into(),
            exit_code,
            stderr,
        }
    }

    /// Check if error is retriable
    pub fn is_retriable(&self) -> bool {
        match self {
            CliError::Network { .. } => true,
            CliError::Api { status, .. } => *status >= 500,
            CliError::Timeout { .. } => true,
            CliError::AuthenticationExpired => true,
            _ => false,
        }
    }

    /// Get error category for metrics/logging
    pub fn category(&self) -> &'static str {
        match self {
            CliError::Config { .. } => "config",
            CliError::Auth { .. }
            | CliError::AuthenticationExpired
            | CliError::Unauthorized
            | CliError::NoAuthentication
            | CliError::RefreshNotSupported
            | CliError::NoRefreshToken => "auth",
            CliError::Api { .. } => "api",
            CliError::Network { .. } => "network",
            CliError::Validation { .. } | CliError::ValidationStructured { .. } => "validation",
            CliError::Io { .. } | CliError::IO { .. } => "io",
            CliError::Template { .. } => "template",
            CliError::NotFound { .. } => "not_found",
            CliError::AlreadyExists { .. } => "already_exists",
            CliError::Serialization { .. } => "serialization",
            CliError::InvalidInput { .. } => "input",
            CliError::CommandFailed { .. } => "command",
            CliError::Timeout { .. } => "timeout",
            CliError::Permission { .. } => "permission",
            CliError::Keychain { .. } => "keychain",
            CliError::ContextNotFound { .. } | CliError::NoCurrentContext => "context",
            CliError::WorkflowFailed { .. } | CliError::Execution { .. } => "workflow",
            CliError::NotImplemented { .. } => "not_implemented",
            CliError::NotSupported { .. } => "not_supported",
            CliError::Parse { .. } => "parse",
            CliError::Internal { .. } => "internal",
            CliError::LocalEngine { .. } => "local_engine",
        }
    }
}

// Implement conversion from common error types
impl From<std::io::Error> for CliError {
    fn from(err: std::io::Error) -> Self {
        Self::io("I/O operation failed", err)
    }
}

impl From<serde_json::Error> for CliError {
    fn from(err: serde_json::Error) -> Self {
        Self::Serialization {
            message: err.to_string(),
            format: "JSON".to_string(),
            source: Box::new(err),
        }
    }
}

impl From<serde_yaml::Error> for CliError {
    fn from(err: serde_yaml::Error) -> Self {
        Self::Serialization {
            message: err.to_string(),
            format: "YAML".to_string(),
            source: Box::new(err),
        }
    }
}

impl From<reqwest::Error> for CliError {
    fn from(err: reqwest::Error) -> Self {
        if err.is_connect() || err.is_timeout() {
            Self::network_with_source("Network request failed", err)
        } else if let Some(status) = err.status() {
            Self::api(status.as_u16(), "API request failed")
        } else {
            Self::network_with_source("HTTP request failed", err)
        }
    }
}

impl From<url::ParseError> for CliError {
    fn from(err: url::ParseError) -> Self {
        Self::invalid_input(format!("Invalid URL: {}", err))
    }
}

impl From<handlebars::RenderError> for CliError {
    fn from(err: handlebars::RenderError) -> Self {
        Self::template(format!("Template rendering failed: {}", err))
    }
}

impl From<toml::de::Error> for CliError {
    fn from(err: toml::de::Error) -> Self {
        Self::Serialization {
            message: err.to_string(),
            format: "TOML".to_string(),
            source: Box::new(err),
        }
    }
}

impl From<toml::ser::Error> for CliError {
    fn from(err: toml::ser::Error) -> Self {
        Self::Serialization {
            message: err.to_string(),
            format: "TOML".to_string(),
            source: Box::new(err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_creation() {
        let err = CliError::config("Invalid configuration");
        assert_eq!(err.category(), "config");

        let err = CliError::not_found("workflow", "my-workflow");
        assert_eq!(err.category(), "not_found");

        let err = CliError::api(404, "Not found");
        assert!(!err.is_retriable());

        let err = CliError::api(500, "Server error");
        assert!(err.is_retriable());
    }

    #[test]
    fn test_validation_error() {
        let validation_errors = vec![ValidationError {
            path: "workflow.name".to_string(),
            message: "Name is required".to_string(),
            severity: ValidationSeverity::Error,
            rule: Some("required".to_string()),
        }];

        let err = CliError::validation_structured("Workflow validation failed", validation_errors);
        assert_eq!(err.category(), "validation");
    }

    #[test]
    fn test_error_conversion() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "File not found");
        let cli_err: CliError = io_err.into();
        assert_eq!(cli_err.category(), "io");
    }
}
