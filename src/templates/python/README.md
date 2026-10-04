# {{project_name}}

An ORCHER project in Python: a `hello` workflow that runs a `greet` task.

```bash
python3 -m venv .venv && .venv/bin/pip install -r requirements.txt
orcher dev start                 # a local engine, if you have none
.venv/bin/python worker.py       # the worker, in one terminal
.venv/bin/python start.py Ada    # start the workflow, in another
orcher test                      # run it end to end
```

| File | What it is |
|------|------------|
| `workflows.py` | The workflows and tasks |
| `worker.py` | Runs them, polling the `{{task_queue}}` task queue |
| `start.py` | Starts `hello` and prints its result |
| `test_workflow.py` | Runs `hello` end to end and checks the result |
| `settings.py` | The engine's address, namespace and API key, from the environment |
