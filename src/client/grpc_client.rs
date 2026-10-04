//! gRPC client for the ORCHER orchestrator (preferred over HTTP).
//! Auth: `ORCHER_TOKEN` env var > `ORCHER_API_KEY` env var > OS keychain per context.

use crate::config::Config;
use crate::error::{CliError, Result};
use crate::secure_storage::SecureStorage;
use orcher_proto::{
    // ActorService client (optional)
    actor_service_client::ActorServiceClient,
    // NamespaceService client
    namespace_service_client::NamespaceServiceClient,
    // QueryService client
    query_service_client::QueryServiceClient,
    // WorkflowService client
    workflow_service_client::WorkflowServiceClient,
    // Request/Response types
    CancelWorkflowRequest,
    CountWorkflowsRequest,
    CreateNamespaceRequest,
    DeleteNamespaceRequest,
    DeprecateNamespaceRequest,
    DescribeWorkflowExecutionRequest,
    DescribeWorkflowExecutionResponse,
    GetExecutionJournalRequest,
    GetExecutionJournalResponse,
    GetExecutionLogsRequest,
    GetExecutionLogsResponse,
    GetNamespaceRequest,
    GetTaskExecutionsRequest,
    GetTaskExecutionsResponse,
    GetWorkflowResultRequest,
    GetWorkflowResultResponse,
    GetWorkflowStatusRequest,
    GetWorkflowStatusResponse,
    ListNamespacesRequest,
    ListWorkflowsRequest,
    ListWorkflowsResponse,
    NamespaceInfo,
    QueryWorkflowRequest,
    QueryWorkflowResponse,
    SearchWorkflowsRequest,
    SearchWorkflowsResponse,
    SendEventRequest,
    StartWorkflowRequest,
    StartWorkflowResponse,
    TerminateWorkflowRequest,
    UpdateNamespaceRequest,
};
use std::time::Duration;
use tonic::metadata::MetadataValue;
use tonic::service::Interceptor;
use tonic::transport::{Certificate, Channel, ClientTlsConfig, Endpoint, Identity};
use tonic::{Request, Status};
use tracing::{debug, info, warn};

/// Default gRPC port for ORCHER orchestrator
pub const DEFAULT_GRPC_PORT: u16 = 50051;

/// Default timeout for gRPC requests
pub const DEFAULT_GRPC_TIMEOUT: Duration = Duration::from_secs(30);

/// Authentication interceptor for gRPC requests
///
/// Injects authentication headers (Bearer token) into all outgoing gRPC requests.
///
/// ## Authentication Sources (in priority order)
/// 1. `ORCHER_TOKEN` environment variable (for CI/CD pipelines)
/// 2. Token from secure storage (OS keychain) for current context
///
/// ## Note
/// API keys are not supported for CLI authentication. API keys are only
/// used for service/worker authentication via `orcher service register`.
#[derive(Clone)]
pub struct AuthInterceptor {
    /// Bearer token for authentication
    token: Option<String>,
    /// Deadline for a call that sets none of its own
    default_timeout: Option<Duration>,
}

impl AuthInterceptor {
    /// Create a new auth interceptor with no authentication
    pub fn none() -> Self {
        Self {
            token: None,
            default_timeout: None,
        }
    }

    /// Create a new auth interceptor with a bearer token
    pub fn with_token(token: String) -> Self {
        Self {
            token: Some(token),
            default_timeout: None,
        }
    }

    /// Load authentication from environment and secure storage
    ///
    /// Priority:
    /// 1. ORCHER_TOKEN environment variable (for CI/CD)
    /// 2. ORCHER_API_KEY environment variable
    /// 3. Token from secure storage for current context
    ///
    /// ORCHER_API_KEY was documented here and on every caller but never read,
    /// so a command given only an API key sent no credentials and the server
    /// answered "missing authorization header". Explicit variables come
    /// before a stored login: the one set for this command is the one meant.
    pub fn from_env_and_storage(context: Option<&str>) -> Self {
        // Priority 1: ORCHER_TOKEN env var
        if let Some(token) = non_empty_env("ORCHER_TOKEN") {
            debug!("Using token from ORCHER_TOKEN environment variable");
            return Self::with_token(token);
        }

        // Priority 2: ORCHER_API_KEY env var
        if let Some(key) = non_empty_env("ORCHER_API_KEY") {
            debug!("Using API key from ORCHER_API_KEY environment variable");
            return Self::with_token(key);
        }

        // Priority 2: Token from secure storage
        let context_name = context
            .map(|c| c.to_string())
            .or_else(|| Config::load().ok().map(|c| c.current_context))
            .unwrap_or_else(|| "local".to_string());

        if let Ok(storage) = SecureStorage::new(&context_name) {
            if let Ok(Some(token)) = storage.get_token() {
                debug!(
                    "Using token from secure storage for context '{}'",
                    context_name
                );
                return Self::with_token(token);
            }
        }

        debug!("No authentication configured");
        Self::none()
    }

    /// Check if authentication is configured
    pub fn is_authenticated(&self) -> bool {
        self.token.is_some()
    }
}

/// An environment variable's value, treating an empty one as unset.
pub(crate) fn non_empty_env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

impl AuthInterceptor {
    /// Give every call that sets no deadline of its own this one.
    pub fn with_default_timeout(mut self, timeout: Duration) -> Self {
        self.default_timeout = Some(timeout);
        self
    }
}

impl Interceptor for AuthInterceptor {
    fn call(&mut self, mut request: Request<()>) -> std::result::Result<Request<()>, Status> {
        // The deadline is per call, not on the channel: a channel deadline
        // caps every call, including one that waits on purpose for longer
        // (a workflow's result).
        if let Some(timeout) = self.default_timeout {
            if !request.metadata().contains_key("grpc-timeout") {
                request.set_timeout(timeout);
            }
        }

        // Add Bearer token if available
        if let Some(ref token) = self.token {
            let header_value = format!("Bearer {}", token);
            match header_value.parse::<MetadataValue<_>>() {
                Ok(value) => {
                    request.metadata_mut().insert("authorization", value);
                }
                Err(e) => {
                    warn!("Failed to create authorization header: {}", e);
                }
            }
        }

        Ok(request)
    }
}

/// Type alias for intercepted service
type InterceptedService = tonic::service::interceptor::InterceptedService<Channel, AuthInterceptor>;

/// Build a tonic [`Endpoint`] for `address`.
///
/// TLS is enabled automatically when the URI scheme is `https://` (cloud) and
/// left off for `http://` (local development) — mirroring how the CLI already
/// treats HTTP addresses.
///
/// By default the OS trust store (native roots) is used, so any standard,
/// publicly-issued cloud certificate just works. An optional per-context
/// [`TlsConfig`](crate::config::TlsConfig) can supply a custom CA (private
/// clouds) and/or a client identity (mutual TLS).
pub fn build_grpc_endpoint(
    address: &str,
    timeout: Duration,
    connect_timeout: Duration,
    tls: Option<&crate::config::TlsConfig>,
) -> Result<Endpoint> {
    Ok(build_untimed_endpoint(address, connect_timeout, tls)?.timeout(timeout))
}

/// An endpoint without a channel-wide request deadline, for a client whose
/// calls set their own.
fn build_untimed_endpoint(
    address: &str,
    connect_timeout: Duration,
    tls: Option<&crate::config::TlsConfig>,
) -> Result<Endpoint> {
    // Normalize: bare host defaults to plaintext http, matching the rest of the CLI.
    let uri = if address.starts_with("http://") || address.starts_with("https://") {
        address.to_string()
    } else {
        format!("http://{}", address)
    };
    let is_https = uri.starts_with("https://");

    let mut endpoint = Endpoint::from_shared(uri.clone())
        .map_err(|e| CliError::Config {
            message: format!("Invalid gRPC endpoint '{}': {}", uri, e),
            source: None,
        })?
        .connect_timeout(connect_timeout);

    if is_https {
        // Start from the system trust store.
        let mut tls_config = ClientTlsConfig::new().with_native_roots();

        if let Some(cfg) = tls {
            if cfg.insecure == Some(true) {
                // rustls (tonic's TLS backend) has no supported "accept invalid
                // certificate" toggle; be explicit rather than silently secure.
                warn!(
                    "TLS 'insecure' is set for this context, but disabling gRPC \
                     certificate verification is not supported — verifying normally. \
                     For private CAs, set 'ca-cert' instead."
                );
            }

            // Custom CA certificate (private / enterprise clouds).
            if let Some(ca_path) = &cfg.ca_cert {
                let pem = std::fs::read(ca_path).map_err(|e| CliError::Config {
                    message: format!("Failed to read CA certificate '{}': {}", ca_path, e),
                    source: Some(Box::new(e)),
                })?;
                tls_config = tls_config.ca_certificate(Certificate::from_pem(pem));
            }

            // Client identity for mutual TLS.
            if let (Some(cert_path), Some(key_path)) = (&cfg.client_cert, &cfg.client_key) {
                let cert = std::fs::read(cert_path).map_err(|e| CliError::Config {
                    message: format!("Failed to read client certificate '{}': {}", cert_path, e),
                    source: Some(Box::new(e)),
                })?;
                let key = std::fs::read(key_path).map_err(|e| CliError::Config {
                    message: format!("Failed to read client key '{}': {}", key_path, e),
                    source: Some(Box::new(e)),
                })?;
                tls_config = tls_config.identity(Identity::from_pem(cert, key));
            }
        }

        endpoint = endpoint
            .tls_config(tls_config)
            .map_err(|e| CliError::Config {
                message: format!("Failed to configure TLS for '{}': {}", uri, e),
                source: None,
            })?;
    }

    Ok(endpoint)
}

/// Load the TLS settings for a context (or the current context when `None`).
fn load_context_tls(context: Option<&str>) -> Option<crate::config::TlsConfig> {
    let config = Config::load().ok()?;
    let name = context
        .map(|c| c.to_string())
        .unwrap_or_else(|| config.current_context.clone());
    config.contexts.get(&name)?.tls.clone()
}

/// gRPC client for ORCHER orchestrator
#[derive(Clone)]
pub struct OrcherGrpcClient {
    /// WorkflowService client for workflow management
    workflow_client: WorkflowServiceClient<InterceptedService>,
    /// QueryService client for querying workflows
    query_client: QueryServiceClient<InterceptedService>,
    /// ActorService client for actor operations (optional)
    actor_client: ActorServiceClient<InterceptedService>,
    /// NamespaceService client for namespace management
    namespace_client: NamespaceServiceClient<InterceptedService>,
    /// Default namespace
    namespace: String,
    /// Whether client is authenticated
    authenticated: bool,
}

impl OrcherGrpcClient {
    /// Create a new gRPC client connected to the orchestrator
    pub async fn connect(address: &str) -> Result<Self> {
        Self::connect_with_options(address, "default", DEFAULT_GRPC_TIMEOUT).await
    }

    /// Create a new gRPC client with custom options
    pub async fn connect_with_options(
        address: &str,
        namespace: &str,
        timeout: Duration,
    ) -> Result<Self> {
        Self::connect_with_auth(address, namespace, timeout, None).await
    }

    /// Create a new gRPC client with authentication
    ///
    /// # Arguments
    ///
    /// * `address` - Server address (e.g., "localhost:50051" or "https://api.orcher.io")
    /// * `namespace` - Default namespace for operations
    /// * `timeout` - Request timeout
    /// * `context` - Optional context name for loading credentials from secure storage
    ///
    /// # Authentication Priority
    ///
    /// 1. `ORCHER_TOKEN` environment variable
    /// 2. `ORCHER_API_KEY` environment variable
    /// 3. Token from secure storage for the specified context
    pub async fn connect_with_auth(
        address: &str,
        namespace: &str,
        timeout: Duration,
        context: Option<&str>,
    ) -> Result<Self> {
        info!("Connecting to ORCHER orchestrator at {}", address);

        // Load per-context TLS settings (custom CA / mTLS) if configured.
        let tls = load_context_tls(context);

        // Build the endpoint — TLS is auto-enabled for https:// (cloud) addresses.
        let endpoint = build_untimed_endpoint(address, Duration::from_secs(10), tls.as_ref())?;

        // Connect to the server
        let channel = endpoint.connect().await.map_err(|e| CliError::Network {
            message: format!("Failed to connect to orchestrator at {}: {}", address, e),
            source: None,
        })?;

        // Create auth interceptor
        let interceptor =
            AuthInterceptor::from_env_and_storage(context).with_default_timeout(timeout);
        let authenticated = interceptor.is_authenticated();

        if authenticated {
            info!("Connected to ORCHER orchestrator with authentication");
        } else {
            debug!("Connected to ORCHER orchestrator (unauthenticated)");
        }

        Ok(Self {
            workflow_client: WorkflowServiceClient::with_interceptor(
                channel.clone(),
                interceptor.clone(),
            ),
            query_client: QueryServiceClient::with_interceptor(
                channel.clone(),
                interceptor.clone(),
            ),
            actor_client: ActorServiceClient::with_interceptor(
                channel.clone(),
                interceptor.clone(),
            ),
            namespace_client: NamespaceServiceClient::with_interceptor(channel, interceptor),
            namespace: namespace.to_string(),
            authenticated,
        })
    }

    /// Check if the client is authenticated
    pub fn is_authenticated(&self) -> bool {
        self.authenticated
    }

    /// Get the default namespace
    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    /// Set the default namespace
    pub fn set_namespace(&mut self, namespace: &str) {
        self.namespace = namespace.to_string();
    }

    // =========================================================================
    // WorkflowService Operations
    // =========================================================================

    /// Start a new workflow execution
    pub async fn start_workflow(
        &mut self,
        workflow_type: &str,
        workflow_id: Option<&str>,
        task_queue: &str,
        input: Option<Vec<u8>>,
    ) -> Result<StartWorkflowResponse> {
        let request = StartWorkflowRequest {
            workflow_id: workflow_id.unwrap_or_default().to_string(),
            workflow_type: workflow_type.to_string(),
            task_queue: task_queue.to_string(),
            namespace: self.namespace.clone(),
            input: input.unwrap_or_default(),
            ..Default::default()
        };

        let response = self
            .workflow_client
            .start_workflow(request)
            .await
            .map_err(|e| CliError::Api {
                status: e.code() as u16,
                message: format!("Failed to start workflow: {}", e.message()),
                endpoint: Some("WorkflowService.StartWorkflow".to_string()),
            })?;

        Ok(response.into_inner())
    }

    /// Wait up to `wait` for a workflow to finish, and return its outcome.
    ///
    /// The engine holds the call open until the workflow ends or `wait`
    /// passes, so the request's own deadline allows for both.
    pub async fn get_workflow_result(
        &mut self,
        workflow_id: &str,
        execution_id: Option<&str>,
        wait: Duration,
    ) -> Result<GetWorkflowResultResponse> {
        let mut request = Request::new(GetWorkflowResultRequest {
            workflow_id: workflow_id.to_string(),
            execution_id: execution_id.unwrap_or_default().to_string(),
            namespace: self.namespace.clone(),
            timeout: Some(prost_types::Duration {
                seconds: wait.as_secs() as i64,
                nanos: 0,
            }),
        });
        request.set_timeout(wait + DEFAULT_GRPC_TIMEOUT);

        let response = self
            .workflow_client
            .get_workflow_result(request)
            .await
            .map_err(|e| match e.code() {
                tonic::Code::DeadlineExceeded => CliError::Timeout {
                    operation: format!(
                        "workflow '{}' did not finish within {}s; wait longer with --timeout",
                        workflow_id,
                        wait.as_secs()
                    ),
                    duration: wait,
                },
                _ => CliError::Api {
                    status: e.code() as u16,
                    message: format!("Failed to get workflow result: {}", e.message()),
                    endpoint: Some("WorkflowService.GetWorkflowResult".to_string()),
                },
            })?;

        Ok(response.into_inner())
    }

    /// Get workflow status
    pub async fn get_workflow_status(
        &mut self,
        workflow_id: &str,
        execution_id: Option<&str>,
    ) -> Result<GetWorkflowStatusResponse> {
        let request = GetWorkflowStatusRequest {
            workflow_id: workflow_id.to_string(),
            execution_id: execution_id.unwrap_or_default().to_string(),
            namespace: self.namespace.clone(),
        };

        let response = self
            .workflow_client
            .get_workflow_status(request)
            .await
            .map_err(|e| CliError::Api {
                status: e.code() as u16,
                message: format!("Failed to get workflow status: {}", e.message()),
                endpoint: Some("WorkflowService.GetWorkflowStatus".to_string()),
            })?;

        Ok(response.into_inner())
    }

    /// Cancel a workflow execution (graceful)
    pub async fn cancel_workflow(
        &mut self,
        workflow_id: &str,
        execution_id: Option<&str>,
        reason: Option<&str>,
    ) -> Result<bool> {
        let request = CancelWorkflowRequest {
            workflow_id: workflow_id.to_string(),
            execution_id: execution_id.unwrap_or_default().to_string(),
            namespace: self.namespace.clone(),
            reason: reason.unwrap_or_default().to_string(),
            ..Default::default()
        };

        let response = self
            .workflow_client
            .cancel_workflow(request)
            .await
            .map_err(|e| CliError::Api {
                status: e.code() as u16,
                message: format!("Failed to cancel workflow: {}", e.message()),
                endpoint: Some("WorkflowService.CancelWorkflow".to_string()),
            })?;

        Ok(response.into_inner().requested)
    }

    /// Terminate a workflow execution (forceful)
    pub async fn terminate_workflow(
        &mut self,
        workflow_id: &str,
        execution_id: Option<&str>,
        reason: &str,
    ) -> Result<bool> {
        let request = TerminateWorkflowRequest {
            workflow_id: workflow_id.to_string(),
            execution_id: execution_id.unwrap_or_default().to_string(),
            namespace: self.namespace.clone(),
            reason: reason.to_string(),
            ..Default::default()
        };

        let response = self
            .workflow_client
            .terminate_workflow(request)
            .await
            .map_err(|e| CliError::Api {
                status: e.code() as u16,
                message: format!("Failed to terminate workflow: {}", e.message()),
                endpoint: Some("WorkflowService.TerminateWorkflow".to_string()),
            })?;

        Ok(response.into_inner().terminated)
    }

    /// Send an event to a workflow
    pub async fn send_event(
        &mut self,
        workflow_id: &str,
        event_name: &str,
        payload: Option<Vec<u8>>,
    ) -> Result<bool> {
        let request = SendEventRequest {
            workflow_id: workflow_id.to_string(),
            namespace: self.namespace.clone(),
            event_name: event_name.to_string(),
            payload: payload.unwrap_or_default(),
            ..Default::default()
        };

        let response = self
            .workflow_client
            .send_event(request)
            .await
            .map_err(|e| CliError::Api {
                status: e.code() as u16,
                message: format!("Failed to send event: {}", e.message()),
                endpoint: Some("WorkflowService.SendEvent".to_string()),
            })?;

        Ok(response.into_inner().recorded)
    }

    /// Query a workflow
    pub async fn query_workflow(
        &mut self,
        workflow_id: &str,
        query_type: &str,
        query_args: Option<Vec<u8>>,
    ) -> Result<QueryWorkflowResponse> {
        let request = QueryWorkflowRequest {
            workflow_id: workflow_id.to_string(),
            namespace: self.namespace.clone(),
            query_type: query_type.to_string(),
            query_args: query_args.unwrap_or_default(),
            ..Default::default()
        };

        let response = self
            .workflow_client
            .query_workflow(request)
            .await
            .map_err(|e| CliError::Api {
                status: e.code() as u16,
                message: format!("Failed to query workflow: {}", e.message()),
                endpoint: Some("WorkflowService.QueryWorkflow".to_string()),
            })?;

        Ok(response.into_inner())
    }

    // =========================================================================
    // QueryService Operations
    // =========================================================================

    /// List workflow executions
    pub async fn list_workflows(
        &mut self,
        workflow_type: Option<&str>,
        status: Option<i32>,
        page_size: i32,
        page_token: Option<Vec<u8>>,
    ) -> Result<ListWorkflowsResponse> {
        let request = ListWorkflowsRequest {
            namespace: self.namespace.clone(),
            workflow_type: workflow_type.unwrap_or_default().to_string(),
            status_filter: status.map(|s| vec![s]).unwrap_or_default(),
            page_size,
            next_page_token: page_token.unwrap_or_default(),
            ..Default::default()
        };

        let response = self
            .query_client
            .list_workflows(request)
            .await
            .map_err(|e| CliError::Api {
                status: e.code() as u16,
                message: format!("Failed to list workflows: {}", e.message()),
                endpoint: Some("QueryService.ListWorkflows".to_string()),
            })?;

        Ok(response.into_inner())
    }

    /// Search workflows with advanced query
    pub async fn search_workflows(
        &mut self,
        query: &str,
        page_size: i32,
        page_token: Option<Vec<u8>>,
    ) -> Result<SearchWorkflowsResponse> {
        let request = SearchWorkflowsRequest {
            namespace: self.namespace.clone(),
            query: query.to_string(),
            page_size,
            next_page_token: page_token.unwrap_or_default(),
            ..Default::default()
        };

        let response = self
            .query_client
            .search_workflows(request)
            .await
            .map_err(|e| CliError::Api {
                status: e.code() as u16,
                message: format!("Failed to search workflows: {}", e.message()),
                endpoint: Some("QueryService.SearchWorkflows".to_string()),
            })?;

        Ok(response.into_inner())
    }

    /// Get workflow execution journal (history)
    pub async fn get_execution_journal(
        &mut self,
        workflow_id: &str,
        execution_id: Option<&str>,
        page_size: i32,
        page_token: Option<Vec<u8>>,
    ) -> Result<GetExecutionJournalResponse> {
        let request = GetExecutionJournalRequest {
            namespace: self.namespace.clone(),
            workflow_id: workflow_id.to_string(),
            execution_id: execution_id.unwrap_or_default().to_string(),
            page_size,
            next_page_token: page_token.unwrap_or_default(),
            ..Default::default()
        };

        let response = self
            .query_client
            .get_execution_journal(request)
            .await
            .map_err(|e| CliError::Api {
                status: e.code() as u16,
                message: format!("Failed to get execution journal: {}", e.message()),
                endpoint: Some("QueryService.GetExecutionJournal".to_string()),
            })?;

        Ok(response.into_inner())
    }

    /// Describe a workflow execution (detailed info)
    pub async fn describe_workflow(
        &mut self,
        workflow_id: &str,
        execution_id: Option<&str>,
    ) -> Result<DescribeWorkflowExecutionResponse> {
        let request = DescribeWorkflowExecutionRequest {
            namespace: self.namespace.clone(),
            workflow_id: workflow_id.to_string(),
            execution_id: execution_id.unwrap_or_default().to_string(),
        };

        let response = self
            .query_client
            .describe_workflow_execution(request)
            .await
            .map_err(|e| CliError::Api {
                status: e.code() as u16,
                message: format!("Failed to describe workflow: {}", e.message()),
                endpoint: Some("QueryService.DescribeWorkflowExecution".to_string()),
            })?;

        Ok(response.into_inner())
    }

    /// Count workflows matching a query
    pub async fn count_workflows(&mut self, query: &str) -> Result<i64> {
        let request = CountWorkflowsRequest {
            namespace: self.namespace.clone(),
            query: query.to_string(),
        };

        let response = self
            .query_client
            .count_workflows(request)
            .await
            .map_err(|e| CliError::Api {
                status: e.code() as u16,
                message: format!("Failed to count workflows: {}", e.message()),
                endpoint: Some("QueryService.CountWorkflows".to_string()),
            })?;

        Ok(response.into_inner().count)
    }

    /// Get execution logs (human-readable text logs)
    pub async fn get_execution_logs(
        &mut self,
        workflow_id: &str,
        execution_id: Option<&str>,
        page_size: i32,
        level: Option<&str>,
        step: Option<&str>,
    ) -> Result<GetExecutionLogsResponse> {
        let request = GetExecutionLogsRequest {
            namespace: self.namespace.clone(),
            workflow_id: workflow_id.to_string(),
            execution_id: execution_id.unwrap_or_default().to_string(),
            page_size,
            level: level.unwrap_or_default().to_string(),
            step: step.unwrap_or_default().to_string(),
            ..Default::default()
        };

        let response = self
            .query_client
            .get_execution_logs(request)
            .await
            .map_err(|e| CliError::Api {
                status: e.code() as u16,
                message: format!("Failed to get execution logs: {}", e.message()),
                endpoint: Some("QueryService.GetExecutionLogs".to_string()),
            })?;

        Ok(response.into_inner())
    }

    /// Get the log lines after `after_id`, for following a log as it grows.
    pub async fn get_execution_logs_after(
        &mut self,
        workflow_id: &str,
        after_id: i64,
        page_size: i32,
        level: Option<&str>,
        step: Option<&str>,
    ) -> Result<GetExecutionLogsResponse> {
        let request = GetExecutionLogsRequest {
            namespace: self.namespace.clone(),
            workflow_id: workflow_id.to_string(),
            page_size,
            level: level.unwrap_or_default().to_string(),
            step: step.unwrap_or_default().to_string(),
            after_id,
            ..Default::default()
        };

        let response = self
            .query_client
            .get_execution_logs(request)
            .await
            .map_err(|e| CliError::Api {
                status: e.code() as u16,
                message: format!("Failed to get execution logs: {}", e.message()),
                endpoint: Some("QueryService.GetExecutionLogs".to_string()),
            })?;

        Ok(response.into_inner())
    }

    /// Get task executions for a workflow
    pub async fn get_task_executions(
        &mut self,
        workflow_id: &str,
        execution_id: Option<&str>,
        page_size: i32,
    ) -> Result<GetTaskExecutionsResponse> {
        let request = GetTaskExecutionsRequest {
            namespace: self.namespace.clone(),
            workflow_id: workflow_id.to_string(),
            execution_id: execution_id.unwrap_or_default().to_string(),
            page_size,
            ..Default::default()
        };

        let response = self
            .query_client
            .get_task_executions(request)
            .await
            .map_err(|e| CliError::Api {
                status: e.code() as u16,
                message: format!("Failed to get task executions: {}", e.message()),
                endpoint: Some("QueryService.GetTaskExecutions".to_string()),
            })?;

        Ok(response.into_inner())
    }

    // =========================================================================
    // NamespaceService Operations
    // =========================================================================

    /// Create a new namespace
    pub async fn create_namespace(
        &mut self,
        name: &str,
        description: &str,
        retention_period_days: i32,
        owner_email: &str,
        data: std::collections::HashMap<String, String>,
    ) -> Result<NamespaceInfo> {
        let request = CreateNamespaceRequest {
            name: name.to_string(),
            description: description.to_string(),
            retention_period_days,
            owner_email: owner_email.to_string(),
            data,
        };

        let response = self
            .namespace_client
            .create_namespace(request)
            .await
            .map_err(|e| CliError::Api {
                status: e.code() as u16,
                message: format!("Failed to create namespace: {}", e.message()),
                endpoint: Some("NamespaceService.CreateNamespace".to_string()),
            })?;

        response
            .into_inner()
            .namespace
            .ok_or_else(|| CliError::Api {
                status: 0,
                message: "Empty response from CreateNamespace".to_string(),
                endpoint: Some("NamespaceService.CreateNamespace".to_string()),
            })
    }

    /// Get namespace details
    pub async fn get_namespace(&mut self, name: &str) -> Result<NamespaceInfo> {
        let request = GetNamespaceRequest {
            name: name.to_string(),
        };

        let response = self
            .namespace_client
            .get_namespace(request)
            .await
            .map_err(|e| CliError::Api {
                status: e.code() as u16,
                message: format!("Failed to get namespace: {}", e.message()),
                endpoint: Some("NamespaceService.GetNamespace".to_string()),
            })?;

        response
            .into_inner()
            .namespace
            .ok_or_else(|| CliError::Api {
                status: 0,
                message: "Empty response from GetNamespace".to_string(),
                endpoint: Some("NamespaceService.GetNamespace".to_string()),
            })
    }

    /// List namespaces with pagination
    pub async fn list_namespaces(
        &mut self,
        page_size: i32,
        page_offset: i32,
    ) -> Result<(Vec<NamespaceInfo>, i64)> {
        let request = ListNamespacesRequest {
            page_size,
            page_offset,
        };

        let response = self
            .namespace_client
            .list_namespaces(request)
            .await
            .map_err(|e| CliError::Api {
                status: e.code() as u16,
                message: format!("Failed to list namespaces: {}", e.message()),
                endpoint: Some("NamespaceService.ListNamespaces".to_string()),
            })?;

        let inner = response.into_inner();
        Ok((inner.namespaces, inner.total_count))
    }

    /// Update namespace metadata
    pub async fn update_namespace(
        &mut self,
        name: &str,
        description: &str,
        owner_email: &str,
        retention_period_days: i32,
        data: std::collections::HashMap<String, String>,
    ) -> Result<NamespaceInfo> {
        let request = UpdateNamespaceRequest {
            name: name.to_string(),
            description: description.to_string(),
            owner_email: owner_email.to_string(),
            retention_period_days,
            data,
        };

        let response = self
            .namespace_client
            .update_namespace(request)
            .await
            .map_err(|e| CliError::Api {
                status: e.code() as u16,
                message: format!("Failed to update namespace: {}", e.message()),
                endpoint: Some("NamespaceService.UpdateNamespace".to_string()),
            })?;

        response
            .into_inner()
            .namespace
            .ok_or_else(|| CliError::Api {
                status: 0,
                message: "Empty response from UpdateNamespace".to_string(),
                endpoint: Some("NamespaceService.UpdateNamespace".to_string()),
            })
    }

    /// Deprecate a namespace
    pub async fn deprecate_namespace(&mut self, name: &str) -> Result<NamespaceInfo> {
        let request = DeprecateNamespaceRequest {
            name: name.to_string(),
        };

        let response = self
            .namespace_client
            .deprecate_namespace(request)
            .await
            .map_err(|e| CliError::Api {
                status: e.code() as u16,
                message: format!("Failed to deprecate namespace: {}", e.message()),
                endpoint: Some("NamespaceService.DeprecateNamespace".to_string()),
            })?;

        response
            .into_inner()
            .namespace
            .ok_or_else(|| CliError::Api {
                status: 0,
                message: "Empty response from DeprecateNamespace".to_string(),
                endpoint: Some("NamespaceService.DeprecateNamespace".to_string()),
            })
    }

    /// Delete a namespace (soft delete)
    pub async fn delete_namespace(&mut self, name: &str) -> Result<()> {
        let request = DeleteNamespaceRequest {
            name: name.to_string(),
        };

        self.namespace_client
            .delete_namespace(request)
            .await
            .map_err(|e| CliError::Api {
                status: e.code() as u16,
                message: format!("Failed to delete namespace: {}", e.message()),
                endpoint: Some("NamespaceService.DeleteNamespace".to_string()),
            })?;

        Ok(())
    }
}

/// Convert WorkflowExecutionStatus to a display string
pub fn status_to_string(status: i32) -> &'static str {
    match status {
        1 => "RUNNING",
        2 => "COMPLETED",
        3 => "FAILED",
        4 => "CANCELED",
        5 => "TERMINATED",
        6 => "RESTARTED_FRESH",
        7 => "TIMED_OUT",
        8 => "RESET",
        9 => "PENDING",
        _ => "UNKNOWN",
    }
}

/// Parse a status string to WorkflowExecutionStatus value
pub fn string_to_status(status: &str) -> Option<i32> {
    match status.to_uppercase().as_str() {
        "RUNNING" => Some(1),
        "COMPLETED" => Some(2),
        "FAILED" => Some(3),
        "CANCELED" | "CANCELLED" => Some(4),
        "TERMINATED" => Some(5),
        "RESTARTED_FRESH" | "CONTINUED_AS_NEW" => Some(6),
        "TIMED_OUT" | "TIMEOUT" => Some(7),
        "RESET" => Some(8),
        "PENDING" => Some(9),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ORCHER_API_KEY was documented as a credential source and never read,
    /// so a command given only a key sent nothing. Explicit variables come
    /// before any stored login, the token before the key.
    #[test]
    fn credentials_come_from_the_token_then_the_api_key() {
        let context = Some("credential-order-test-context-without-a-stored-login");
        // The library tests run single-threaded (--test-threads=1) because
        // several of them change the process environment.
        {
            std::env::remove_var("ORCHER_TOKEN");
            std::env::set_var("ORCHER_API_KEY", "key-from-env");
        }
        assert_eq!(
            AuthInterceptor::from_env_and_storage(context)
                .token
                .as_deref(),
            Some("key-from-env")
        );

        std::env::set_var("ORCHER_TOKEN", "token-from-env");
        assert_eq!(
            AuthInterceptor::from_env_and_storage(context)
                .token
                .as_deref(),
            Some("token-from-env"),
            "the token wins over the key"
        );

        {
            std::env::set_var("ORCHER_TOKEN", " ");
            std::env::remove_var("ORCHER_API_KEY");
        }
        assert!(
            AuthInterceptor::from_env_and_storage(context)
                .token
                .is_none(),
            "an empty variable is not a credential"
        );

        std::env::remove_var("ORCHER_TOKEN");
    }

    #[test]
    fn test_status_to_string() {
        assert_eq!(status_to_string(1), "RUNNING");
        assert_eq!(status_to_string(2), "COMPLETED");
        assert_eq!(status_to_string(3), "FAILED");
        assert_eq!(status_to_string(99), "UNKNOWN");
    }

    #[test]
    fn test_string_to_status() {
        assert_eq!(string_to_status("running"), Some(1));
        assert_eq!(string_to_status("COMPLETED"), Some(2));
        assert_eq!(string_to_status("cancelled"), Some(4));
        assert_eq!(string_to_status("invalid"), None);
    }

    // Short timeouts so the connect tests fail fast.
    const T: Duration = Duration::from_millis(500);

    #[test]
    fn test_build_endpoint_http_plaintext() {
        // Local http and bare host both build a plaintext endpoint.
        assert!(build_grpc_endpoint("http://localhost:50051", T, T, None).is_ok());
        assert!(build_grpc_endpoint("localhost:50051", T, T, None).is_ok());
    }

    #[test]
    fn test_build_endpoint_https_tls() {
        // https builds an endpoint with TLS configured (system roots).
        assert!(build_grpc_endpoint(crate::constants::DEFAULT_CLOUD_URL, T, T, None).is_ok());
    }

    #[test]
    fn test_build_endpoint_rejects_invalid_uri() {
        assert!(build_grpc_endpoint("http://[bad", T, T, None).is_err());
    }

    /// Connecting an https endpoint to an unreachable host must return an error,
    /// NOT panic. A panic here would mean rustls could not resolve a crypto
    /// provider (both ring and aws-lc-rs are in the tree) — the one TLS failure
    /// mode we can't otherwise catch without a live cloud server.
    #[tokio::test]
    async fn test_https_connect_resolves_crypto_provider() {
        let endpoint = build_grpc_endpoint("https://127.0.0.1:1", T, T, None)
            .expect("https endpoint should build");
        // Port 1 is unreachable: we expect a connection error, and critically no panic.
        assert!(endpoint.connect().await.is_err());
    }
}
