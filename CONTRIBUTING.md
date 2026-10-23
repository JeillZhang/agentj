# Contributing to AgentJ

Thanks for taking the time to contribute. This document covers the parts that are
easy to get wrong: the local gates, the branch/commit conventions, and what must
never be committed.

## Prerequisites

- Rust 1.96+ (stable) — `rustup toolchain install stable`
- Node 24+ and npm 11 (`npm --version`) — the UI is an npm workspace
- Bun ≥ 1.2 — only for `agentj-ui/apps/tui`
- Optional: `cargo install cargo-deny cargo-llvm-cov`

## Local gates

Run these before opening a pull request; CI runs the same commands.

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
npm --prefix agentj-ui ci
npm --prefix agentj-ui test --workspace=@agentj/sdk
npm --prefix agentj-ui test --workspace=@agentj/web
npm --prefix agentj-ui test --workspace=@agentj/protocol
bun --cwd agentj-ui/apps/tui test
cargo deny check advisories licenses bans sources
cargo llvm-cov --workspace --fail-under-lines 92
./scripts/check-version.sh
```

The coverage number is the CI ratchet (`.github/workflows/ci.yml` → `COVERAGE_THRESHOLD`),
not a suggestion: measured baseline `93.67%` on 2026-10-23, and the threshold only moves up.
Never lower it locally to make a run pass.

Protocol changes have two extra gates — regenerate and re-check for drift:

```bash
cargo run -p agentj-protocol --bin gen_schemas
cargo test -p agentj-protocol -- drift
npm --prefix agentj-ui run generate --workspace=@agentj/protocol
```

## Branches and commits

- Branch from `main`: `feat/<topic>`, `fix/<topic>`, `chore/<topic>`, `docs/<topic>`.
- One logical change per commit; keep commits reviewable on their own.
- Commit messages are English, imperative mood, and may use Conventional Commits
  style (`fix(ipc): …`).
- Pull requests are opened against `origin` (GitHub). The GitCode remote is a
  read-only mirror — never open a merge request there.
- Do not bump `version` in `Cargo.toml`, `agentj-ui/packages/sdk/src/config.ts`
  or the desktop manifests unless a maintainer asked for a release.

## What not to commit

- `docs/superpowers/` — local design/plan working documents (git-ignored).
- `node_modules/`, build output (`target/`, `agentj-ui/apps/web/dist/`), and any
  editor or agent runtime state (`.opencode/`, `.omo/`, `.playwright-mcp/`).
- Secrets, tokens or credentials of any kind. Configuration examples belong in
  `docs/guide/`, never in a committed `auth.json`.

## Protocol changes

`PROTOCOL_VERSION`, `protocol/methods.json` and `schemas/*.json` move together.
Wire fields are `snake_case`, additions are additive-only, and unknown enum
values must stay tolerated. See [`docs/specs/ipc-protocol.md`](docs/specs/ipc-protocol.md).

## Reporting bugs and security issues

Functional bugs and feature requests: open a regular issue with a minimal
reproduction (command, config, output). Security problems: see
[`SECURITY.md`](SECURITY.md) — do not file them publicly.
