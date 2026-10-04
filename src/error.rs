//! The errors a command can end with. Each one's message is written for the
//! person at the terminal: `main` prints it after `Error:` and exits with 1.

use thiserror::Error;

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug, Error)]
pub enum Error {
    /// The engine could not be reached at all.
    #[error("cannot reach the engine at {address}: {reason}\n  Is it running? Start a local one with `orcher dev start`, or point at another with --server.")]
    Connect { address: String, reason: String },

    /// The engine answered, but with an error.
    #[error("{}", api_message(.rpc, .code, .message))]
    Api {
        rpc: &'static str,
        code: tonic::Code,
        message: String,
    },

    /// The arguments make no sense together, or a value is malformed.
    #[error("{0}")]
    InvalidInput(String),

    /// A local operation failed: reading a file, running Docker, and so on.
    #[error("{0}")]
    Local(String),
}

impl Error {
    pub fn invalid_input(message: impl Into<String>) -> Self {
        Self::InvalidInput(message.into())
    }

    pub fn local(message: impl Into<String>) -> Self {
        Self::Local(message.into())
    }

    /// Wraps a gRPC status from `rpc` (for example `WorkflowService.StartWorkflow`).
    pub fn api(rpc: &'static str, status: tonic::Status) -> Self {
        Self::Api {
            rpc,
            code: status.code(),
            message: status.message().to_string(),
        }
    }
}

fn api_message(rpc: &str, code: &tonic::Code, message: &str) -> String {
    let hint = match code {
        tonic::Code::Unauthenticated => {
            "\n  The engine requires an API key: pass --api-key or set ORCHER_API_KEY."
        }
        tonic::Code::PermissionDenied => "\n  The API key in use is not allowed to do this.",
        tonic::Code::Unimplemented => "\n  This engine does not support the operation.",
        _ => "",
    };
    let message = if message.is_empty() {
        code.description()
    } else {
        message
    };
    format!("{rpc}: {message}{hint}")
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Self::Local(err.to_string())
    }
}

impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Self {
        Self::Local(format!("JSON: {err}"))
    }
}

impl From<serde_yaml::Error> for Error {
    fn from(err: serde_yaml::Error) -> Self {
        Self::Local(format!("YAML: {err}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unauthenticated_call_says_how_to_authenticate() {
        let err = Error::api(
            "QueryService.ListWorkflows",
            tonic::Status::unauthenticated("missing authorization header"),
        );
        let text = err.to_string();
        assert!(text.starts_with("QueryService.ListWorkflows: missing authorization header"));
        assert!(text.contains("ORCHER_API_KEY"));
    }

    #[test]
    fn an_empty_status_message_falls_back_to_the_code() {
        let err = Error::api(
            "WorkflowService.CancelWorkflow",
            tonic::Status::not_found(""),
        );
        assert!(err.to_string().contains("not found"), "{err}");
    }
}
