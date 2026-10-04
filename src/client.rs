//! A thin client over the engine's public gRPC API. Every method is one RPC;
//! the commands decide what to show.

use crate::error::{Error, Result};
use crate::settings::GlobalArgs;
use orcher_proto::{
    namespace_service_client::NamespaceServiceClient, query_service_client::QueryServiceClient,
    workflow_service_client::WorkflowServiceClient, CancelWorkflowRequest, CreateNamespaceRequest,
    DeleteNamespaceRequest, DeprecateNamespaceRequest, DescribeWorkflowExecutionRequest,
    DescribeWorkflowExecutionResponse, GetExecutionJournalRequest, GetExecutionJournalResponse,
    GetExecutionLogsRequest, GetExecutionLogsResponse, GetNamespaceRequest,
    GetTaskExecutionsRequest, GetTaskExecutionsResponse, GetWorkflowStatusRequest,
    GetWorkflowStatusResponse, ListNamespacesRequest, ListWorkflowsRequest, ListWorkflowsResponse,
    NamespaceInfo, SearchWorkflowsRequest, SearchWorkflowsResponse, SendEventRequest,
    TerminateWorkflowRequest, UpdateNamespaceRequest,
};
use std::time::Duration;
use tonic::metadata::MetadataValue;
use tonic::service::interceptor::InterceptedService;
use tonic::service::Interceptor;
use tonic::transport::{Channel, ClientTlsConfig, Endpoint};

/// How long to wait for the TCP and TLS handshake.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// How long an ordinary call may take. Calls that wait on purpose (for a
/// workflow's result) set their own deadline.
pub const CALL_TIMEOUT: Duration = Duration::from_secs(30);

/// Adds the API key, when there is one, to every request.
#[derive(Clone)]
pub struct ApiKey(Option<MetadataValue<tonic::metadata::Ascii>>);

impl Interceptor for ApiKey {
    fn call(
        &mut self,
        mut request: tonic::Request<()>,
    ) -> Result<tonic::Request<()>, tonic::Status> {
        if let Some(value) = &self.0 {
            request
                .metadata_mut()
                .insert("authorization", value.clone());
        }
        Ok(request)
    }
}

type Svc = InterceptedService<Channel, ApiKey>;

pub struct Client {
    workflows: WorkflowServiceClient<Svc>,
    queries: QueryServiceClient<Svc>,
    namespaces: NamespaceServiceClient<Svc>,
    namespace: String,
}

/// Builds the endpoint for `address`: TLS for `https://`, plaintext otherwise.
pub fn endpoint(address: &str) -> Result<Endpoint> {
    let endpoint = Endpoint::from_shared(address.to_string())
        .map_err(|e| Error::invalid_input(format!("invalid server address '{address}': {e}")))?
        .connect_timeout(CONNECT_TIMEOUT);
    if address.starts_with("https://") {
        endpoint
            .tls_config(ClientTlsConfig::new().with_native_roots())
            .map_err(|e| Error::local(format!("cannot set up TLS for {address}: {e}")))
    } else {
        Ok(endpoint)
    }
}

/// A request carrying the usual deadline.
fn req<T>(message: T) -> tonic::Request<T> {
    let mut request = tonic::Request::new(message);
    request.set_timeout(CALL_TIMEOUT);
    request
}

impl Client {
    /// Connects to the engine named by the global options.
    pub async fn connect(args: &GlobalArgs) -> Result<Self> {
        let address = args.server_url();
        let channel = endpoint(&address)?
            .connect()
            .await
            .map_err(|e| Error::Connect {
                address: address.clone(),
                reason: root_cause(&e),
            })?;

        let key =
            match args.api_key() {
                Some(key) => Some(format!("Bearer {key}").parse().map_err(|_| {
                    Error::invalid_input("the API key contains invalid characters")
                })?),
                None => None,
            };
        let auth = ApiKey(key);

        Ok(Self {
            workflows: WorkflowServiceClient::with_interceptor(channel.clone(), auth.clone()),
            queries: QueryServiceClient::with_interceptor(channel.clone(), auth.clone()),
            namespaces: NamespaceServiceClient::with_interceptor(channel, auth),
            namespace: args.namespace.clone(),
        })
    }

    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    // Workflows

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
        self.workflows
            .get_workflow_status(req(request))
            .await
            .map(|r| r.into_inner())
            .map_err(|s| Error::api("WorkflowService.GetWorkflowStatus", s))
    }

    pub async fn cancel_workflow(
        &mut self,
        workflow_id: &str,
        execution_id: Option<&str>,
        reason: &str,
    ) -> Result<bool> {
        let request = CancelWorkflowRequest {
            workflow_id: workflow_id.to_string(),
            execution_id: execution_id.unwrap_or_default().to_string(),
            namespace: self.namespace.clone(),
            reason: reason.to_string(),
            ..Default::default()
        };
        self.workflows
            .cancel_workflow(req(request))
            .await
            .map(|r| r.into_inner().requested)
            .map_err(|s| Error::api("WorkflowService.CancelWorkflow", s))
    }

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
        self.workflows
            .terminate_workflow(req(request))
            .await
            .map(|r| r.into_inner().terminated)
            .map_err(|s| Error::api("WorkflowService.TerminateWorkflow", s))
    }

    pub async fn send_event(
        &mut self,
        workflow_id: &str,
        event_name: &str,
        payload: Vec<u8>,
    ) -> Result<bool> {
        let request = SendEventRequest {
            workflow_id: workflow_id.to_string(),
            namespace: self.namespace.clone(),
            event_name: event_name.to_string(),
            payload,
            ..Default::default()
        };
        self.workflows
            .send_event(req(request))
            .await
            .map(|r| r.into_inner().recorded)
            .map_err(|s| Error::api("WorkflowService.SendEvent", s))
    }

    // Queries

    pub async fn list_workflows(
        &mut self,
        workflow_type: Option<&str>,
        status: Option<i32>,
        page_size: i32,
    ) -> Result<ListWorkflowsResponse> {
        let request = ListWorkflowsRequest {
            namespace: self.namespace.clone(),
            workflow_type: workflow_type.unwrap_or_default().to_string(),
            status_filter: status.into_iter().collect(),
            page_size,
            ..Default::default()
        };
        self.queries
            .list_workflows(req(request))
            .await
            .map(|r| r.into_inner())
            .map_err(|s| Error::api("QueryService.ListWorkflows", s))
    }

    pub async fn search_workflows(
        &mut self,
        query: &str,
        page_size: i32,
    ) -> Result<SearchWorkflowsResponse> {
        let request = SearchWorkflowsRequest {
            namespace: self.namespace.clone(),
            query: query.to_string(),
            page_size,
            ..Default::default()
        };
        self.queries
            .search_workflows(req(request))
            .await
            .map(|r| r.into_inner())
            .map_err(|s| Error::api("QueryService.SearchWorkflows", s))
    }

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
        self.queries
            .describe_workflow_execution(req(request))
            .await
            .map(|r| r.into_inner())
            .map_err(|s| Error::api("QueryService.DescribeWorkflowExecution", s))
    }

    pub async fn get_execution_journal(
        &mut self,
        workflow_id: &str,
        execution_id: Option<&str>,
        page_size: i32,
    ) -> Result<GetExecutionJournalResponse> {
        let request = GetExecutionJournalRequest {
            namespace: self.namespace.clone(),
            workflow_id: workflow_id.to_string(),
            execution_id: execution_id.unwrap_or_default().to_string(),
            page_size,
            ..Default::default()
        };
        self.queries
            .get_execution_journal(req(request))
            .await
            .map(|r| r.into_inner())
            .map_err(|s| Error::api("QueryService.GetExecutionJournal", s))
    }

    pub async fn get_execution_logs(
        &mut self,
        request: GetExecutionLogsRequest,
    ) -> Result<GetExecutionLogsResponse> {
        let request = GetExecutionLogsRequest {
            namespace: self.namespace.clone(),
            ..request
        };
        self.queries
            .get_execution_logs(req(request))
            .await
            .map(|r| r.into_inner())
            .map_err(|s| Error::api("QueryService.GetExecutionLogs", s))
    }

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
        self.queries
            .get_task_executions(req(request))
            .await
            .map(|r| r.into_inner())
            .map_err(|s| Error::api("QueryService.GetTaskExecutions", s))
    }

    // Namespaces

    pub async fn create_namespace(
        &mut self,
        request: CreateNamespaceRequest,
    ) -> Result<NamespaceInfo> {
        let response = self
            .namespaces
            .create_namespace(req(request))
            .await
            .map_err(|s| Error::api("NamespaceService.CreateNamespace", s))?;
        response
            .into_inner()
            .namespace
            .ok_or_else(|| empty("NamespaceService.CreateNamespace"))
    }

    pub async fn get_namespace(&mut self, name: &str) -> Result<NamespaceInfo> {
        let request = GetNamespaceRequest {
            name: name.to_string(),
        };
        let response = self
            .namespaces
            .get_namespace(req(request))
            .await
            .map_err(|s| Error::api("NamespaceService.GetNamespace", s))?;
        response
            .into_inner()
            .namespace
            .ok_or_else(|| empty("NamespaceService.GetNamespace"))
    }

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
            .namespaces
            .list_namespaces(req(request))
            .await
            .map_err(|s| Error::api("NamespaceService.ListNamespaces", s))?
            .into_inner();
        Ok((response.namespaces, response.total_count))
    }

    pub async fn update_namespace(
        &mut self,
        request: UpdateNamespaceRequest,
    ) -> Result<NamespaceInfo> {
        let response = self
            .namespaces
            .update_namespace(req(request))
            .await
            .map_err(|s| Error::api("NamespaceService.UpdateNamespace", s))?;
        response
            .into_inner()
            .namespace
            .ok_or_else(|| empty("NamespaceService.UpdateNamespace"))
    }

    pub async fn deprecate_namespace(&mut self, name: &str) -> Result<NamespaceInfo> {
        let request = DeprecateNamespaceRequest {
            name: name.to_string(),
        };
        let response = self
            .namespaces
            .deprecate_namespace(req(request))
            .await
            .map_err(|s| Error::api("NamespaceService.DeprecateNamespace", s))?;
        response
            .into_inner()
            .namespace
            .ok_or_else(|| empty("NamespaceService.DeprecateNamespace"))
    }

    pub async fn delete_namespace(&mut self, name: &str) -> Result<()> {
        let request = DeleteNamespaceRequest {
            name: name.to_string(),
        };
        self.namespaces
            .delete_namespace(req(request))
            .await
            .map(|_| ())
            .map_err(|s| Error::api("NamespaceService.DeleteNamespace", s))
    }
}

fn empty(rpc: &'static str) -> Error {
    Error::Api {
        rpc,
        code: tonic::Code::Internal,
        message: "the engine returned an empty response".to_string(),
    }
}

/// The innermost cause of a transport error, which is the one that says what
/// actually went wrong ("Connection refused" rather than "transport error").
fn root_cause(err: &(dyn std::error::Error + 'static)) -> String {
    let mut cause = err;
    while let Some(next) = cause.source() {
        cause = next;
    }
    cause.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plaintext_and_tls_endpoints_build() {
        assert!(endpoint("http://localhost:50051").is_ok());
        assert!(endpoint("https://engine.example.com:443").is_ok());
        assert!(endpoint("http://[bad").is_err());
    }

    /// Connecting over TLS to a closed port must fail with an error, not
    /// panic: a panic would mean rustls found no crypto provider to use.
    #[tokio::test]
    async fn a_tls_connection_failure_is_an_error_not_a_panic() {
        let endpoint = endpoint("https://127.0.0.1:1").expect("endpoint builds");
        assert!(endpoint.connect().await.is_err());
    }

    #[tokio::test]
    async fn an_unreachable_engine_is_reported_with_its_address() {
        let args = GlobalArgs {
            server: "http://127.0.0.1:1".to_string(),
            namespace: "default".to_string(),
            api_key: None,
            output: crate::settings::Output::Table,
            quiet: false,
            no_color: true,
        };
        let err = Client::connect(&args).await.err().expect("connect fails");
        let text = err.to_string();
        assert!(text.contains("http://127.0.0.1:1"), "{text}");
        assert!(text.contains("orcher dev start"), "{text}");
    }
}
