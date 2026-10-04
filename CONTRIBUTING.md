# Contributing

Thanks for your interest in the ORCHER CLI. Bug reports, fixes and
improvements are welcome. For larger changes, please open an issue first so
we can agree on the approach before you spend time on it.

## Building and testing

You need a stable Rust toolchain.

```bash
cargo build
cargo test                  # unit tests, as CI runs them
cargo fmt --all --check     # CI fails on unformatted code
cargo clippy --all-targets -- -D warnings
```

The gRPC types come from the [`orcher-proto`](https://github.com/orcher-io/protos)
crate on crates.io.

## Pre-commit hooks

The repository uses [pre-commit](https://pre-commit.com) for fast local
checks. Install the hooks once per clone:

```bash
pipx install pre-commit      # or: brew install pre-commit / pip install pre-commit
pre-commit install           # installs the pre-commit and commit-msg git hooks
```

They then run on every `git commit`, against the changed files only. Run them
by hand with `pre-commit run --all-files`. CI runs the same hooks on every pull
request.

| Hook | What it does |
|------|--------------|
| trailing-whitespace, end-of-file-fixer, mixed-line-ending | whitespace hygiene |
| check-merge-conflict, check-added-large-files, check-yaml, check-toml | guardrails |
| detect-private-key, **gitleaks** | secret scanning |
| **typos** | spell-check (allowlist in `_typos.toml`) |
| **rustfmt** | checks formatting of changed Rust files |
| **conventional-pre-commit** | enforces `type(scope): subject` commit messages |

## Commit messages and releases

Commits follow [Conventional Commits](https://www.conventionalcommits.org):
`feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `ci:`, `chore:`. Mark a
breaking change with `!` (for example `feat!: ...`).

Releases are cut by release-please from these messages, so the type you pick
decides the next version and the changelog entry. While the CLI is below
1.0, `feat` and `fix` bump the patch version and a breaking change bumps the
minor version.

## Pull requests

- Keep each pull request focused on one change.
- Add or update tests for any behavior you change.
- Make sure `cargo fmt`, `cargo clippy` and `cargo test` pass locally.

## License

By contributing, you agree that your contributions are licensed under the
[Apache License, Version 2.0](LICENSE).
