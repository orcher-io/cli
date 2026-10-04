//! {{project_name}}: run the worker, or start the hello workflow.
//!
//!   cargo run -- worker          run the workflows and tasks until stopped
//!   cargo run -- start Ada       start `hello` and print its result
//!   orcher test                  run it end to end (or: cargo test)

mod workflows;

use orcher_sdk::client::StartWorkflowOptions;
use orcher_sdk::prelude::*;

const TASK_QUEUE: &str = "{{task_queue}}";

/// Where the engine is. `orcher` sets these for `orcher test`; export them
/// yourself to point the worker and the starter elsewhere.
fn server_url() -> String {
    std::env::var("ORCHER_SERVER").unwrap_or_else(|_| "http://localhost:50051".to_string())
}

fn namespace() -> String {
    std::env::var("ORCHER_NAMESPACE").unwrap_or_else(|_| "default".to_string())
}

fn api_key() -> Option<String> {
    std::env::var("ORCHER_API_KEY")
        .ok()
        .filter(|k| !k.is_empty())
}

async fn build_worker() -> Result<Worker> {
    let mut builder = Worker::builder()
        .server_url(server_url())
        .namespace(namespace())
        .task_queue(TASK_QUEUE);
    if let Some(key) = api_key() {
        builder = builder.api_key(key);
    }
    builder.build().await
}

async fn run_hello(name: &str) -> Result<String> {
    let mut client = Client::connect(server_url()).await?;
    if let Some(key) = api_key() {
        client = client.with_api_key(key);
    }
    let id = uuid::Uuid::new_v4().simple().to_string();
    let options = StartWorkflowOptions::new(TASK_QUEUE)
        .with_namespace(namespace())
        .with_workflow_id(format!("hello-{}", &id[..8]));
    let handle = client
        .start_workflow_with_options("hello", name.to_string(), options)
        .await?;
    handle.result().await
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("start") => {
            let name = args.get(1).map(String::as_str).unwrap_or("world");
            println!("{}", run_hello(name).await?);
            Ok(())
        }
        _ => {
            let worker = build_worker().await?;
            println!("worker polling {TASK_QUEUE} on {}", server_url());
            worker.run().await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// Runs `hello` end to end against the engine: a worker in this process,
    /// the workflow started through a client, and its result checked.
    #[tokio::test]
    async fn hello_greets_by_name() -> Result<()> {
        let worker = Arc::new(build_worker().await?);
        let running = tokio::spawn({
            let worker = worker.clone();
            async move { worker.run().await }
        });
        let result = run_hello("test").await;
        worker.shutdown();
        let _ = running.await;
        let result = result?;
        assert_eq!(result, "Hello, test!");
        println!("ok: hello returned {result:?}");
        Ok(())
    }
}
