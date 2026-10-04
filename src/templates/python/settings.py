"""Where the engine is. `orcher` sets these for `orcher test`; export them
yourself to point the worker and the starter elsewhere."""

import os

SERVER_URL = os.environ.get("ORCHER_SERVER", "http://localhost:50051")
NAMESPACE = os.environ.get("ORCHER_NAMESPACE", "default")
API_KEY = os.environ.get("ORCHER_API_KEY") or None
TASK_QUEUE = "{{task_queue}}"
