//! CLI command type definitions.

use clap::Subcommand;

#[derive(Subcommand)]
pub enum NewCommands {
    /// Create a new project from a template: python, typescript or rust
    Project {
        /// Project name: lowercase letters, digits and hyphens
        name: String,

        /// Template to use: python, typescript or rust
        #[arg(short = 't', long = "template", default_value = "rust")]
        template: String,

        /// Directory to create the project in
        #[arg(short = 'd', long = "dir")]
        directory: Option<String>,
    },

    /// Create a new Python project (the orcher-sdk package)
    #[command(alias = "py")]
    Python {
        /// Project name: lowercase letters, digits and hyphens
        name: String,

        /// Directory to create the project in
        #[arg(short = 'd', long = "dir")]
        directory: Option<String>,
    },

    /// Create a new TypeScript project (@orcher/sdk)
    #[command(alias = "ts")]
    Typescript {
        /// Project name: lowercase letters, digits and hyphens
        name: String,

        /// Directory to create the project in
        #[arg(short = 'd', long = "dir")]
        directory: Option<String>,
    },

    /// Create a new Rust project (the orcher-sdk crate)
    #[command(alias = "rs")]
    Rust {
        /// Project name: lowercase letters, digits and hyphens
        name: String,

        /// Directory to create the project in
        #[arg(short = 'd', long = "dir")]
        directory: Option<String>,
    },

    /// Add a workflow, with one task, to the project in this directory
    Workflow {
        /// Workflow name
        name: String,

        /// Workflow type; only sequential is generated
        #[arg(short = 't', long = "type", default_value = "sequential")]
        workflow_type: String,

        /// File to write (default: one named after the workflow, where the
        /// project keeps its source)
        #[arg(short = 'f', long = "file")]
        file: Option<String>,

        /// Project language: python, typescript or rust (default: detected
        /// from the project in this directory)
        #[arg(short = 'l', long = "language")]
        language: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum ConfigCommands {
    /// View current configuration
    View {
        /// Show raw configuration without masking
        #[arg(long)]
        raw: bool,
    },

    /// List all contexts
    #[command(name = "get-contexts")]
    GetContexts,

    /// Set current context
    #[command(name = "use-context")]
    UseContext {
        /// Context name
        context: String,
    },

    /// Set configuration property
    Set {
        /// Property key
        key: String,

        /// Property value
        value: String,
    },

    /// Unset configuration property
    Unset {
        /// Property key
        key: String,
    },

    /// Delete context
    #[command(name = "delete-context")]
    DeleteContext {
        /// Context name
        context: String,
    },

    /// Set context properties
    #[command(name = "set-context")]
    SetContext {
        /// Context name
        context: String,

        /// Server URL
        #[arg(long)]
        server: Option<String>,

        /// Namespace
        #[arg(long)]
        namespace: Option<String>,

        /// Timeout
        #[arg(long)]
        timeout: Option<String>,
    },

    /// Test connection to server
    #[command(name = "test-connection")]
    TestConnection {
        /// Context to test (defaults to current context)
        #[arg(long)]
        context: Option<String>,

        /// Server URL to test (overrides context)
        #[arg(long)]
        server: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum AuthCommands {
    /// Login to ORCHER server
    Login {
        /// Server URL
        #[arg(long)]
        server: Option<String>,

        /// Use token authentication
        #[arg(long)]
        token: bool,

        /// Use device flow (for CLI/headless environments)
        #[arg(long, conflicts_with = "token")]
        device: bool,

        /// OAuth provider for device flow (github, google, microsoft)
        #[arg(long, requires = "device")]
        provider: Option<String>,

        /// Username for basic auth
        #[arg(short = 'u', long = "username")]
        username: Option<String>,
    },

    /// Logout from current context
    Logout,

    /// Show current authentication status
    Status,
}

#[derive(Subcommand)]
pub enum TemplateCommands {
    /// List available templates
    List,

    /// Show template details
    Describe {
        /// Template name
        name: String,
    },
}

#[derive(Subcommand)]
pub enum DevCommands {
    /// Start a local engine in Docker and wait until it is ready
    Start {
        /// Host port for gRPC: workers and the CLI connect here
        #[arg(
            short = 'p',
            long = "port",
            alias = "grpc-port",
            default_value = "50051"
        )]
        port: u16,

        /// Host port for HTTP: /health/ready and /metrics
        #[arg(long = "http-port", default_value = "8080")]
        http_port: u16,

        /// Host address to publish the ports on
        #[arg(long, default_value = "127.0.0.1")]
        host: String,

        /// Engine release to run
        #[arg(long, env = "ORCHER_ENGINE_VERSION", default_value = crate::local_engine::DEFAULT_ENGINE_VERSION)]
        engine_version: String,

        /// Engine image to run, overriding --engine-version (for a mirror, say)
        #[arg(long, env = "ORCHER_ENGINE_IMAGE")]
        image: Option<String>,

        /// Seconds to wait for the engine to become ready
        #[arg(long, default_value = "120")]
        timeout: u64,
    },

    /// Stop the local engine
    Stop {
        /// Also delete its database: every workflow it ran
        #[arg(long)]
        delete_data: bool,
    },

    /// Show whether the local engine is running, and where
    Status,

    /// Show the local engine's logs
    Logs {
        /// Follow log output
        #[arg(short = 'f', long = "follow")]
        follow: bool,

        /// How many lines to show from the end
        #[arg(long, default_value = "200")]
        tail: String,

        /// Show the database's log instead of the engine's
        #[arg(long)]
        postgres: bool,
    },
}
