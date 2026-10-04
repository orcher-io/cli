"""The worker: runs {{project_name}}'s workflows and tasks until stopped."""

import asyncio

import workflows  # noqa: F401  (registers the workflows and tasks)
from orcher import Worker
from settings import NAMESPACE, SERVER_URL, TASK_QUEUE


def build_worker() -> Worker:
    # The API key, when ORCHER_API_KEY is set, is read by the builder itself.
    return (
        Worker.builder()
        .server_url(SERVER_URL)
        .namespace(NAMESPACE)
        .task_queue(TASK_QUEUE)
        .build()
    )


async def main() -> None:
    worker = build_worker()
    print(f"worker polling {TASK_QUEUE} on {SERVER_URL}", flush=True)
    await worker.run()


if __name__ == "__main__":
    asyncio.run(main())
