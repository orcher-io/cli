"""The workflows and tasks of {{project_name}}.

A workflow decides what happens and in what order; it must be deterministic,
so anything that touches the outside world (an API, a database, the clock)
goes in a task.
"""

from orcher import TaskContext, WorkflowContext, task, workflow


@task(name="greet")
async def greet(ctx: TaskContext, name: str) -> str:
    # Tasks are where side effects go: call an API or write to a database here.
    return f"Hello, {name}!"


@workflow(name="hello")
async def hello(ctx: WorkflowContext, name: str) -> str:
    return await ctx.execute_task(greet, name=name)
