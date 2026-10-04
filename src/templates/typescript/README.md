# {{project_name}}

An ORCHER project in TypeScript: a `hello` workflow that runs a `greet` task.

```bash
npm install && npm run build
orcher dev start                 # a local engine, if you have none
npm run worker                   # the worker, in one terminal
npm run start-workflow -- Ada    # start the workflow, in another
orcher test                      # run it end to end
```

| File | What it is |
|------|------------|
| `src/workflows.ts` | The workflows and tasks |
| `src/worker.ts` | Runs them, polling the `{{task_queue}}` task queue |
| `src/start.ts` | Starts `hello` and prints its result |
| `src/test.ts` | Runs `hello` end to end and checks the result |
| `src/settings.ts` | The engine's address, namespace and API key, from the environment |
