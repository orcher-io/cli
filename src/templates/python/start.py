"""The starter: start the hello workflow and wait for its result.

    python start.py Ada
"""

import asyncio
import sys
import uuid

from orcher import Client, ClientConfig
from settings import API_KEY, NAMESPACE, SERVER_URL, TASK_QUEUE


async def run_hello(name: str) -> str:
    config = ClientConfig(server_url=SERVER_URL, namespace=NAMESPACE, api_key=API_KEY)
    async with Client(config) as client:
        handle = await client.start_workflow(
            "hello",
            task_queue=TASK_QUEUE,
            workflow_id=f"hello-{uuid.uuid4().hex[:8]}",
            args=(name,),
        )
        return await handle.result(timeout=60)


async def main() -> None:
    name = sys.argv[1] if len(sys.argv) > 1 else "world"
    print(await run_hello(name), flush=True)


if __name__ == "__main__":
    asyncio.run(main())
