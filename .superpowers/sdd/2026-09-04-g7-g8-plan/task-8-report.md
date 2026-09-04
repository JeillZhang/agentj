# T8 Report — Docs: WebSocket transport section + gap archive G7/G8 → 已实现

## Status
Done

## Files
- `docs/specs/ipc-protocol.md` — §2.3 expanded from 未来预留 to v0.10.0 (ws://127.0.0.1:<port>/ws, native Text frames, dual-channel auth ?token= + initialize.token -32806, lifecycle exclusion, max_connections independent)
- `docs/superpowers/specs/2026-08-30-gap-analysis-vs-original-design.md` — G7/G8 rows flipped to ✅ 已实现 (v0.10.0, design+plan links), plus G7/G8 bullets in §G2-G8 updated

## Verify
- `cargo test -p agentj-protocol -- drift` → ok (2 passed, schemas unchanged)
- `git diff --stat docs/` → 2 files, 11+/6- ; grep confirms G7 已实现(v0.10.0 EventGuard depth<16) and G8 已实现(v0.10.0 同端口分类器 + axum /ws + WebSocketTransport)

## Commit
pending (see git log)
