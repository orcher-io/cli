# {{project_name}}

An ORCHER project in Rust: a `hello` workflow that runs a `greet` task.

```bash
orcher dev start                 # a local engine, if you have none
cargo run -- worker              # the worker, in one terminal
cargo run -- start Ada           # start the workflow, in another
orcher test                      # run it end to end
```

| File | What it is |
|------|------------|
| `src/workflows.rs` | The workflows and tasks |
| `src/main.rs` | The worker (`worker`, polling the `{{task_queue}}` task queue), the starter (`start`), and a test that runs `hello` end to end |
