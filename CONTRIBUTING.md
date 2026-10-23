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

Run these before opening a pull request; CI runs the same commands. Every
`run:` step in `.github/workflows/ci.yml` that is a test or a gate appears here —
`scripts/tests/assert-ci.sh governance-files` derives them from the workflow and
fails when this list falls behind.

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
bash scripts/tests/assert-ui.sh --strict
bash scripts/tests/assert-cargo.sh --strict
bash scripts/tests/assert-ci.sh --strict
cargo test --workspace --locked
npm --prefix agentj-ui ci
npm --prefix agentj-ui test --workspace=@agentj/protocol
npm --prefix agentj-ui test --workspace=@agentj/sdk
npm --prefix agentj-ui test --workspace=@agentj/tokens
npm --prefix agentj-ui test --workspace=@agentj/web
bun --cwd agentj-ui/apps/tui test
cargo deny check advisories licenses bans sources -D unmatched-skip
cargo llvm-cov --workspace --fail-under-lines 92
node agentj-ui/apps/desktop/check-desktop-ui.mjs
./scripts/check-version.sh
node scripts/check-relocate.mjs
```

The `scripts/tests/assert-*.sh` checks need `python3` with PyYAML and `cargo-deny`
(the CI `lint` job installs both before running them). Run them with `--strict`:
without it a check that cannot run reports SKIP and the run still exits 0, which is
how a missing `cargo-deny` used to hide the `deny-config` guard.

The coverage number is the CI ratchet (`.github/workflows/ci.yml` → `COVERAGE_THRESHOLD`),
not a suggestion: measured baseline `93.67%` on 2026-10-23, and the threshold only moves up.
Never lower it locally to make a run pass.

Protocol changes have two extra gates — regenerate and re-check for drift:

```bash
cargo run -p agentj-protocol --bin gen_schemas
cargo test -p agentj-protocol -- drift
npm --prefix agentj-ui run generate --workspace=@agentj/protocol
```

## Dependency and licence policy

`deny.toml` is the enforcement; this section is the rationale it points at.

- **Licence allowlist** — only these reach the dependency graph: `MIT`, `MIT-0`,
  `Apache-2.0`, `Apache-2.0 WITH LLVM-exception`, `BSD-2-Clause`, `BSD-3-Clause`,
  `ISC`, `Zlib`, `BSL-1.0`, `CC0-1.0`, `Unicode-3.0`, `Unlicense`, `MPL-2.0`.
  Anything else fails `cargo deny check licenses`; add a licence only after a
  deliberate compatibility review, never to unblock a build.
- **Yanked releases** — `advisories.yanked` stays at the cargo-deny default
  (`deny`): a yanked version must break the build, not warn about it.
- **Duplicate versions** — `bans.multiple-versions = "deny"` with an explicit
  skip list. Every skip must name a real duplicate in the graph and carry a
  comment saying why the duplication is cheaper than unifying it; an unmatched
  skip is a failure (`cargo deny check bans -D unmatched-skip` promotes the warning,
  and `scripts/tests/assert-cargo.sh deny-config` fails), so the list cannot rot.
- **Wildcard requirements** — `bans.wildcards = "deny"`; path dependencies
  still carry an explicit `version`, so `cargo publish` semantics stay honest.
- **Sources** — `sources.allow-registry` lists crates.io only; git and unknown
  registries are denied.
- **Dependabot** — `.github/dependabot.yml` groups minor and patch updates per
  ecosystem (cargo, npm, github-actions) and opens at most five PRs per
  ecosystem per week.

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
