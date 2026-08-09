# AGENTS.md

> Placeholder — last updated 2026-08-09. Update as the project takes shape.

## Project Status

This repository is newly initialized and has no source code, builds, tests, or conventions yet.

## Setup

_No setup steps available. Add them once tooling (package manager, build system, etc.) is configured._

## Commands

_No build/test/lint commands yet._

## Architecture

_No code to document._

## Conventions

_No conventions established yet. When they emerge, capture them here — especially anything that differs from language/framework defaults._

## Rules

- Commit messages in English. `docs/superpowers/specs/` docs in Chinese.
- Docs under `docs/` are tracked deliverables — commit them.
- Bump version on every feature (`0.1.0` → `0.2.0`) and bugfix (`0.1.0` → `0.1.1`).
- Before planning/developing, read `.claude/summary/summary_and_next.md` and `docs/superpowers/` first.
- All diagrams in spec docs under `docs/superpowers/specs/` must use UML or Mermaid modeling diagrams — no other diagram formats.
- Update `docs/superpowers/specs` target spec docs TODO list after each phase.
- `.claude/summary/summary_and_next.md` only retains content for the **currently developing** feature design document: its progress, summary, remaining issues, and next steps. When a new feature starts, replace (don't append) the previous feature's content.
- After each plan completes, update `.claude/summary/summary_and_next.md` with the current feature's:
  - Progress and summary of this phase
  - Any remaining issues or known limitations
  - The next plan / next steps
- After each feature phase completes AND `cargo test --workspace` + `cargo clippy --workspace -- -D warnings` pass, automatically:
  - Update `AGENTS.md`: sync version, migrations count, new API routes, architecture notes, config vars, crate boundaries — any structural change introduced in the phase.
  - Update `README.md`: sync version tagline, Features checklist, Project Structure tree, API endpoint tables, migrations count, and config var table — any user-facing change introduced in the phase.
  - Update `.claude/summary/summary_and_next.md`: rewrite it with the current feature's "## {phase}: {title} ✅ (本次完成)" progress, verification results, remaining issues, and next steps — keeping only content for the currently developing feature design document, and updating the "## 待实施" table if needed.
  - Commit the three files together as `docs: auto-sync AGENTS.md, README.md, summary after {phase} completion`.
  - Do NOT wait for the user to request this — it is a mandatory post-phase step.
- Clean up temp files, Docker containers after each development phase.
- When a command hangs >120s, cancel and retry.
- PR creation: create the PR and share the link — the user handles merge.
- Rust function or method should less than 50 lines, and <= 120 characters for each line.
