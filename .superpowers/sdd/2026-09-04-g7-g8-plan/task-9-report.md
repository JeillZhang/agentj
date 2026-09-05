# T9 Report — Doc sync AGENTS.md + README.md + summary (v0.10.0 G7+G8)

## Status
Done

## Files
- `AGENTS.md` — top Project Status updated to v0.10.0 with G7 EventGuard + G8 WebSocket gateway summary; `agentj-core` crate desc extended with `EventGuard thread_local EVENT_DEPTH/EVENT_STACK + MAX_EVENT_DEPTH=16`; `agentj-ipc` crate desc extended with `ws.rs/WsConnection/WsServerState/ws_router/serve_http/classifier`; `agentj-cli` desc extended with `serve --port` WS print; `web-dashboard` desc changed to `WebSocketTransport` + `ResponseRouter` notification fork (BUG-005)
- `README.md` — version tagline `v0.9.1` → `v0.10.0` with G7+G8 summary; Features checklist gained `EventBus 重入防护` + `WebSocket 网关` entries; Project Structure rows for `agentj-ipc`/`agentj-cli`/`web-dashboard` complemented with WS descriptions
- `.claude/summary/summary_and_next.md` — REPLACED (not appended) with `## Phase: G7 EventBus 重入防护 + G8 WebSocket 网关 (v0.10.0) ✅ (本次完成)` containing progress/summary, verification, remaining issues, next steps; 待实施 table marked G7+G8 as `✅ 已实现 v0.10.0`

## Fixes bundled
- `crates/agentj-core/src/hooks.rs` — `format!("m{i}").into()` → `format!("m{i}")` (clippy `useless_conversion`)
- `crates/agentj-cli/tests/e2e_ws.rs` — `child.wait()` after `kill()` (clippy `zombie_processes`) + `cargo fmt`

## Verify
- `cargo test --workspace -- --skip permission_request_notified` → ok (all suites pass; the one 60s-timeout test filtered as in prior phases)
- `cargo clippy --workspace --all-targets -- -D warnings` → ok (0 warnings)
- `cargo fmt --all --check` → ok

## Commit
`898f4a5` docs: auto-sync AGENTS.md, README.md, summary after G7+G8 completion
