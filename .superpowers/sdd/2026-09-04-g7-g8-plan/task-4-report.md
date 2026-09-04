# Task 4 Report — Same-port connection classifier + axum WS gateway in TcpIpcServer (G8)

- Task: T4 G8 — Connection classifier + axum Router in TcpIpcServer (same-port TCP+WS) [P2]
- Branch: feature/g7-g8-eventbus-websocket-gateway
- Commit: 944319c33b17a3d6a78d2f2c196cece3a031b007
- Date: 2026-09-04

## Changes
- `crates/agentj-ipc/src/tcp.rs`:
  - Module-level helper `fn looks_like_http(peeked: &[u8]) -> bool` (first byte is_ascii_uppercase → HTTP, else NDJSON).
  - `TcpIpcServer` struct: added `ws_state: Arc<WsServerState>`.
  - `bind()`: build shared `Arc<IpcServer>` ONCE (`let shared = Arc::new(server)`), clone into both `shared` and `ws_state.shared`; construct `WsServerState { shared, token: token.clone(), max_connections, active_ws: AtomicUsize::new(0) }`.
  - `run()` accept loop: `let (stream, peer)` captures peer; at spawn entry `stream.peek(&mut buf[16]).await.unwrap_or(0)` with MSG_PEEK, `if looks_like_http(&buf[..peeked])` → WS branch: `active.fetch_sub(1)` (release TCP slot), `ws_router(ws_state, peer)` + `serve_http(stream, router).await`; else existing TCP path with `TcpConnection::new` + shutdown signal. `active.fetch_add(1)` still at ACCEPT ENTRY before spawn; `fetch_sub(1)` in BOTH branches — no underflow. Added classifier unit test `looks_like_http_classifies_correctly`.
- `crates/agentj-ipc/src/ws.rs`:
  - Added imports: `axum::extract::{Query, ws::{WebSocketUpgrade}}`, `axum::http::StatusCode`, `axum::response::{IntoResponse, Response}`, `axum::routing::get`, `Router`, `hyper::{body::Incoming, service::service_fn}`, `hyper_util::rt::{TokioExecutor, TokioIo}`, `std::convert::Infallible`, `std::net::SocketAddr`, `std::sync::atomic::{AtomicUsize, Ordering}`, `tower::ServiceExt`.
  - `WsServerState { shared: Arc<IpcServer>, token: Option<String>, max_connections: usize, active_ws: AtomicUsize }`.
  - `WsQuery { token: Option<String> }` (Deserialize).
  - `pub fn ws_router(state: Arc<WsServerState>, peer: SocketAddr) -> Router`: per-connection router `/ws` → `ws_upgrade(state, query, peer, ws)`.
  - `async fn ws_upgrade(state, query, peer, ws) -> Response`: `?token=` check (403 if mismatch when token configured), `active_ws >= max_connections` check (503), `fetch_add(1)`, `ws.on_upgrade` → `WsConnection::new(socket, peer.to_string())` → `shared.serve_connection(Box::new(conn), token)` → `fetch_sub(1)`.
  - `pub async fn serve_http(stream, router)`: `TokioIo::new(stream)` + `service_fn` cloning router + `router.oneshot(req.map(Body::new))` + `hyper_util::server::conn::auto::Builder::new(TokioExecutor::new()).serve_connection_with_upgrades`.
- `crates/agentj-ipc/tests/ws_classifier.rs` (new, integration):
  - Helpers `initialize_line(id)` + `bind_server(token, max_connections)` (Kernel + InMemoryStore + TcpIpcServer::bind).
  - 4 tests: `same_port_serves_ws_and_raw_tcp` (WS initialize + raw TCP initialize on same port), `ws_bad_query_token_is_refused_at_upgrade` (403), `ws_initialize_without_token_fails_with_not_authorized` (-32806), `ws_max_connections_rejects_excess` (503).

## Tests
- TDD Step 2: `cargo test -p agentj-ipc --test ws_classifier` → 3 FAILED, 1 passed (`ws_bad_query_token_is_refused_at_upgrade` passed vacuously because raw-TCP path returns HttparseError, but the other 3 hit `Protocol(HttparseError(Version))` — WS upgrade not handled).
  - Failure log: `same_port_serves_ws_and_raw_tcp` / `ws_initialize_without_token_fails_with_not_authorized` / `ws_max_connections_rejects_excess` all panicked at `connect_async unwrap: Protocol(HttparseError(Version))`.
- TDD Step 4: after implementing ws.rs + tcp.rs classifier:
  - `cargo test -p agentj-ipc --test ws_classifier` → 4 passed
  - `cargo test -p agentj-ipc` → 48 passed, 0 failed (44 lib/tcp/ws/stdio + 4 classifier integration; previously 44 before T4)
  - Includes new `looks_like_http_classifies_correctly` unit test
- `cargo clippy -p agentj-ipc -- -D warnings` → PASS (0 warnings)
- `cargo fmt --all --check` → PASS (required one `cargo fmt --all` to normalize ws_classifier formatting)

## Deviations
- `tests/ws_classifier.rs`: added `use futures::SinkExt` (required for `ws.send()`) and used `tokio::io::AsyncWriteExt` only (removed unused `AsyncReadExt` import that clippy flagged); otherwise exact brief code (brief omitted SinkExt import — without it, `ws.send` fails E0599). All 4 test bodies match brief verbatim except import fix and fmt normalization.
- No functional deviation: `stream.peek` MSG_PEEK semantics preserved (does not consume bytes for TCP branch), `active` counter symmetry, WS excluded from last-disconnect shutdown (D7), `?token=` at upgrade + `initialize.token` recheck by `auth_gate` (-32806).

## Verification
- `cargo test -p agentj-ipc` PASS (48/48)
- `cargo clippy -p agentj-ipc -- -D warnings` PASS
- `cargo fmt --all --check` PASS

## Concerns
- None. Same-port classifier uses first-byte heuristic (uppercase → HTTP, `{` → NDJSON) which is correct per protocol; no wss/TLS, CORS, or extra rate-limit beyond 503 added.
