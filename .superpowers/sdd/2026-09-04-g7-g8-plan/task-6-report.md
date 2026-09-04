# T6 Report — CLI e2e: serve --port dual transport + WS print (G8)

## Changes
- **Created** `crates/agentj-cli/tests/e2e_ws.rs` — `serve_port_accepts_ws_clients_and_keeps_tcp_attach` spawns `agentj serve --port <p>` via `env!("CARGO_BIN_EXE_agentj")`, waits for listen, WS initialize over `ws://127.0.0.1:{port}/ws` (contains connection_id), TCP attach NDJSON initialize (regression), and D7 daemon-liveness after WS-only disconnect.
- **Modified** `crates/agentj-cli/src/main.rs` — `serve_tcp()` now prints `websocket on ws://127.0.0.1:{port}/ws` after `listening on ...`.
- **Modified** `crates/agentj-cli/Cargo.toml` — added dev-deps `futures`, `tokio` (rt-multi-thread/macros/net/io-util), `tokio-tungstenite` needed for e2e_ws compilation.
- **Modified** `Cargo.lock` — locked new dev deps.

## Deviations from Brief
- **dev-deps addition**: Brief lists 2 files but e2e_ws compilation requires `futures` + `tokio-tungstenite` dev-dependencies; added to `crates/agentj-cli/Cargo.toml` (and Cargo.lock).
- **SinkExt import**: Added `SinkExt` to `futures` import for `ws.send()` (trait must be in scope).
- **Unused import**: Removed `AsyncReadExt` (unused) to keep clippy clean.
- **child.kill()**: Brief shows `child.kill().await` but `std::process::Child::kill` is synchronous; fixed to `child.kill()` (await removed). Alternative would be `tokio::process::Command` but kept `std::process::Command` as briefed.

## Test Summary
- TDD step 2: `cargo test -p agentj-cli --test e2e_ws` before fix → FAIL (missing crates + trait + kill await).
- Final `cargo test -p agentj-cli --test e2e_ws` → **1 passed** (`serve_port_accepts_ws_clients_and_keeps_tcp_attach`).
- `cargo test -p agentj-cli --test e2e_cli` → **2 passed** (existing TCP/static attach regression green).
- `cargo test -p agentj-cli --bins` → **16 passed** (unit tests).
- `cargo clippy -p agentj-cli -- -D warnings` → PASS (0 warnings).

## Build Result
- `cargo test -p agentj-cli --test e2e_ws && cargo test -p agentj-cli --test e2e_cli && cargo clippy -p agentj-cli -- -D warnings` — all PASS.

## Commit
- `b2626fadcc4bd2238102108cb3390aeeb4737b2c` — `test(cli): e2e dual-transport serve --port (WS + TCP attach) (G8)`

## Concerns
- Brief's `child.kill().await` is incorrect for `std::process::Child`; fixed to sync kill. If future refactors switch to `tokio::process::Command`, async kill would be needed.
- Adding dev-deps to Cargo.toml exceeds the brief's "2 files" but is necessary for compilation; consider updating brief to include Cargo.toml.
