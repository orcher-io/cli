//! The workflows and tasks of {{project_name}}.
//!
//! A workflow decides what happens and in what order; it must be
//! deterministic, so anything that touches the outside world (an API, a
//! database, the clock) goes in a task.

use orcher_sdk::prelude::*;

// Tasks are where side effects go: call an API or write to a database here.
#[task]
pub async fn greet(_ctx: TaskContext, name: String) -> Result<String> {
    Ok(format!("Hello, {name}!"))
}

#[workflow(name = "hello")]
pub async fn hello(ctx: WorkflowContext, name: String) -> Result<String> {
    ctx.execute_task(greet, name).await
}
