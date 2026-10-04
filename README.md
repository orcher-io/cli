<p>
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/orcher-io/cli/main/assets/banner.svg">
    <source media="(prefers-color-scheme: light)" srcset="https://raw.githubusercontent.com/orcher-io/cli/main/assets/banner-light.svg">
    <img alt="ORCHER CLI" src="https://raw.githubusercontent.com/orcher-io/cli/main/assets/banner.svg" width="100%">
  </picture>
</p>

<p align="center"><sub>Run ORCHER on your machine, and start, watch and control workflows on any engine, from your terminal.</sub></p>

<br />

<div>
  <a href="https://github.com/orcher-io/cli/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/orcher-io/cli/ci.yml?branch=main&style=flat-square&labelColor=0a0a0a&color=04B385&logo=github&logoColor=white&label=CI" alt="CI"></a>
  <a href="./LICENSE"><img src="https://img.shields.io/badge/license-Apache_2.0-38BDF0?style=flat-square&labelColor=0a0a0a" alt="Apache 2.0"></a>
</div>

<br />

`orcher` works the same against a local engine, a self-hosted one, or ORCHER Cloud.

- <img height="14" src="https://octicons-col.vercel.app/server/38BDF0"> **Local engine**: `orcher dev start` runs the engine and its database in Docker, ready in seconds
- <img height="14" src="https://octicons-col.vercel.app/rocket/38BDF0"> **Run**: start a workflow with JSON input or `key=value` parameters, and wait for its result
- <img height="14" src="https://octicons-col.vercel.app/list-unordered/38BDF0"> **Inspect**: list, filter and search executions; see each one's status, result, journal, tasks and log
- <img height="14" src="https://octicons-col.vercel.app/stop/38BDF0"> **Control**: cancel or terminate a workflow, send it an event, or act on many at once
- <img height="14" src="https://octicons-col.vercel.app/globe/38BDF0"> **Cloud**: log in to ORCHER Cloud and keep a context per environment
- <img height="14" src="https://octicons-col.vercel.app/code/38BDF0"> **Scriptable**: `-o json`, `-o yaml` or `-o name`, and exit codes that mean something

<br />

### <img height="16" src="https://octicons-col.vercel.app/download/38BDF0"> Install

macOS and Linux:

```bash
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/orcher-io/cli/releases/latest/download/orcher-installer.sh | sh
```

Windows (PowerShell):

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/orcher-io/cli/releases/latest/download/orcher-installer.ps1 | iex"
```

Or with a package manager:

| | |
|---|---|
| Homebrew | `brew install orcher-io/tap/orcher` |
| Cargo | `cargo install orcher` |
| pip | `pip install orcher`, or run it without installing: `uvx orcher` |
| npm | `npm install -g orcher`, or run it without installing: `npx orcher` |

Every release has binaries for macOS (Apple silicon and Intel), Linux (x86_64 and arm64, glibc and musl) and Windows (x86_64) on its [release page](https://github.com/orcher-io/cli/releases), with checksums.

> [!NOTE]
> The CLI is pre-1.0: commands and flags may change between minor releases, and each release's notes list what changed.

<br />

### <img height="16" src="https://octicons-col.vercel.app/play/38BDF0"> Quick start

Start a local engine. It needs [Docker](https://docs.docker.com/get-docker/), and keeps its data until you delete it:

```bash
orcher dev start                           # the engine on localhost:50051
```

Run a worker against it (the [quickstart](https://github.com/orcher-io/quickstart) has one in Python, TypeScript and Rust), then:

```bash
orcher workflow start order --task-queue quickstart-python --id order-1001 --input '"order-1001"'
orcher workflow result order-1001          # wait for it, and print what it returned
orcher workflow list                       # recent workflow executions
orcher workflow get order-1001             # one of them in detail
orcher workflow history order-1001         # every step the engine recorded
orcher logs order-1001 --follow            # its log, until it ends
```

<br />

### <img height="16" src="https://octicons-col.vercel.app/terminal/38BDF0"> Commands

| Command | What it does |
|---------|--------------|
| `orcher dev start\|stop\|status\|logs` | Run a local engine in Docker: the quick local loop |
| `orcher server start\|stop\|restart` | The same local engine with more control: an engine config file (`--engine-config`), an existing database (`--database-url`), the log level, a graceful stop |
| `orcher server status\|version` | Whether an engine (and a gateway, when configured) answers, and which versions are running |
| `orcher workflow start <type>` | Start a workflow on `--task-queue`, with JSON `--input`; `--wait` for its result |
| `orcher workflow result <id>` | Wait for a workflow to finish and print its result |
| `orcher run <type>[@queue] -p key=value` | Start a workflow with parameters and follow it to the end |
| `orcher workflow list` | List executions; filter with `--type` and `--status`, or `--query` |
| `orcher workflow get <id>` | Status, timing, and result or error; `--full` adds pending tasks, timers and events |
| `orcher workflow history <id>` | The journal: every step the engine recorded |
| `orcher workflow tasks <id>` | The tasks the workflow ran, with attempts and durations |
| `orcher workflow event <id> <name>` | Send an event, with an optional JSON `--payload` |
| `orcher workflow cancel\|terminate <id>` | Ask a workflow to stop, or stop it at once |
| `orcher logs <id>` | A workflow's log; `--follow` until it ends, `--journal` or `--tasks` for the other views |
| `orcher status` | A dashboard: where the CLI points, what answers, and workflow counts |
| `orcher queue list\|stats` | Workflows by task queue |
| `orcher namespace ...` | `list`, `get`, `create`, `update`, `deprecate`, `delete` |
| `orcher batch ...` | Cancel, terminate, signal or reset many workflows at once, through a gateway |
| `orcher auth login\|logout\|status` | Log in to ORCHER Cloud (browser device flow, token, or password) |
| `orcher config ...` | Contexts: `get-contexts`, `use-context`, `set-context`, `view`, `test-connection` |
| `orcher new python\|typescript\|rust <name>` | A new project on the published SDK: a workflow, a task, a worker, a starter and an end-to-end test |
| `orcher new workflow <name>` | Add a workflow, with one task, to the project in this directory |
| `orcher test` | Check the project here builds (`--dry-run`), or run its workflows end to end on the engine |
| `orcher completion <shell>` | Shell completions for bash, zsh, fish, PowerShell and elvish |

Run `orcher <command> --help` for every flag and more examples.

<br />

### <img height="16" src="https://octicons-col.vercel.app/gear/38BDF0"> Where commands go

| Flag | Environment | Default | |
|------|-------------|---------|---|
| `--server` | `ORCHER_SERVER` | the context's, else `http://localhost:50051` | The engine's gRPC address; `https://` uses TLS |
| `--api-url` | `ORCHER_API_URL` | the context's `api`, if any | A gateway's HTTP address: login, batch operations, log streaming |
| `--context` | `ORCHER_CONTEXT` | the current context | A named environment from the config file |
| `--config` | `ORCHER_CONFIG` | `~/.orcher/config.yaml` | The config file |
| `-n`, `--namespace` | `ORCHER_NAMESPACE` | `default` | The namespace to act in |
| | `ORCHER_API_KEY`, `ORCHER_TOKEN` | the context's stored login | Credentials for an engine that requires them |
| | `ORCHER_CLOUD_URL` | `https://api.orcher.io` | Where ORCHER Cloud is |
| | `ORCHER_CREDENTIAL_STORE` | the OS keychain | `file` keeps logins in `credentials` beside the config file (owner-only), as happens anyway where there is no keychain |
| `-o`, `--output` | | `table` | `table`, `json`, `yaml` or `name` |

The local engine's ports are bound to `127.0.0.1`: it runs without authentication unless its config file turns it on. `orcher dev start` and `orcher server start` take `--engine-version` (default `0.5.5`) to run another engine release, and other ports when the defaults, 50051 and 8080, are taken.

A command that fails prints the reason to stderr and exits with status 1; so do `workflow result` and `run` for a workflow that failed, was canceled or terminated, or did not finish in time. Commands that would stop or delete something ask first; pass `--force` when there is no terminal to ask on.

<br />

### <img height="16" src="https://octicons-col.vercel.app/heart/38BDF0"> Contributing

Issues and pull requests are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md) for how to build, test and propose a change.

### <img height="16" src="https://octicons-col.vercel.app/law/38BDF0"> License

Licensed under the [Apache License, Version 2.0](LICENSE).
