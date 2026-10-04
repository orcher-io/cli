//! Options shared by every command: where the engine is, which namespace to
//! act in, how to authenticate and how to print.

use clap::{Args, ValueEnum};

/// The engine a fresh `orcher dev start` listens on.
pub const DEFAULT_SERVER: &str = "http://localhost:50051";

#[derive(Args, Debug, Clone)]
pub struct GlobalArgs {
    /// Engine gRPC address
    #[arg(long, global = true, env = "ORCHER_SERVER", default_value = DEFAULT_SERVER)]
    pub server: String,

    /// Namespace to act in
    #[arg(
        short,
        long,
        global = true,
        env = "ORCHER_NAMESPACE",
        default_value = "default"
    )]
    pub namespace: String,

    /// API key, for an engine that requires one
    #[arg(long, global = true, env = "ORCHER_API_KEY", hide_env_values = true)]
    pub api_key: Option<String>,

    /// Output format
    #[arg(short, long, global = true, value_enum, default_value_t = Output::Table)]
    pub output: Output,

    /// Print only what was asked for: no hints or summaries
    #[arg(short, long, global = true)]
    pub quiet: bool,

    /// Disable colored output
    #[arg(long, global = true, env = "NO_COLOR", value_parser = clap::builder::FalseyValueParser::new())]
    pub no_color: bool,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Output {
    /// Human-readable tables and details
    Table,
    /// JSON, for scripts
    Json,
    /// YAML, for scripts
    Yaml,
    /// Identifiers only, one per line
    Name,
}

impl GlobalArgs {
    /// The server address with a scheme: a bare `host:port` means plaintext.
    pub fn server_url(&self) -> String {
        normalize_server(&self.server)
    }

    /// Whether the output is meant for a program rather than a person.
    pub fn is_structured(&self) -> bool {
        matches!(self.output, Output::Json | Output::Yaml)
    }

    /// The API key, ignoring an empty value.
    pub fn api_key(&self) -> Option<&str> {
        self.api_key.as_deref().filter(|k| !k.trim().is_empty())
    }
}

pub fn normalize_server(server: &str) -> String {
    let server = server.trim().trim_end_matches('/');
    if server.starts_with("http://") || server.starts_with("https://") {
        server.to_string()
    } else {
        format!("http://{server}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_address_is_plaintext() {
        assert_eq!(
            normalize_server("localhost:50061"),
            "http://localhost:50061"
        );
        assert_eq!(
            normalize_server("https://engine.example.com/"),
            "https://engine.example.com"
        );
    }
}
