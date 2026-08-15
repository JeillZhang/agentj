# SDD Progress — v0.1 Core Implementation Plan

Plan: `docs/superpowers/plans/2026-08-09-agentj-v0.1-plan.md`
Branch: `feature/v0.1-core`
Started: 2026-08-15

## Task Status

| Task | Title | Status | Review |
|------|-------|--------|--------|
| T0 | Scaffold the Cargo workspace | ✅ done (repo already scaffolded, gates pass) | n/a (verify-only) |
| T1 | Create the agentj-core crate skeleton | ✅ done | PASS |
| T2 | Add shared types and error enums | pending | — |
| T3 | Add module traits, registries, and KernelContext | pending | — |
| T4 | Add the Kernel module registry, event loop, and lifecycle state machine | pending | — |
| T5 | Add the agentj-config crate with layered config merge | pending | — |
| T6 | Add the agentj-session crate with an in-memory session store | pending | — |
| T7 | Add the agentj-model crate with MockModelProvider | pending | — |
| T8 | Add the OpenAICompatibleProvider with SSE streaming, retry, tool-call assembly | pending | — |
| T9 | Add the agentj-tools crate with built-in tools | pending | — |
| T10 | Add the agentj-context crate with ContextPipeline | pending | — |
| T11 | Add AgentLoop with a streaming run_turn_stream API | pending | — |
| T12 | Add the agentj-mcp crate with an MCP client bridge | pending | — |
| T13 | Add the agentj-ipc crate with the Connection trait and JSON-RPC 2.0 over stdio | pending | — |
| T14 | Add the agentj-cli binary with run/serve commands (Connection model) | pending | — |
| T15 | Add benchmarks and sync docs for v0.1 | pending | — |
| T16 | Add ToolSource ownership contract to ToolRegistry | pending | — |
| T17 | Add session lifecycle contract implementation (archive/delete) | pending | — |
| T18 | Add session recovery contract (snapshot/restore/load_all) | pending | — |
| T19 | Add ToolExecutor with process-group isolation and orphan reclamation | pending | — |
| T20 | Add the ResponsesProvider (OpenAI Responses API) | pending | — |
| T21 | Add the AnthropicProvider (Anthropic Messages API) | pending | — |
| T22 | Add the opencode-go model catalog artifact and loader | pending | — |
| T23 | Add the OpenCodeGoProvider facade | pending | — |
| T24 | Add the ProviderFactory | pending | — |

## Execution Notes

- Execution mode: subagent-driven-development, fresh subagent per task.
- Reviewer gate: `ajteam-task-reviewer` after each task (3-round bounce cap).
- TDD enforced: test_code first, RED verified, then impl_code, GREEN verified.
- Commits: one per task, English messages.
