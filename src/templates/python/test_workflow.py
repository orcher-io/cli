"""Runs the hello workflow end to end against the engine: starts a worker in
this process, starts the workflow, and checks its result.

    orcher test        # or: python test_workflow.py
"""

import asyncio
import sys

from start import run_hello
from worker import build_worker


async def main() -> int:
    worker = build_worker()
    running = asyncio.create_task(worker.run())
    try:
        result = await run_hello("test")
    finally:
        await worker.shutdown()
        running.cancel()
    if result != "Hello, test!":
        print(f"FAIL: hello returned {result!r}", flush=True)
        return 1
    print("ok: hello returned 'Hello, test!'", flush=True)
    return 0


if __name__ == "__main__":
    sys.exit(asyncio.run(main()))
