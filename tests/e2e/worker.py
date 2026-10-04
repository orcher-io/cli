"""A worker with one workflow per thing the end-to-end test needs to see."""

import asyncio
import os
from datetime import timedelta

from orcher import TaskContext, Worker, WorkflowContext, task, workflow

SERVER_URL = os.environ.get("ORCHER_SERVER", "http://localhost:50051")
TASK_QUEUE = "cli-e2e"


@task(name="greet")
async def greet(ctx: TaskContext, name: str) -> str:
    return f"hello, {name}"


@workflow(name="hello")
async def hello(ctx: WorkflowContext, name: str) -> dict:
    """Runs one task and completes with a result."""
    greeting = await ctx.execute_task(greet, name=name)
    return {"greeting": greeting}


@workflow(name="greet_params")
async def greet_params(ctx: WorkflowContext, name: str, times: int = 1) -> dict:
    """Takes `orcher run`'s key=value parameters, which arrive as keyword
    arguments."""
    greeting = await ctx.execute_task(greet, name=name)
    return {"greeting": greeting, "times": times}


@workflow(name="approval")
async def approval(ctx: WorkflowContext) -> dict:
    """Waits for an `approve` event and completes with its payload."""
    decision = await ctx.wait_for_event("approve")
    return {"approved": decision}


@workflow(name="sleeper")
async def sleeper(ctx: WorkflowContext) -> str:
    """Sleeps for an hour: something to cancel or terminate."""
    await ctx.sleep(timedelta(hours=1))
    return "woke up"


@workflow(name="broken")
async def broken(ctx: WorkflowContext) -> str:
    """Fails straight away."""
    raise RuntimeError("this workflow always fails")


async def main() -> None:
    worker = (
        Worker.builder()
        .server_url(SERVER_URL)
        .namespace("default")
        .task_queue(TASK_QUEUE)
        .build()
    )
    print(f"worker polling {TASK_QUEUE} on {SERVER_URL}", flush=True)
    await worker.run()


if __name__ == "__main__":
    asyncio.run(main())
