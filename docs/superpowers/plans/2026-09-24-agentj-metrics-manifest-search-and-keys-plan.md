# Plan: AgentJ v0.22.0 — Metrics Export, Asset Manifest Extension, TUI Server-side Search & Web Keyboard Consolidation

> **Source spec:** `docs/superpowers/specs/2026-09-24-agentj-metrics-manifest-search-and-keys-design.md`
> **Version:** workspace `0.21.1` → **`0.22.0`**; `PROTOCOL_VERSION` `0.13.0` → **`0.14.0`** (three additive wire changes)
> **Language:** task titles / ACs / code in English (per jskill-feature-plan); context notes in Chinese.

## Agent Worker Instructions

- **Required sub-skills:** `superpowers:subagent-driven-development` (recommended) or `superpowers:executing-plans`
- **Execution mode:** `subagent-driven-development` (fresh subagent per task, review between tasks)
- **Required verification (final; each task's `verify` field is authoritative for that task):**
  - `cargo fmt --check` · `cargo test --workspace` · `cargo clippy --workspace --all-targets -- -D warnings`
  - `cargo test -p agentj-protocol -- drift` (2 passed)
  - `npm --prefix agentj-ui test --workspace=@agentj/sdk`
  - `npm --prefix agentj-ui test --workspace=@agentj/web`
  - `bun --cwd agentj-ui/apps/tui test`
  - `npm --prefix agentj-ui run build --workspace=@agentj/web`
  - `./scripts/check-version.sh` · `node agentj-ui/apps/desktop/check-desktop-ui.mjs`
- **Version bump:** feature → minor (`0.22.0`); `PROTOCOL_VERSION` → `0.14.0` (additive wire, see Global Constraints).
- **Live acceptance:** deferred to the acceptance stage (`jskill-test-acceptance`); automated gates are this plan's exit criteria.
- **Commits:** English messages, one logical change per commit (`feat:`, `fix:`, `chore:`, `docs:`).

## Global Constraints

1. **Wire additive discipline:** only optional fields with `#[serde(default, skip_serializing_if = "Option::is_none")]`; no `deny_unknown_fields`; unknown fields stay tolerated; non-query `file/list` behavior and `/version` existing fields stay byte-identical.
2. **No new third-party dependencies:** Prometheus exposition is hand-formatted; `sha2`/`base64`/`tokio`/`uuid` already in `agentj-ipc`; zero new npm packages.
3. **Rust style:** functions ≤50 lines, ≤120 chars/line; existing error-code discipline (`-32602`/`-32807`/`-32803`) unchanged.
4. **`breaking: true` semantics:** marks Rust API surface changes (pub struct field additions force literal updates). The wire contract remains additive and backward compatible — never confuse this with wire breakage.
5. **Web a11y:** menus adopt roving tabindex (`0/-1`) + Home/End as designed (explicit behavior change: Tab leaves the menu); sidebar shortcut scope unchanged (Tab stays native outside menus); `ComposerOptions` listbox untouched.
6. **TUI:** persistence goes through `src/config.ts`; search `Esc`/`r`/`Enter` semantics and hint strings unchanged; `XDG_CONFIG_HOME` semantics unchanged.
7. **Generated artifacts:** `schemas/*.json` regenerated via `cargo run -p agentj-protocol --bin gen_schemas`; TS types via `npm --prefix agentj-ui run generate --workspace=@agentj/protocol`. Regen tasks are ordered so each run only rewrites the files listed in that task.
8. **Compile-green boundaries:** protocol field additions fix every constructor site in the same task; the one intentional red window (`FileListResult.total` before its schema regen) is resolved by the immediately following task T8.

---

## Tasks

### T0: Add snapshot() per-IP projection to LoginLimiter [P0] [Sequential]

```yaml
- id: T0
  title: "Add IpSnapshot and a sorted snapshot(now) projection to LoginLimiter"
  files:
    - crates/agentj-ipc/src/login_limiter.rs
  depends_on: []
  breaking: false
  ac:
    - given: "a limiter (free_attempts=1) with one locked IP and one failed IP"
      when: "snapshot(now) is read"
      then: "entries are sorted locked-desc → failures-desc → ip-asc; the locked entry carries retry_after > 0 and the other carries retry_after = 0"
    - given: "an empty limiter"
      when: "snapshot(now) is read"
      then: "the vector is empty"
  regression:
    - "login_limiter::tests::counters_track_attempts_failures_lockouts_and_blocks"
    - "login_limiter::tests::locks_on_fifth_failure_then_escalates_to_the_cap"
  test_code: |
    #[test]
    fn snapshot_sorts_locked_first_then_failures_then_ip() {
        // free_attempts = 2: the 2nd failure locks, so locked_ip ends locked (failures=2)
        // while busy_ip stays allowed (failures=1) — a deterministic rank difference.
        let limiter = LoginLimiter::with_policy(Policy { free_attempts: 2, ..Policy::default() });
        let t0 = Instant::now();
        let locked_ip = ip("203.0.113.9");
        let busy_ip = ip("198.51.100.4");
        let _ = limiter.record_failure(locked_ip, t0);
        let _ = limiter.record_failure(locked_ip, t0); // locks (free_attempts = 2)
        let _ = limiter.record_failure(busy_ip, t0);   // still allowed
        let snap = limiter.snapshot(t0);
        assert_eq!(snap.len(), 2);
        assert_eq!(snap[0].ip, locked_ip);
        assert!(snap[0].locked);
        assert!(snap[0].retry_after > Duration::ZERO);
        assert_eq!(snap[1].ip, busy_ip);
        assert!(!snap[1].locked);
        assert_eq!(snap[1].retry_after, Duration::ZERO);
        assert!(limiter.snapshot(t0).iter().all(|s| s.last_seen == t0));
    }
  impl_code: |
    // login_limiter.rs additions
    #[derive(Debug, Clone, Copy)]
    pub(crate) struct IpSnapshot {
        pub(crate) ip: IpAddr,
        pub(crate) failures: u32,
        pub(crate) locked: bool,
        pub(crate) retry_after: Duration,
        pub(crate) last_seen: Instant,
    }
    impl LoginLimiter {
        /// Read-only projection under the existing entries lock (≤ max_tracked, no await).
        pub(crate) fn snapshot(&self, now: Instant) -> Vec<IpSnapshot> {
            let entries = self.entries.lock().expect("limiter lock");
            let mut out: Vec<IpSnapshot> = entries
                .iter()
                .map(|(ip, entry)| {
                    let locked = matches!(self.status(entry, now), Throttle::Locked { .. });
                    let retry_after = match self.status(entry, now) {
                        Throttle::Locked { retry_after } => retry_after,
                        Throttle::Allow => Duration::ZERO,
                    };
                    IpSnapshot { ip: *ip, failures: entry.failures, locked, retry_after, last_seen: entry.last }
                })
                .collect();
            out.sort_by(|a, b| {
                b.locked.cmp(&a.locked)
                    .then(b.failures.cmp(&a.failures))
                    .then(a.ip.cmp(&b.ip))
            });
            out
        }
    }
  verify:
    - "cargo test -p agentj-ipc login_limiter"
    - "cargo clippy -p agentj-ipc --all-targets -- -D warnings"
```

### T1: Add auth-gated GET /metrics with Prometheus text [P0] [Sequential]

```yaml
- id: T1
  title: "Add auth_required() and GET /metrics (Prometheus exposition) to the web surface"
  files:
    - crates/agentj-ipc/src/web.rs
  depends_on: [T0]
  breaking: false
  ac:
    - given: "a token-configured server and a remote peer without a session cookie"
      when: "GET /metrics is requested"
      then: "401 text/plain with Cache-Control no-store; no per-IP or aggregate values are leaked"
    - given: "a loopback peer, a valid session cookie, or a token-less server"
      when: "GET /metrics is requested"
      then: "200 with Content-Type text/plain; version=0.0.4 and Cache-Control no-store"
    - given: "the limiter saw failures from two IPs, one locked"
      when: "the body is read"
      then: "it contains # TYPE agentj_login_attempts_total counter, agentj_login_ip_failures_total{ip=\"...\"} and agentj_login_ip_locked{ip=\"...\"} 1"
    - given: "more than 50 tracked IPs"
      when: "the body is read"
      then: "per-IP series are capped at 50 (locked first, then failures desc, then ip asc) and agentj_login_ip_series_truncated is 1"
    - given: "a dist whose manifest verified cleanly at startup"
      when: "the body is read"
      then: "it contains agentj_web_manifest_present 1 and agentj_web_assets_checked <n>"
  regression:
    - "web::tests::remote_peer_without_cookie_is_redirected_to_login"
    - "web::tests::loopback_and_cookie_paths_pass_the_gate"
    - "web::tests::version_reports_login_limiter_counters"
  test_code: |
    #[tokio::test]
    async fn metrics_gates_remote_peers_and_formats_prometheus() {
        let state = test_state(crate::web::WebConfig::new(None, Some("s3cret".into())));
        let remote: SocketAddr = "203.0.113.7:5555".parse().unwrap();
        let (status, headers, body) =
            get(crate::web::build_router(state.clone(), remote), "/metrics").await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert_eq!(headers["cache-control"], "no-store");
        assert!(!body.contains("agentj_login_attempts_total"));

        let local: SocketAddr = "127.0.0.1:4444".parse().unwrap();
        let (status, headers, body) = get(crate::web::build_router(state, local), "/metrics").await;
        assert_eq!(status, StatusCode::OK);
        assert!(headers["content-type"].to_str().unwrap().starts_with("text/plain"));
        assert_eq!(headers["cache-control"], "no-store");
        assert!(body.contains("# TYPE agentj_login_attempts_total counter"));
    }
    #[tokio::test]
    async fn metrics_exposes_the_startup_integrity_summary() {
        let dist = temp_dist("metrics-integrity");
        write_temp_dist_manifest(&dist);
        let state = test_state(crate::web::WebConfig::new(Some(dist), None));
        let local: SocketAddr = "127.0.0.1:4444".parse().unwrap();
        let (status, _, body) = get(crate::web::build_router(state, local), "/metrics").await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("agentj_web_manifest_present 1"));
        assert!(body.contains("agentj_web_assets_checked 2"));
    }
  impl_code: |
    // web.rs additions
    pub const PER_IP_SERIES_LIMIT: usize = 50;

    /// Auth gate without the dist requirement (machine surface; static surface keeps gate_required).
    pub fn auth_required(web: &WebConfig, peer: SocketAddr, headers: &HeaderMap) -> bool {
        web.token.is_some()
            && !peer_is_loopback(peer)
            && !session_cookie(headers).is_some_and(|sid| web.is_valid_session(sid))
    }
    pub fn gate_required(web: &WebConfig, peer: SocketAddr, headers: &HeaderMap) -> bool {
        web.enabled() && auth_required(web, peer, headers)
    }

    // build_router: register before auth_routes/static (explicit route beats SPA fallback)
    //   .route("/metrics", get(move |headers: HeaderMap| {
    //       let (web, peer) = (web.clone(), peer);
    //       async move { metrics_handler(web, peer, headers).await }
    //   }))
    async fn metrics_handler(web: Arc<WebConfig>, peer: SocketAddr, headers: HeaderMap) -> Response {
        if auth_required(&web, peer, &headers) {
            return (StatusCode::UNAUTHORIZED,
                    [("content-type", "text/plain"), ("cache-control", "no-store")],
                    "unauthorized\n").into_response();
        }
        let body = prometheus_text(&web);
        // 200 + Content-Type: text/plain; version=0.0.4; charset=utf-8 + Cache-Control: no-store
    }
    fn prometheus_text(web: &WebConfig) -> String {
        // counters() + snapshot(now) + integrity() summary (manifest_present / checked /
        // mismatched / missing); spot counters are appended in T3 once SpotCheckReport exists.
        // HELP/TYPE lines; sort by (locked desc, failures desc, ip asc); cap at
        // PER_IP_SERIES_LIMIT; agentj_login_ip_series_truncated = truncated as 0/1.
    }
  verify:
    - "cargo test -p agentj-ipc web"
    - "cargo clippy -p agentj-ipc --all-targets -- -D warnings"
```

### T2: Include dist passthrough files (public/) in asset-manifest.json [P0] [Parallel]

```yaml
- id: T2
  title: "Merge dist directory files (public/ passthrough) into asset-manifest.json at build time"
  files:
    - agentj-ui/apps/web/build/version-sri.ts
    - agentj-ui/apps/web/build/version-sri.test.ts
  depends_on: []
  breaking: false
  ac:
    - given: "a dist with index.html, assets/a.js and a public passthrough favicon.svg"
      when: "finalizeDist runs"
      then: "the manifest keys are exactly ['/assets/a.js','/favicon.svg','/index.html'] (sorted) and the favicon digest is sha384 of its bytes"
    - given: "hidden files (.DS_Store) or excluded files (version.json, asset-manifest.json) in dist"
      when: "the directory is enumerated"
      then: "they are not added to the manifest"
    - given: "a bundle output and a dist file sharing a key"
      when: "hashes are merged"
      then: "the bundle hash wins"
    - given: "a dist file that cannot be read during enumeration"
      when: "finalizeDist runs"
      then: "the build fails (throw), mirroring the missing SRI reference behavior"
  regression:
    - "build/version-sri.test.ts: writes a manifest covering bundle files and the rewritten index.html"
    - "build/version-sri.test.ts: excludes version.json and asset-manifest.json from the manifest"
    - "build/version-sri-plugin.test.ts: keeps version.json and asset-manifest.json out of the manifest"
  test_code: |
    it("includes dist root files such as public passthrough in the manifest", () => {
      const dist = mkdtempSync(join(tmpdir(), "aj-sri-public-"));
      writeFileSync(join(dist, "index.html"), '<script src="/assets/a.js"></script>');
      mkdirSync(join(dist, "assets"), { recursive: true });
      writeFileSync(join(dist, "assets/a.js"), "console.log(1)");
      writeFileSync(join(dist, "favicon.svg"), "<svg/>");
      writeFileSync(join(dist, ".DS_Store"), "junk");
      finalizeDist(dist, { productVersion: "0.22.0", protocolVersion: "0.14.0", buildTime: "t" }, ["assets/a.js"]);
      const manifest = JSON.parse(readFileSync(join(dist, "asset-manifest.json"), "utf8"));
      expect(Object.keys(manifest.assets).sort()).toEqual(["/assets/a.js", "/favicon.svg", "/index.html"]);
      expect(manifest.assets["/favicon.svg"]).toBe(integrityOf(Buffer.from("<svg/>")));
      expect(manifest.assets["/.DS_Store"]).toBeUndefined();
    });
  impl_code: |
    // version-sri.ts: inside finalizeDist (signature unchanged: (distDir, v, bundleFiles)),
    // after bundleHashes are computed, enumerate dist recursively (skip hidden names,
    // EXCLUDED_FILES, "/index.html" and keys already hashed) and merge their sha384 into the
    // map before injectSri/buildManifestJson. Plugin call site unchanged; publicDir is NOT
    // needed (Vite 8 copies public/ into dist during renderStart/pre — verified in node_modules/vite).
    // Also `export` the existing `integrityOf` helper so the test can recompute expected digests
    // (extend the test-file import list accordingly). An unreadable entry → readFileSync throws
    // → build fails, matching the missing-SRI-reference behavior.
  verify:
    - "npm --prefix agentj-ui test --workspace=@agentj/web -- build/version-sri"
    - "npm --prefix agentj-ui test --workspace=@agentj/web -- build/version-sri-plugin"
```

### T3: Retain the manifest and add spot-check core to WebConfig [P0] [Sequential]

```yaml
- id: T3
  title: "Retain AssetManifest in WebConfig and add run_spot_round with the /version runtime block"
  files:
    - crates/agentj-ipc/src/web.rs
  depends_on: [T1]
  breaking: false
  ac:
    - given: "a dist with 3 manifest entries and sample=1"
      when: "run_spot_round() is called three times"
      then: "each round checks exactly 1 file, the cursor rotates (a→b→c) and wraps to a on the fourth"
    - given: "a tampered file sampled in a round"
      when: "the round completes"
      then: "mismatched increments by 1 and the path is reported once (repeat sampling does not re-warn)"
    - given: "a manifest entry whose file was deleted before its turn"
      when: "the round samples it"
      then: "missing increments by 1"
    - given: "no manifest or an unusable manifest"
      when: "run_spot_round() is called"
      then: "it is a no-op and the report stays zeroed"
    - given: "GET /version after rounds"
      when: "integrity is read"
      then: "it gains runtime { rounds, checked, mismatched, missing, last_run_age_secs } and existing fields are unchanged"
    - given: "no round has run yet"
      when: "GET /version is read"
      then: "runtime.last_run_age_secs is omitted"
    - given: "rounds have run"
      when: "GET /metrics is read"
      then: "agentj_web_spot_rounds_total and agentj_web_spot_checked_total reflect the report"
  regression:
    - "web::tests::version_reports_asset_integrity_summary"
    - "web::tests::serves_index_assets_and_spa_fallback_with_cache_headers"
    - "web::tests::version_endpoint_reports_server_and_dist_versions"
  test_code: |
    #[test]
    fn spot_round_rotates_samples_and_counts_mismatches_once() {
        let dist = temp_dist("spot-core"); // index.html + assets/app.js
        std::fs::write(dist.join("assets/b.js"), "b").unwrap();
        std::fs::write(dist.join("assets/c.js"), "c").unwrap();
        write_manifest(&dist, &[
            ("/assets/app.js", &sha384_b64(b"console.log(1)")),
            ("/assets/b.js", &sha384_b64(b"b")),
            ("/assets/c.js", &sha384_b64(b"c")),
        ]);
        let web = WebConfig::new(Some(dist.clone()), None)
            .with_spot_check(SpotCheckConfig { interval: Duration::from_secs(60), sample: 1 });
        assert_eq!(web.run_spot_round().checked, 1); // /assets/app.js
        std::fs::write(dist.join("assets/b.js"), "tampered").unwrap();
        let round = web.run_spot_round(); // /assets/b.js
        assert_eq!(round.mismatched, 1);
        assert_eq!(round.offenses, vec!["/assets/b.js".to_string()]);
        std::fs::remove_file(dist.join("assets/c.js")).unwrap();
        assert_eq!(web.run_spot_round().missing, 1); // /assets/c.js deleted
        assert_eq!(web.run_spot_round().checked, 1); // wraps to /assets/app.js
        let report = web.integrity().runtime.clone();
        assert_eq!(
            (report.rounds, report.checked, report.mismatched, report.missing),
            (4, 3, 1, 1)
        );
        assert!(report.last_run.is_some());
        let text = prometheus_text(&web);
        assert!(text.contains("agentj_web_spot_rounds_total 4"));
        assert!(text.contains("agentj_web_spot_checked_total 3"));
    }
    #[tokio::test]
    async fn version_omits_last_run_age_before_any_round() {
        let state = test_state(crate::web::WebConfig::new(None, None));
        let local: SocketAddr = "127.0.0.1:4444".parse().unwrap();
        let (_, _, body) = get(crate::web::build_router(state, local), "/version").await;
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert!(v["integrity"]["runtime"].get("last_run_age_secs").is_none());
    }
  impl_code: |
    // web.rs additions
    #[derive(Debug, Clone, Copy)]
    pub(crate) struct SpotCheckConfig { pub(crate) interval: Duration, pub(crate) sample: usize }
    impl Default for SpotCheckConfig { /* 60s, 8 */ }
    #[derive(Debug, Clone, Default)]
    pub(crate) struct SpotCheckReport {
        pub(crate) rounds: u64, pub(crate) checked: u64,
        pub(crate) mismatched: u64, pub(crate) missing: u64,
        pub(crate) last_run: Option<Instant>,
    }
    pub(crate) struct SpotRound { pub(crate) checked: u64, pub(crate) mismatched: u64,
                                  pub(crate) missing: u64, pub(crate) offenses: Vec<String> }
    // WebConfig gains: manifest: Option<AssetManifest>, spot: SpotCheckConfig,
    //   spot_report: RwLock<SpotCheckReport>, warned: Mutex<HashSet<String>>, spot_cursor: AtomicUsize.
    impl WebConfig {
        pub(crate) fn with_spot_check(self, spot: SpotCheckConfig) -> Self { /* test ctor */ }
        pub(crate) fn run_spot_round(&self) -> SpotRound {
            // read manifest (skip if None), sample `spot.sample` keys starting at the cursor
            // (sorted key order), verify via the existing single-file hash helper, update
            // report under RwLock, return offenses for the caller to warn (dedup via `warned`).
        }
        pub(crate) fn runtime_snapshot(&self) -> (SpotCheckReport, Option<u64>) {
            // clone report + last_run_age_secs (None when never run) for serialization.
        }
    }
    // version_handler: add `"runtime": { "rounds":…, "checked":…, "mismatched":…, "missing":…,
    //   "last_run_age_secs": Option<u64> }` inside the existing `integrity` object.
    // prometheus_text (T1) gains the spot counters from runtime_snapshot():
    //   agentj_web_spot_rounds_total / _checked_total / _mismatched_total / _missing_total.
    // Refactor verify_assets to expose a single-file check reused by run_spot_round.
  verify:
    - "cargo test -p agentj-ipc web"
    - "cargo clippy -p agentj-ipc --all-targets -- -D warnings"
```

### T4: Spawn the periodic spot-check task on server start [P0] [Sequential]

```yaml
- id: T4
  title: "Spawn the periodic spot-check task from TcpIpcServer::bind"
  files:
    - crates/agentj-ipc/src/web.rs
    - crates/agentj-ipc/src/tcp.rs
  depends_on: [T3]
  breaking: false
  ac:
    - given: "a server bound with a manifest and paused tokio time"
      when: "60s elapse"
      then: "at least one spot round ran (integrity().runtime.rounds >= 1)"
    - given: "a server bound without a manifest"
      when: "time advances"
      then: "no round runs and the runtime block stays zeroed"
    - given: "the same offending path sampled across two rounds"
      when: "warn output is captured"
      then: "exactly one WARN line names that path"
  regression:
    - "tcp::tests::web_config_exposes_the_dist_version"
    - "web::tests::spot_round_rotates_samples_and_counts_mismatches_once"
  test_code: |
    #[tokio::test(start_paused = true)]
    async fn spot_task_runs_rounds_on_the_interval() {
        let dist = temp_dist("spot-task");
        write_file(&dist, "assets/a.js", "a");
        write_matching_manifest(&dist, &["assets/a.js"]);
        let web = Arc::new(WebConfig::new(Some(dist), None));
        web.spawn_spot_check();
        tokio::time::advance(Duration::from_secs(60)).await;
        tokio::task::yield_now().await;
        assert!(web.integrity().runtime.rounds >= 1);
        web.stop_spot_check(); // abort handle for test hygiene
    }
  impl_code: |
    // web.rs: `pub(crate) fn spawn_spot_check(self: &Arc<Self>) -> Option<JoinHandle<()>>`
    //   - None when manifest is absent/unusable; else tokio::spawn(loop {
    //       tokio::time::interval(spot.interval).tick().await; let round = self.run_spot_round();
    //       for path in round.offenses { if warned.insert(path) { tracing::warn!(path, "web asset spot check failed") } }
    //     }) and keep the handle on WebConfig for stop/tests.
    // tcp.rs bind(): after building WebConfig, call `web.spawn_spot_check();`.
  verify:
    - "cargo test -p agentj-ipc web"
    - "cargo test -p agentj-ipc tcp"
    - "cargo clippy -p agentj-ipc --all-targets -- -D warnings"
```

### T5: Lock root-level asset verification in the HTTP e2e [P1] [Parallel]

```yaml
- id: T5
  title: "Cover a root-level public asset in the serve e2e fixture"
  files:
    - crates/agentj-cli/tests/e2e_http_static.rs
  depends_on: []
  breaking: false
  ac:
    - given: "a dist whose manifest covers index.html, assets/app.js and a root-level favicon.svg"
      when: "serve --web-dist starts"
      then: "stderr has no integrity warning and GET /version reports integrity.checked == 3"
    - given: "the favicon bytes are tampered before start"
      when: "serve starts"
      then: "stderr warns and GET /version reports mismatched == 1"
  regression:
    - "e2e_http_static::silent_when_manifest_matches"
    - "e2e_http_static::warns_when_manifest_hash_does_not_match"
    - "e2e_http_static::serve_serves_web_dist_with_spa_fallback_and_cache_headers"
  test_code: |
    #[test]
    fn manifest_with_root_level_public_asset_verifies_silently() {
        let dist = temp_dist("public-root"); // index.html + assets/app.js
        std::fs::write(dist.join("favicon.svg"), "<svg/>").unwrap();
        write_asset_manifest(&dist, &[
            ("/index.html", &sha384_b64(b"<html>agentj-e2e-index</html>")),
            ("/assets/app.js", &sha384_b64(b"console.log(1)")),
            ("/favicon.svg", &sha384_b64(b"<svg/>")),
        ]);
        let stderr = run_serve_and_capture(&dist, |addr| {
            let ver = http_get(addr, "/version");
            assert!(ver.contains("\"checked\":3"), "checked must cover root assets: {ver}");
        });
        assert!(!stderr.contains("integrity"), "unexpected warning: {stderr}");
    }
  impl_code: |
    // Test scaffolding only (no production change): temp_dist keeps index.html + assets/app.js;
    // the new test adds favicon.svg and writes a 3-entry manifest via write_asset_manifest.
    // verify_assets already walks every manifest entry, including root-level paths
    // (this task locks that contract with a root-level public asset).
  verify:
    - "cargo test -p agentj-cli --test e2e_http_static"
```

### T6: Add the optional query param to FileListParams [P0] [Parallel]

```yaml
- id: T6
  title: "Add optional query to FileListParams with additive serde semantics"
  files:
    - crates/agentj-protocol/src/file.rs
    - crates/agentj-protocol/tests/contract.rs
    - schemas/FileListParams.json
  depends_on: []
  breaking: true
  ac:
    - given: "legacy params JSON {session_id} (no query)"
      when: "deserialized"
      then: "query is None"
    - given: "FileListParams {query: None}"
      when: "serialized"
      then: "the JSON has no query key"
    - given: "query Some(\"src\")"
      when: "round-tripped"
      then: "the value is preserved"
    - given: "the committed schema"
      when: "the drift gate runs"
      then: "FileListParams.json contains the optional query property"
  regression:
    - "contract::file_list_request_wire_shape"
    - "file::tests::file_list_params_roundtrip_omits_none"
    - "file::tests::file_types_generate_json_schema"
  test_code: |
    #[test]
    fn file_list_params_query_is_additive() {
        let legacy: FileListParams =
            serde_json::from_value(serde_json::json!({"session_id": "s1"})).unwrap();
        assert!(legacy.query.is_none());
        let with_query = FileListParams {
            session_id: "s1".into(), path: None, depth: None, query: Some("src".into()),
        };
        let json = serde_json::to_value(&with_query).unwrap();
        assert_eq!(json["query"], "src");
        let none = FileListParams { session_id: "s1".into(), path: None, depth: None, query: None };
        assert!(serde_json::to_value(none).unwrap().get("query").is_none());
    }
  impl_code: |
    // file.rs: add the field with the additive pattern
    /// Case-insensitive substring filter; empty/whitespace is treated as absent. @since 0.14.0
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    // Fix literals: file.rs in-file test + contract.rs `file_list_request_wire_shape` (query: None).
    // Regenerate: cargo run -p agentj-protocol --bin gen_schemas (only FileListParams.json changes
    // at this point — session structs are untouched until T9).
  verify:
    - "cargo test -p agentj-protocol"
    - "cargo test -p agentj-protocol -- drift"
```

### T7: Add optional total to FileListResult and fix all constructors [P0] [Sequential]

```yaml
- id: T7
  title: "Add optional total to FileListResult and update the three constructor sites"
  files:
    - crates/agentj-protocol/src/file.rs
    - crates/agentj-ipc/src/file_list.rs
    - crates/agentj-ipc/src/lib.rs
  depends_on: [T6]
  breaking: true
  ac:
    - given: "FileListResult {total: None}"
      when: "serialized"
      then: "the JSON has no total key"
    - given: "total Some(7)"
      when: "round-tripped"
      then: "the value is preserved"
    - given: "the workspace"
      when: "cargo check --workspace runs"
      then: "every FileListResult literal (file.rs test, file_list::finish, attach_git_statuses) compiles with total: None"
  regression:
    - "file_list::tests::depth_controls_recursion"
    - "file_list::tests::depth_and_truncation_respected"
    - "ipc::tests::dispatch_file_list_lists_root_sorted_excluding_git"
  test_code: |
    #[test]
    fn file_list_result_total_is_omitted_when_none() {
        let none = FileListResult { entries: vec![], truncated: false, total: None };
        assert!(serde_json::to_value(&none).unwrap().get("total").is_none());
        let some = FileListResult { entries: vec![], truncated: false, total: Some(7) };
        assert_eq!(serde_json::to_value(some).unwrap()["total"], 7);
    }
  impl_code: |
    // file.rs: /// Full match count discovered by a query scan (lower bound when truncated). @since 0.14.0
    // #[serde(default, skip_serializing_if = "Option::is_none")]
    // pub total: Option<u64>,
    // Fix constructors: file.rs test literal, file_list.rs `finish()` (total: None),
    // lib.rs `attach_git_statuses` rebuild (total: result.total).
    // NOTE: the drift gate stays RED until T8 regenerates schemas/FileListResult.json — so this
    // task filters protocol tests to `file_list` (the unfiltered drift gate returns in T8).
  verify:
    - "cargo test -p agentj-protocol --lib file_list"
    - "cargo test -p agentj-ipc file_list"
    - "cargo check --workspace"
```

### T8: Regenerate the FileListResult schema [P1] [Sequential]

```yaml
- id: T8
  title: "Regenerate schemas/FileListResult.json and restore the drift gate"
  files:
    - schemas/FileListResult.json
  depends_on: [T7]
  breaking: false
  ac:
    - given: "the committed schema is stale (no total property)"
      when: "gen_schemas runs"
      then: "FileListResult.json gains the optional total property and the drift gate passes"
  regression:
    - "schema::tests::drift_committed_schemas_match_generated"
  test_code: |
    grep -q '"total"' schemas/FileListResult.json || echo "expected stale schema before regen"
  impl_code: |
    cargo run -p agentj-protocol --bin gen_schemas
  verify:
    - "cargo test -p agentj-protocol -- drift"
```

### T9: Add optional project_root to SessionNewResult [P0] [Sequential]

```yaml
- id: T9
  title: "Add optional project_root to SessionNewResult"
  files:
    - crates/agentj-protocol/src/session.rs
    - crates/agentj-protocol/tests/contract.rs
    - schemas/SessionNewResult.json
  depends_on: [T8]
  breaking: true
  ac:
    - given: "SessionNewResult {project_root: None}"
      when: "serialized"
      then: "no project_root key is emitted"
    - given: "project_root Some(\"/tmp/proj\")"
      when: "round-tripped"
      then: "the value is preserved"
    - given: "the drift gate"
      when: "it runs"
      then: "SessionNewResult.json documents the optional field"
  regression:
    - "contract::session_new_response_wire_shape"
    - "session::tests::session_new_result_roundtrip"
  test_code: |
    #[test]
    fn session_new_result_project_root_is_additive() {
        let none = SessionNewResult { session_id: "s1".into(), project_root: None };
        assert!(serde_json::to_value(&none).unwrap().get("project_root").is_none());
        let some = SessionNewResult { session_id: "s1".into(), project_root: Some("/tmp/proj".into()) };
        assert_eq!(serde_json::to_value(some).unwrap()["project_root"], "/tmp/proj");
    }
  impl_code: |
    // session.rs: /// Effective canonicalized session root (cwd fallback). @since 0.14.0
    // #[serde(default, skip_serializing_if = "Option::is_none")]
    // pub project_root: Option<String>,
    // Fix literals: session.rs in-file test + contract.rs SessionNewResult case (project_root: None).
    // gen_schemas changes only SessionNewResult.json (FileListResult.json was fixed in T8,
    // SessionStatusResult is untouched until T10).
  verify:
    - "cargo test -p agentj-protocol"
    - "cargo test -p agentj-protocol -- drift"
```

### T10: Add optional project_root to SessionStatusResult [P0] [Sequential]

```yaml
- id: T10
  title: "Add optional project_root to SessionStatusResult and fix its literals"
  files:
    - crates/agentj-protocol/src/session.rs
    - crates/agentj-ipc/src/lib.rs
    - schemas/SessionStatusResult.json
  depends_on: [T9, T7]
  breaking: true
  ac:
    - given: "SessionStatusResult {project_root: None}"
      when: "serialized"
      then: "no project_root key is emitted"
    - given: "the dispatch literal in lib.rs"
      when: "cargo check runs"
      then: "it compiles with project_root: None (T14 wires the real value)"
    - given: "the drift gate"
      when: "it runs"
      then: "SessionStatusResult.json documents the optional field"
  regression:
    - "ipc::tests::session_status_returns_store_status"
    - "ipc::tests::session_status_unknown_session_returns_32802"
  test_code: |
    #[test]
    fn session_status_result_project_root_is_additive() {
        let none = SessionStatusResult { status: SessionStatus::Active, project_root: None };
        assert!(serde_json::to_value(&none).unwrap().get("project_root").is_none());
    }
  impl_code: |
    // session.rs: same additive field pattern as T9.
    // Fix literals: session.rs in-file test + lib.rs `dispatch_session_status` (project_root: None).
    // gen_schemas changes only SessionStatusResult.json (all earlier schemas are current).
  verify:
    - "cargo test -p agentj-protocol"
    - "cargo test -p agentj-ipc session"
    - "cargo test -p agentj-protocol -- drift"
```

### T11: Regenerate protocol TS types [P1] [Sequential]

```yaml
- id: T11
  title: "Regenerate agentj-ui/packages/protocol/types.d.ts from the updated schemas"
  files:
    - agentj-ui/packages/protocol/types.d.ts
  depends_on: [T10]
  breaking: false
  ac:
    - given: "the updated schemas (query/total/project_root)"
      when: "the generate script runs"
      then: "types.d.ts contains query, total and project_root and tsc --noEmit passes"
  regression:
    - "npm --prefix agentj-ui run build --workspace=@agentj/protocol"
  test_code: |
    grep -q 'query' agentj-ui/packages/protocol/types.d.ts || echo "expected stale types before regen"
  impl_code: |
    npm --prefix agentj-ui run generate --workspace=@agentj/protocol
  verify:
    - "npm --prefix agentj-ui run build --workspace=@agentj/protocol"
```

### T12: Implement query-aware search traversal in file_list [P0] [Sequential]

```yaml
- id: T12
  title: "Implement file_list::search with rank mirroring the TUI and a visit budget"
  files:
    - crates/agentj-ipc/src/file_list.rs
  depends_on: [T7]
  breaking: false
  ac:
    - given: "src/main.rs (name prefix), src/domain/domain_main.rs (name substring), docs/main-guide/readme.md (path substring only)"
      when: "search(root, None, \"main\") runs"
      then: "the order is rank 0 → rank 1 → rank 2 and total is Some(3)"
    - given: "a query matching more than SEARCH_RESULT_LIMIT entries"
      when: "search runs"
      then: "entries.len() == 100 and total is larger"
    - given: "a tree larger than SEARCH_VISIT_LIMIT"
      when: "search runs"
      then: "truncated is true and total is a lower bound"
    - given: ".git directories and escaping symlinks"
      when: "search runs"
      then: "they are excluded/skipped exactly like the listing walk"
  regression:
    - "file_list::tests::list_root_is_sorted_and_excludes_git"
    - "file_list::tests::symlink_escape_entry_is_skipped"
    - "file_list::tests::escape_via_dotdot_is_rejected"
  test_code: |
    fn search_tree(tag: &str, files: &[&str]) -> PathBuf {
        let root = temp_dir(tag);
        for rel in files {
            let path = root.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "x").unwrap();
        }
        root
    }
    #[test]
    fn search_ranks_prefix_over_substring_over_path() {
        let root = search_tree("search-rank", &[
            "src/main.rs", "src/domain/domain_main.rs", "docs/main-guide/readme.md",
        ]);
        let result = file_list::search(&root, None, "main").unwrap();
        let paths: Vec<_> = result.entries.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(paths, vec![
            "src/main.rs", "src/domain/domain_main.rs", "docs/main-guide/readme.md",
        ]);
        assert_eq!(result.total, Some(3));
        assert!(!result.truncated);
    }
    #[test]
    fn search_is_case_insensitive_and_path_scoped() {
        let root = search_tree("search-scope", &["src/MAIN.rs", "other/main.rs"]);
        let scoped = file_list::search(&root, Some("src"), "main").unwrap();
        assert_eq!(scoped.total, Some(1));
        assert_eq!(scoped.entries[0].path, "src/MAIN.rs");
    }
    #[test]
    fn search_reports_truncation_when_the_visit_budget_runs_out() {
        let root = search_tree("search-budget", &["a/x.rs", "b/y.rs", "c/z.rs"]);
        let result = file_list::search_with_budget(&root, None, "rs", 2).unwrap();
        assert!(result.truncated);
        assert!(result.total.is_some());
    }
  impl_code: |
    // file_list.rs additions
    pub const SEARCH_VISIT_LIMIT: usize = 20_000;
    pub const SEARCH_RESULT_LIMIT: usize = 100;
    pub fn search(root: &Path, path: Option<&str>, query: &str) -> Result<FileListResult, FileListError> {
        search_with_budget(root, path, query, SEARCH_VISIT_LIMIT)
    }
    pub(crate) fn search_with_budget(root: &Path, path: Option<&str>, query: &str, visit_limit: usize)
        -> Result<FileListResult, FileListError> {
        // resolve_target + Walk with the visit budget (reuse .git exclusion / symlink guard),
        // collect (rank, entry) for case-insensitive substring matches
        // (name-prefix 0 > name-substring 1 > path-substring 2), sort rank→path, cap at
        // SEARCH_RESULT_LIMIT, set total = Some(matches_found) and truncated = budget_hit.
    }
    // rank helper mirrors agentj-ui/apps/tui/src/files.ts::searchRank.
  verify:
    - "cargo test -p agentj-ipc file_list"
    - "cargo clippy -p agentj-ipc --all-targets -- -D warnings"
```

### T13: Wire query mode through file/list dispatch [P0] [Sequential]

```yaml
- id: T13
  title: "Dispatch file/list to search() when a trimmed query is present"
  files:
    - crates/agentj-ipc/src/lib.rs
  depends_on: [T12, T6, T10]
  breaking: false
  ac:
    - given: "params {session_id, query: \"main\"}"
      when: "file/list is dispatched"
      then: "the result carries ranked entries and total; the request does not fail on an old-style shape"
    - given: "params {query: \"   \"}"
      when: "dispatched"
      then: "the legacy listing path runs (no total)"
    - given: "a normal listing"
      when: "dispatched"
      then: "behavior and payload are byte-identical to v0.21 (regression locked)"
  regression:
    - "ipc::tests::dispatch_file_list_lists_root_sorted_excluding_git"
    - "ipc::tests::dispatch_file_list_maps_errors_to_codes_and_data"
    - "ipc::tests::dispatch_file_list_reports_git_statuses"
  test_code: |
    /// Query variant of `file_list_call` (the shared helper has no query param).
    async fn file_list_search_call(
        server: &IpcServer,
        sid: &str,
        path: Option<&str>,
        query: &str,
    ) -> serde_json::Value {
        let raw = serde_json::json!({
            "jsonrpc": "2.0", "id": 22, "method": "file/list",
            "params": { "session_id": sid, "path": path, "query": query },
        })
        .to_string();
        serde_json::from_str(&server.handle_request(&raw).await.unwrap()).unwrap()
    }
    #[tokio::test]
    async fn dispatch_file_list_query_returns_ranked_matches_with_total() {
        let root = temp_dir_filelist("query");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/main.rs"), "x").unwrap();
        std::fs::write(root.join("README.md"), "x").unwrap();
        let (server, sid) = file_list_server(&root);
        let v = file_list_search_call(&server, &sid, None, "main").await;
        assert!(v["error"].is_null(), "query call must succeed: {v}");
        assert_eq!(v["result"]["total"], 1);
        assert_eq!(v["result"]["entries"][0]["path"], "src/main.rs");
        let blank = file_list_search_call(&server, &sid, None, "   ").await;
        assert!(blank["result"].get("total").is_none(), "blank query = legacy listing");
        std::fs::remove_dir_all(&root).ok();
    }
  impl_code: |
    // dispatch_file_list: after resolving root,
    //   let query = params.query.as_deref().map(str::trim).filter(|q| !q.is_empty());
    //   let result = match query {
    //       Some(q) => file_list::search(&root, params.path.as_deref(), q)?,
    //       None => file_list::list(&root, params.path.as_deref(), params.depth)?,
    //   };
    //   attach_git_statuses must carry `total` through unchanged (T7 already does).
  verify:
    - "cargo test -p agentj-ipc file_list"
    - "cargo clippy -p agentj-ipc --all-targets -- -D warnings"
```

### T14: Return the effective project_root from session dispatch [P0] [Sequential]

```yaml
- id: T14
  title: "Return the effective project_root from session/new and session/status"
  files:
    - crates/agentj-ipc/src/lib.rs
  depends_on: [T10, T13]
  breaking: false
  ac:
    - given: "session/new without project_root"
      when: "dispatched"
      then: "the result project_root is the canonicalized process cwd"
    - given: "session/new with project_root"
      when: "dispatched"
      then: "the result project_root equals the canonicalized request value"
    - given: "session/status for a session with a stored root (or none)"
      when: "dispatched"
      then: "project_root is the stored root, else the canonicalized cwd fallback"
  regression:
    - "ipc::tests::session_new_with_project_root_persists_it"
    - "ipc::tests::session_status_returns_store_status"
  test_code: |
    #[tokio::test]
    async fn dispatch_session_status_reports_effective_project_root() {
        let server = /* session server with a temp dir as cwd/root */;
        let created = call(&server, "session/new", serde_json::json!({"project_root": dir_str})).await;
        let sid = created["result"]["session_id"].as_str().unwrap();
        let status = call(&server, "session/status", serde_json::json!({"session_id": sid})).await;
        assert_eq!(status["result"]["project_root"], canonical_dir_str);
    }
  impl_code: |
    // dispatch_session_new: effective = match resolve_project_root(params)? {
    //     Some(root) => root, None => std::env::current_dir()?.canonicalize()? };
    //   return json!({ "session_id": sid, "project_root": effective.to_string_lossy() });
    // dispatch_session_status: project_root = session_store.project_root(sid)
    //   .or_else(|| std::env::current_dir().ok().and_then(|d| d.canonicalize().ok()))
    //   .map(|p| p.to_string_lossy().into_owned());
    //   SessionStatusResult { status, project_root }
  verify:
    - "cargo test -p agentj-ipc session"
    - "cargo clippy -p agentj-ipc --all-targets -- -D warnings"
```

### T15: TUI server-side search with debounce and legacy fallback [P0] [Sequential]

```yaml
- id: T15
  title: "Search via debounced file/list queries with automatic legacy fallback"
  files:
    - agentj-ui/apps/tui/src/app.tsx
    - agentj-ui/apps/tui/src/app.test.tsx
  depends_on: [T6, T7]
  breaking: false
  ac:
    - given: "search mode and a query typed character by character"
      when: "150ms pass without further input"
      then: "exactly one request {session_id, path: null, depth: 8, query} is sent and server-ranked rows + total render"
    - given: "a reply without total (old kernel)"
      when: "it arrives"
      then: "the reply entries become the local index and subsequent keystrokes filter locally without new requests"
    - given: "an in-flight request and continued typing"
      when: "the stale reply arrives"
      then: "the typed query is preserved and stale rows do not overwrite current state"
    - given: "Esc / r / Enter"
      when: "pressed in search mode"
      then: "clear-or-exit / rebuild (re-query or re-index) / jump behave exactly as v0.21"
  regression:
    - "app.test.tsx: s enters search, printables filter, r rebuilds, enter jumps"
    - "app.test.tsx: keeps the query typed while the index request is in flight"
    - "app.test.tsx: jump loads ancestors depth-first and selects the nested target"
    - "app.test.tsx: esc clears a non-empty query before it exits search"
  test_code: |
    // fakeKernel gains an option { search?: "server" | "legacy" }:
    //   server → file/list with a string query returns ranked matches + total;
    //   legacy → the query is ignored and the depth-8 listing (no total) is returned.
    test("server search debounces, renders ranked rows and shows total", async () => {
      const kernel = fakeKernel({ search: "server" });
      const { setup, input } = await renderApp(kernel);
      await openFilesSidebar(setup, input);
      await act(async () => { input.press("s"); await setup.flush(); });
      await act(async () => { await input.type("mai"); await setup.flush(); });
      await act(async () => { await new Promise((r) => setTimeout(r, 200)); await setup.flush(); });
      const queryCalls = kernel.calls.filter(
        (c) => c.method === METHODS.fileList && c.query === "mai",
      );
      expect(queryCalls).toHaveLength(1); // debounced to a single request
      await act(async () => {
        await setup.waitFor(() => setup.captureCharFrame().includes("main.rs"));
      });
      act(() => setup.renderer.destroy());
    });
    test("falls back to the local index when the kernel omits total", async () => {
      const kernel = fakeKernel({ search: "legacy" });
      const { setup, input } = await renderApp(kernel);
      await openFilesSidebar(setup, input);
      await act(async () => { input.press("s"); await setup.flush(); });
      await act(async () => { await input.type("main"); await setup.flush(); });
      await act(async () => { await new Promise((r) => setTimeout(r, 200)); await setup.flush(); });
      const withQuery = (): number =>
        kernel.calls.filter((c) => c.method === METHODS.fileList && typeof c.query === "string").length;
      expect(withQuery()).toBe(1); // first reply (no total) becomes the local index
      await act(async () => { await input.type("."); await setup.flush(); });
      await act(async () => { await new Promise((r) => setTimeout(r, 200)); await setup.flush(); });
      expect(withQuery()).toBe(1); // local filtering issues no further requests
      act(() => setup.renderer.destroy());
    });
  impl_code: |
    // app.tsx: SearchState gains `mode: "unknown" | "server" | "local"` + `timerRef`.
    // app.test.tsx: extend fakeKernel with the { search: "server" | "legacy" } option.
    //   - startSearch: reset state (no upfront index fetch).
    //   - query mutations (append/backspace/clear): schedule 150ms debounce → runServerSearch(query).
    //   - runServerSearch: request {path: null, depth: SEARCH_INDEX_DEPTH, query}; on reply:
    //       typeof reply.total === "number"
    //         ? server mode: rows = buildSearchIndex(entries), total = reply.total, truncated = reply.truncated
    //         : local mode: index = buildSearchIndex(entries.slice(0, MAX_SEARCH_INDEX)),
    //           rows/total = filterEntries(index, query, SEARCH_RESULT_LIMIT)
    //   - stale guard: ignore replies whose query !== current state.query (keep typed input, C1 pattern).
    //   - rebuild (r): server mode → re-run current query; local mode → re-send depth-8 (no query).
    //   - empty query: rows cleared, no request; Esc tiers unchanged.
  verify:
    - "bun --cwd agentj-ui/apps/tui test src/app.test.tsx"
    - "bun --cwd agentj-ui/apps/tui test src/files.test.ts"
```

### T16: Key TUI filetree prefs by the session project_root [P0] [Sequential]

```yaml
- id: T16
  title: "Capture the session project_root and use it as the filetree prefs key"
  files:
    - agentj-ui/apps/tui/src/session.tsx
    - agentj-ui/apps/tui/src/app.tsx
    - agentj-ui/apps/tui/src/app.test.tsx
  depends_on: [T10, T15]
  breaking: false
  ac:
    - given: "session/new returns {session_id, project_root: \"/tmp/proj\"}"
      when: "the tree is toggled open"
      then: "filetree.json stores the expansion under the \"/tmp/proj\" key"
    - given: "session/status returns a project_root"
      when: "a session is continued"
      then: "the same root key is used"
    - given: "a kernel reply without project_root"
      when: "prefs persist"
      then: "the __default__ fallback key is used and old project-id keys are pruned"
  regression:
    - "session.test.tsx: createNew calls session/new via METHODS"
    - "session.test.tsx: continueSession calls status and subscribe"
    - "filetreePrefs.test.ts: caps entries per key and prunes stale project keys"
    - "filesView.test.tsx: restores persisted expanded paths depth-ascending after the root load"
  test_code: |
    // fakeKernel gains an option { sessionRoot?: string } → session/new (+ status) replies
    // carry project_root when set.
    test("keys filetree prefs by the session project_root", async () => {
      const kernel = fakeKernel({ sessionRoot: "/tmp/proj" });
      const { setup, input } = await renderApp(kernel);
      await openFilesSidebar(setup, input);                                  // creates the session
      await act(async () => { input.press("enter"); await setup.flush(); }); // expand src
      expect(loadFileTreePrefs().expanded["/tmp/proj"]).toEqual(["src"]);
      act(() => setup.renderer.destroy());
    });
    test("falls back to __default__ when the kernel omits project_root", async () => {
      const kernel = fakeKernel(); // no project_root in session/new
      const { setup, input } = await renderApp(kernel);
      await openFilesSidebar(setup, input);
      await act(async () => { input.press("enter"); await setup.flush(); });
      expect(loadFileTreePrefs().expanded["__default__"]).toEqual(["src"]);
      act(() => setup.renderer.destroy());
    });
  impl_code: |
    // session.tsx: `useSession` gains `projectRoot` state; createNew captures res.project_root,
    //   continueSession captures (await client.request(METHODS.sessionStatus, …)).project_root;
    //   return { createNew, continueSession, archiveSession, projectRoot }.
    // app.tsx: const activeProjectKey = session.projectRoot ?? DEFAULT_PROJECT_KEY;
    //   pass it to useFileTree (replaces the session.projectId ?? projects[0].id derivation).
    // Also update the existing app.test.tsx case "persists expansion under the active session's
    // project group" to the new key semantics (root key, or __default__ when the kernel omits it).
  verify:
    - "bun --cwd agentj-ui/apps/tui test src/app.test.tsx"
    - "bun --cwd agentj-ui/apps/tui test src/session.test.tsx"
    - "bun --cwd agentj-ui/apps/tui test src/filetreePrefs.test.ts"
```

### T17: Teach the mock kernel query search and legacy mode [P1] [Sequential]

```yaml
- id: T17
  title: "Answer file/list query searches (and a legacy no-total mode) in the mock kernel"
  files:
    - agentj-ui/apps/tui/src/e2e/mock-kernel.ts
    - agentj-ui/apps/tui/src/e2e/mock-files.test.ts
  depends_on: [T15, T16]
  breaking: false
  ac:
    - given: "file/list with query and legacy mode off"
      when: "the mock answers"
      then: "ranked filtered entries plus total are returned"
    - given: "legacy mode on"
      when: "a query arrives"
      then: "query is ignored and the depth-8 listing is returned without total"
  regression:
    - "e2e/mock-files.test.ts: answers root and per-directory listings with git statuses"
    - "e2e/mock-files.test.ts: depth 8 returns the flat search fixture"
  test_code: |
    // T17 also extracts the ws-open dance (currently duplicated across the sibling tests) into a
    // local `openWs(url)` helper; the sketches below use it.
    test("answers query searches with ranked matches and total", async () => {
      const k = await startMockKernel();
      const ws = await openWs(k.url);
      await rpc(ws, 1, "initialize", { protocol_version: "0.14.0" });
      await rpc(ws, 2, "session/new", {});
      const reply = (await rpc(ws, 3, "file/list", {
        session_id: "sess-1", path: null, depth: 8, query: "main",
      })) as { entries: Array<{ path: string }>; total?: number };
      expect(reply.total).toBeGreaterThan(0);
      expect(reply.entries[0].path).toContain("main");
      ws.close();
      k.close();
    });
    test("legacy mode ignores query and omits total", async () => {
      const k = await startMockKernel({ legacySearch: true });
      const ws = await openWs(k.url);
      await rpc(ws, 1, "initialize", { protocol_version: "0.14.0" });
      await rpc(ws, 2, "session/new", {});
      const reply = (await rpc(ws, 3, "file/list", {
        session_id: "sess-1", path: null, depth: 8, query: "main",
      })) as { total?: number };
      expect(reply.total).toBeUndefined();
      ws.close();
      k.close();
    });
  impl_code: |
    // mock-kernel.ts: file/list handler branches on `typeof params.query === "string" && !legacySearch`
    //   → filter the depth-8 fixture with the same rank rule, return { entries, total, truncated:false }.
    //   `legacySearch` option (default false) forces the v0.21 reply shape (no total).
  verify:
    - "bun --cwd agentj-ui/apps/tui test src/e2e/mock-files.test.ts"
```

### T18: Extend useRovingFocus with wrap and Home/End helpers [P0] [Parallel]

```yaml
- id: T18
  title: "Add wrap option and firstIndex/lastIndex to the shared roving helpers"
  files:
    - agentj-ui/apps/web/src/hooks/useRovingFocus.ts
    - agentj-ui/apps/web/src/hooks/useRovingFocus.test.tsx
  depends_on: []
  breaking: false
  ac:
    - given: "moveSelection(2, +1, 3, { wrap: true })"
      when: "called"
      then: "0 (wraps); without options the same call returns 2 (clamp)"
    - given: "firstIndex(0) / lastIndex(0)"
      when: "called"
      then: "both return -1"
    - given: "useRovingFocus({ count: 3, wrap: true })"
      when: "moveBy(+1) from the last index"
      then: "selected wraps to 0"
  regression:
    - "hooks/useRovingFocus.test.tsx: moves and clamps a selection index"
    - "hooks/useRovingFocus.test.tsx: tracks the selection and clamps moveBy to the list bounds"
    - "components/fileTree.test.tsx (clamp semantics unchanged)"
  test_code: |
    it("wraps when asked and clamps by default", () => {
      expect(moveSelection(2, 1, 3, { wrap: true })).toBe(0);
      expect(moveSelection(2, 1, 3)).toBe(2);
      expect(moveSelection(-1, -1, 3, { wrap: true })).toBe(2);
    });
    it("returns -1 for empty lists", () => {
      expect(firstIndex(0)).toBe(-1);
      expect(lastIndex(0)).toBe(-1);
      expect(lastIndex(3)).toBe(2);
    });
  impl_code: |
    // useRovingFocus.ts
    export function moveSelection(current, delta, count, options?: { wrap?: boolean }): number {
      if (count <= 0) return -1;
      if (options?.wrap) { const base = current < 0 ? 0 : current; return (base + delta + count) % count; }
      return Math.min(count - 1, Math.max(0, (current < 0 ? 0 : current) + delta));
    }
    export function firstIndex(count: number): number { return count > 0 ? 0 : -1; }
    export function lastIndex(count: number): number { return count > 0 ? count - 1 : -1; }
    // useRovingFocus({ count, wrap = false }) — moveBy/setSelected route through moveSelection.
  verify:
    - "npm --prefix agentj-ui test --workspace=@agentj/web -- hooks/useRovingFocus"
    - "npm --prefix agentj-ui test --workspace=@agentj/web -- components/fileTree"
```

### T19: Migrate TopbarMenu roving to the shared hook [P0] [Parallel]

```yaml
- id: T19
  title: "Migrate TopbarMenu flyout roving to useRovingFocus (wrap + Home/End + tabindex)"
  files:
    - agentj-ui/apps/web/src/components/TopbarMenu.tsx
    - agentj-ui/apps/web/src/components/topbarMenu.test.tsx
  depends_on: [T18]
  breaking: false
  ac:
    - given: "an open project flyout"
      when: "ArrowDown/ArrowUp are pressed"
      then: "selection wraps and DOM focus follows; Home/End jump to the first/last item"
    - given: "rendered flyout items"
      when: "tabindex is inspected"
      then: "only the selected item has tabindex 0 (roving); Tab leaves the menu"
    - given: "the existing interactions"
      when: "hover open/close, 120ms leave delay, Esc layering and focus restore run"
      then: "behavior and aria contracts (aria-haspopup/expanded/controls, menuitemradio/checked) are unchanged"
  regression:
    - "topbarMenu.test.tsx: first-level menu has exactly five items in order and no inline lists"
    - "topbarMenu.test.tsx: Escape closes the flyout first, then the menu and restores focus"
    - "a11y.test.tsx: keyboard a11y — project flyout trigger exposes aria-haspopup + aria-expanded and focuses its items"
  test_code: |
    it("roves with wrap, Home/End and a single tab stop", async () => {
      render(<TopbarMenu … />);
      await user.click(screen.getByRole("button", { name: /更多|⋯/ }));
      await user.hover(screen.getByRole("menuitem", { name: /项目/ }));
      const items = screen.getAllByRole("menuitemradio");
      await user.keyboard("{ArrowDown}");
      expect(document.activeElement).toBe(items[1]);
      await user.keyboard("{End}");
      expect(document.activeElement).toBe(items[items.length - 1]);
      await user.keyboard("{ArrowDown}"); // wraps
      expect(document.activeElement).toBe(items[0]);
      expect(items[0]).toHaveAttribute("tabindex", "0");
      expect(items[1]).toHaveAttribute("tabindex", "-1");
    });
  impl_code: |
    // Replace `onFlyoutKeyDown`'s querySelectorAll roving with: item refs array,
    // `useRovingFocus({ count, wrap: true })`, ArrowUp/Down via moveBy, Home/End via
    // firstIndex/lastIndex, `tabIndexFor(index, selected)` on items, and a focus effect that
    // focuses the selected ref after keyboard moves. Keep hover bridge/delay, Esc layering,
    // focus-without-opening suppression, portal-free anchoring and all aria attributes.
  verify:
    - "npm --prefix agentj-ui test --workspace=@agentj/web -- components/topbarMenu"
    - "npm --prefix agentj-ui test --workspace=@agentj/web -- components/a11y"
```

### T20: Migrate SessionListMenu roving to the shared hook [P0] [Parallel]

```yaml
- id: T20
  title: "Migrate SessionListMenu flyout roving to useRovingFocus (wrap + Home/End + tabindex)"
  files:
    - agentj-ui/apps/web/src/components/SessionListMenu.tsx
    - agentj-ui/apps/web/src/components/sessionListMenu.test.tsx
  depends_on: [T18]
  breaking: false
  ac:
    - given: "an open 排序/过滤 flyout"
      when: "arrows / Home / End are pressed"
      then: "selection wraps, focus follows, first/last jump works, one tab stop (roving)"
    - given: "the menu opens"
      when: "it was triggered from focus"
      then: "focus still enters the first item; portal positioning, clampMenuLeft, scroll-close and aria-disabled cleanup item behave as before"
  regression:
    - "sessionListMenu.test.tsx: moves focus into the first menu item when the menu opens"
    - "sessionListMenu.test.tsx: ArrowDown roams the flyout items"
    - "sessionListMenu.test.tsx: clamps the right edge into the 390px viewport"
  test_code: |
    it("roves the flyout with wrap and Home/End", async () => {
      render(<SessionListMenu … />);
      await user.click(screen.getByRole("button", { name: /会话列表菜单|⋯/ }));
      await user.hover(screen.getByRole("menuitem", { name: /排序/ }));
      const radios = screen.getAllByRole("menuitemradio");
      await user.keyboard("{End}");
      expect(document.activeElement).toBe(radios[radios.length - 1]);
      await user.keyboard("{ArrowDown}"); // wraps
      expect(document.activeElement).toBe(radios[0]);
    });
  impl_code: |
    // Same migration pattern as T19 (shared hook + refs + tabindexFor + focus effect),
    // preserving the portal, viewport clamping, 120ms close delay, scroll/resize listeners
    // and the aria-disabled cleanup row.
  verify:
    - "npm --prefix agentj-ui test --workspace=@agentj/web -- components/sessionListMenu"
```

### T21: Migrate SidebarTabs to the shared roving helpers [P1] [Parallel]

```yaml
- id: T21
  title: "Migrate SidebarTabs arrow roving to moveSelection/tabIndexFor"
  files:
    - agentj-ui/apps/web/src/components/SidebarTabs.tsx
    - agentj-ui/apps/web/src/components/sidebarTabs.test.tsx
  depends_on: [T18]
  breaking: false
  ac:
    - given: "two tabs"
      when: "ArrowLeft/ArrowRight are pressed"
      then: "selection wraps (Sessions ⇄ Files) and focus follows; tabIndex stays 0/-1"
    - given: "the BUG-011 focus-park path"
      when: "a tab is activated"
      then: "data-tab anchors and focus parking still work"
  regression:
    - "sidebarTabs.test.tsx: switches tabs with arrow keys"
    - "sidebarTabs.test.tsx: uses roving tabIndex on the selected tab"
    - "UnifiedSidebar.test.tsx: keeps focus inside the sidebar after f switches to the files tab (BUG-011)"
  test_code: |
    it("wraps with shared helpers and keeps a single tab stop", async () => {
      render(<SidebarTabs tab="threads" onTab={() => {}} />);
      await user.keyboard("{ArrowLeft}"); // wraps to files
      expect(document.activeElement).toHaveAttribute("data-tab", "files");
      expect(screen.getByRole("tab", { name: /文件/ })).toHaveAttribute("tabindex", "0");
    });
  impl_code: |
    // Replace the local % TABS.length arithmetic with moveSelection(index, delta, count, { wrap: true })
    // and tabIndexFor; keep role=tablist/tab, aria-selected and data-tab untouched.
  verify:
    - "npm --prefix agentj-ui test --workspace=@agentj/web -- components/sidebarTabs"
```

### T22: Migrate SettingsDialog tablist to the shared helpers [P1] [Parallel]

```yaml
- id: T22
  title: "Migrate SettingsDialog tablist roving to moveSelection/tabIndexFor"
  files:
    - agentj-ui/apps/web/src/components/SettingsDialog.tsx
    - agentj-ui/apps/web/src/components/settingsDialog.test.tsx
  depends_on: [T18]
  breaking: false
  ac:
    - given: "the vertical tablist"
      when: "ArrowUp/Down, Home/End are pressed"
      then: "selection wraps and jumps to first/last exactly as before, now via shared helpers"
    - given: "a tab is activated"
      when: "the panel renders"
      then: "role=tablist/tab + aria-orientation and tabpanel switching are unchanged"
  regression:
    - "settingsDialog.test.tsx: ArrowDown moves tab selection, focus and the tabpanel"
    - "settingsDialog.test.tsx: keeps keyboard roaming across four tabs"
  test_code: |
    it("keeps wrap + Home/End via shared helpers", async () => {
      render(<SettingsDialog open … />);
      await user.keyboard("{End}");
      expect(document.activeElement).toHaveTextContent(/演示数据/);
      await user.keyboard("{ArrowDown}"); // wraps to the first tab
      expect(document.activeElement).toHaveTextContent(/连接/);
    });
  impl_code: |
    // Replace the local wrap/Home/End implementation with moveSelection(..., { wrap: true }) +
    // firstIndex/lastIndex + tabIndexFor; keep the tabRefs map and focus behaviour.
  verify:
    - "npm --prefix agentj-ui test --workspace=@agentj/web -- components/settingsDialog"
```

### T23: Add the sidebar hint source and component [P0] [Parallel]

```yaml
- id: T23
  title: "Add shortcutHints.ts and the SidebarHint component"
  files:
    - agentj-ui/apps/web/src/components/shortcutHints.ts
    - agentj-ui/apps/web/src/components/SidebarHint.tsx
    - agentj-ui/apps/web/src/components/sidebarHint.test.tsx
  depends_on: []
  breaking: false
  ac:
    - given: "view=threads"
      when: "SidebarHint renders"
      then: "it shows j/k 移动 · Enter 打开 · n 新建 · f 文件视图"
    - given: "view=files and searching"
      when: "it renders"
      then: "it shows 输入过滤 · Enter 跳转 · Esc 返回"
    - given: "any state"
      when: "the markup is inspected"
      then: "the root element carries aria-hidden=\"true\" and the text comes from the shared const map"
  regression:
    - "UnifiedSidebar.test.tsx: toggles the sidebar tab with f and persists it"
  test_code: |
    it("renders contextual hints per view and search state", () => {
      const { rerender } = render(<SidebarHint view="threads" searching={false} />);
      expect(screen.getByText(/j\/k 移动 · Enter 打开 · n 新建 · f 文件视图/)).toBeTruthy();
      rerender(<SidebarHint view="files" searching={true} />);
      expect(screen.getByText(/输入过滤 · Enter 跳转 · Esc 返回/)).toBeTruthy();
      expect(screen.getByTestId("sidebar-hint")).toHaveAttribute("aria-hidden", "true");
    });
  impl_code: |
    // shortcutHints.ts: export const SIDEBAR_HINTS = {
    //   threads: "j/k 移动 · Enter 打开 · n 新建 · f 文件视图",
    //   files: "j/k 移动 · Enter 展开 · s 搜索 · r 刷新",
    //   filesSearching: "输入过滤 · Enter 跳转 · Esc 返回",
    // } as const;
    // SidebarHint.tsx: props { view: "threads" | "files"; searching: boolean } → <div
    //   className="aj-sidebar__hint" data-testid="sidebar-hint" aria-hidden="true">…</div>
    //   (styling lands in T24's sidebar.css).
  verify:
    - "npm --prefix agentj-ui test --workspace=@agentj/web -- components/sidebarHint"
```

### T24: Render the hint line in UnifiedSidebar [P1] [Sequential]

```yaml
- id: T24
  title: "Render SidebarHint at the sidebar bottom for both views"
  files:
    - agentj-ui/apps/web/src/components/UnifiedSidebar.tsx
    - agentj-ui/apps/web/src/components/UnifiedSidebar.test.tsx
    - agentj-ui/apps/web/src/components/sidebar.css
  depends_on: [T23]
  breaking: false
  ac:
    - given: "the threads view"
      when: "the sidebar renders"
      then: "the threads hint is visible; switching to files (f) swaps the hint text"
    - given: "search mode active"
      when: "the sidebar renders"
      then: "the search hint is visible"
    - given: "a narrow viewport"
      when: "the hint renders"
      then: "it stays on one line with an ellipsis (no wrap, no overflow)"
    - given: "existing keyboard behavior"
      when: "the shortcut-scope tests run"
      then: "they stay green (hint is display-only)"
  regression:
    - "UnifiedSidebar.test.tsx: fires no sidebar shortcut while typing in the composer"
    - "UnifiedSidebar.test.tsx: ignores sidebar shortcuts when focus is outside the sidebar"
    - "UnifiedSidebar.test.tsx: keeps focus inside the sidebar after r refreshes the tree from a row (BUG-012)"
  test_code: |
    it("shows the contextual hint for the active view and search state", async () => {
      renderShortcutSidebar();
      expect(screen.getByTestId("sidebar-hint")).toHaveTextContent(/j\/k 移动/);
      await act(() => { /* press f */ });
      expect(screen.getByTestId("sidebar-hint")).toHaveTextContent(/s 搜索/);
    });
  impl_code: |
    // UnifiedSidebar.tsx: derive `searching = searchMode !== null` and the active view from the
    // existing tab state; render <SidebarHint view={…} searching={…} /> as the last child of the
    // sidebar root (below list/tree, above/independent of FileTree's stats footer).
    // sidebar.css: add `.aj-sidebar__hint` (muted token color, single line, white-space: nowrap,
    // overflow: hidden, text-overflow: ellipsis).
  verify:
    - "npm --prefix agentj-ui test --workspace=@agentj/web -- components/UnifiedSidebar"
```

### T25: Bump workspace version and update the version script [P0] [Parallel]

```yaml
- id: T25
  title: "Bump the Rust workspace to 0.22.0 and point check-version.sh at 0.22.0/0.14.0"
  files:
    - Cargo.toml
    - Cargo.lock
    - scripts/check-version.sh
  depends_on: []
  breaking: false
  ac:
    - given: "Cargo.toml version = 0.22.0"
      when: "cargo check --workspace --offline runs"
      then: "Cargo.lock records the workspace crates at 0.22.0"
    - given: "the script"
      when: "read"
      then: "it greps 0.22.0 for the workspace/sdk/desktop and 0.14.0 for PROTOCOL_VERSION"
  regression:
    - "cargo metadata --no-deps --format-version 1 (workspace parses)"
  test_code: |
    grep -q 'version = "0.22.0"' Cargo.toml || echo "expected stale version before bump"
  impl_code: |
    # Cargo.toml: version = "0.22.0"; Cargo.lock: refresh via
    #   cargo check --workspace --offline
    # scripts/check-version.sh: replace 0.21.1 → 0.22.0 (5 greps) and 0.13.0 → 0.14.0 (3 greps).
  verify:
    - "grep -q 'version = \"0.22.0\"' Cargo.toml"
    - "grep -q '0.14.0' scripts/check-version.sh"
    - "cargo check --workspace --offline"
```

### T26: Bump PROTOCOL_VERSION to 0.14.0 [P1] [Sequential]

```yaml
- id: T26
  title: "Bump PROTOCOL_VERSION to 0.14.0 across the Rust sources and registry test"
  files:
    - crates/agentj-protocol/src/lib.rs
    - protocol/methods.json
    - crates/agentj-protocol/tests/registry.rs
  depends_on: [T25]
  breaking: false
  ac:
    - given: "the constant, methods.json and the registry literal"
      when: "the registry test runs"
      then: "all three read 0.14.0 and the method count stays 21"
    - given: "the initialize handshake tests"
      when: "cargo test -p agentj-ipc runs"
      then: "they pass (they assert the constant, not a literal)"
  regression:
    - "registry::registry_lists_all_v01_methods"
    - "ipc::tests::initialize_reports_protocol_version_not_product_version"
  test_code: |
    grep -q 'PROTOCOL_VERSION: &str = "0.14.0"' crates/agentj-protocol/src/lib.rs || echo "expected stale constant before bump"
  impl_code: |
    # lib.rs: PROTOCOL_VERSION = "0.14.0"; methods.json: "protocol_version": "0.14.0";
    # registry.rs: assert_eq!(reg["protocol_version"], "0.14.0").
  verify:
    - "cargo test -p agentj-protocol"
    - "cargo test -p agentj-ipc initialize"
```

### T27: Bump the SDK VERSION and PROTOCOL_VERSION [P1] [Sequential]

```yaml
- id: T27
  title: "Bump sdk VERSION to 0.22.0 and PROTOCOL_VERSION to 0.14.0 with test literals"
  files:
    - agentj-ui/packages/sdk/src/config.ts
    - agentj-ui/packages/sdk/src/__tests__/config.test.ts
    - agentj-ui/packages/sdk/src/__tests__/init-default.test.ts
  depends_on: [T26]
  breaking: false
  ac:
    - given: "config.ts"
      when: "the sdk tests run"
      then: "VERSION is 0.22.0, PROTOCOL_VERSION is 0.14.0 and both literals in the tests match"
  regression:
    - "sdk config tests: methods cover the 21-method registry"
  test_code: |
    grep -q 'export const VERSION = "0.22.0"' agentj-ui/packages/sdk/src/config.ts || echo "expected stale VERSION before bump"
  impl_code: |
    // config.ts: VERSION = "0.22.0"; PROTOCOL_VERSION = "0.14.0".
    // config.test.ts: update the version test name + expectations.
    // init-default.test.ts: update the literal expectation to 0.14.0.
  verify:
    - "npm --prefix agentj-ui test --workspace=@agentj/sdk"
```

### T28: Bump the desktop manifests [P1] [Sequential]

```yaml
- id: T28
  title: "Bump desktop package.json, tauri.conf.json and the Tauri crate to 0.22.0"
  files:
    - agentj-ui/apps/desktop/package.json
    - agentj-ui/apps/desktop/src-tauri/tauri.conf.json
    - agentj-ui/apps/desktop/src-tauri/Cargo.toml
  depends_on: [T25]
  breaking: false
  ac:
    - given: "the three manifests"
      when: "check-desktop-ui.mjs runs"
      then: "desktop-ui-ok (versions aligned with the workspace)"
  regression:
    - "node agentj-ui/apps/desktop/check-desktop-ui.mjs"
  test_code: |
    grep -q '"version": "0.22.0"' agentj-ui/apps/desktop/package.json || echo "expected stale desktop version before bump"
  impl_code: |
    # package.json + tauri.conf.json: "version": "0.22.0"; src-tauri/Cargo.toml: version = "0.22.0".
  verify:
    - "node agentj-ui/apps/desktop/check-desktop-ui.mjs"
```

### T29: Refresh the desktop lockfile and assert the version gate [P2] [Sequential]

```yaml
- id: T29
  title: "Refresh src-tauri/Cargo.lock and pass check-version.sh end to end"
  files:
    - agentj-ui/apps/desktop/src-tauri/Cargo.lock
  depends_on: [T27, T28]
  breaking: false
  ac:
    - given: "all version sources bumped"
      when: "check-version.sh runs"
      then: "version-ok (0.22.0 workspace/sdk/desktop; 0.14.0 PROTOCOL_VERSION)"
    - given: "the lockfile"
      when: "grepped"
      then: "agentj-ui is recorded at 0.22.0"
  regression:
    - "./scripts/check-version.sh"
  test_code: |
    grep -A1 'name = "agentj-ui"' agentj-ui/apps/desktop/src-tauri/Cargo.lock | grep 0.22.0 || echo "expected stale lock before refresh"
  impl_code: |
    cargo check --manifest-path agentj-ui/apps/desktop/src-tauri/Cargo.toml --offline
  verify:
    - "grep -A1 'name = \"agentj-ui\"' agentj-ui/apps/desktop/src-tauri/Cargo.lock | grep 0.22.0"
    - "./scripts/check-version.sh"
```

### T30: Docs — protocol spec, README, AGENTS [P2] [Parallel]

```yaml
- id: T30
  title: "Document /metrics, file/list query, session project_root and the manifest/hint changes"
  files:
    - docs/specs/ipc-protocol.md
    - README.md
    - AGENTS.md
  depends_on: []
  breaking: false
  ac:
    - given: "the HTTP surface section"
      when: "read"
      then: "it documents GET /metrics (gating, Prometheus text, Top-50) and the integrity runtime block"
    - given: "the file/list section"
      when: "read"
      then: "it documents the optional query param, the visit budget and total"
    - given: "README/AGENTS"
      when: "read"
      then: "they mention the v0.22.0 items, PROTOCOL_VERSION 0.14.0 and the new commands"
  regression:
    - "./scripts/check-version.sh"
  test_code: |
    grep -q "GET /metrics" docs/specs/ipc-protocol.md || echo "expected missing before edit"
  impl_code: |
    # ipc-protocol.md: HTTP 面 + /metrics contract; file/list params/result additions;
    #   SessionNewResult/SessionStatusResult project_root; PROTOCOL_VERSION 0.14.0 notes.
    # README.md: curl /metrics example; manifest public coverage; spot-check; TUI server search.
    # AGENTS.md: v0.22.0 paragraph (four items, gate counts placeholder filled at T31).
  verify:
    - "grep -q 'GET /metrics' docs/specs/ipc-protocol.md && grep -q 'query' docs/specs/ipc-protocol.md"
```

### T31: Summary sync, design backfill and the full gate suite [P2] [Final]

```yaml
- id: T31
  title: "Update summary_and_next.md, backfill the design status and run every gate"
  files:
    - .agents/summary/summary_and_next.md
    - docs/superpowers/specs/2026-09-24-agentj-metrics-manifest-search-and-keys-design.md
    - AGENTS.md
  depends_on: [T0, T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12, T13, T14, T15, T16, T17, T18, T19, T20, T21, T22, T23, T24, T25, T26, T27, T28, T29, T30]
  breaking: false
  ac:
    - given: "all tasks landed"
      when: "the full gate suite runs"
      then: "cargo fmt/test/clippy/drift, sdk, web, tui, web build, check-version (0.22.0) and desktop-ui all pass"
    - given: "the gates passed"
      when: "summary_and_next.md is read"
      then: "it records the v0.22.0 progress, gate counts and next candidates (replace, do not append)"
    - given: "the gates passed"
      when: "the design doc is read"
      then: "实施状态 records v0.22.0, gate counts and the acceptance ownership note"
    - given: "the gates passed"
      when: "AGENTS.md is read"
      then: "the v0.22.0 paragraph carries the final gate counts (T30's placeholder is filled)"
    - given: "the automated gates passed"
      when: "the plan completes"
      then: "live acceptance (curl /metrics, Playwright, TUI) is explicitly deferred to jskill-test-acceptance"
  regression:
    - "all suites above"
  test_code: |
    cargo fmt --check
    cargo test --workspace
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test -p agentj-protocol -- drift
    npm --prefix agentj-ui test --workspace=@agentj/sdk
    npm --prefix agentj-ui test --workspace=@agentj/web
    bun --cwd agentj-ui/apps/tui test
    npm --prefix agentj-ui run build --workspace=@agentj/web
    ./scripts/check-version.sh
    node agentj-ui/apps/desktop/check-desktop-ui.mjs
  impl_code: |
    # Replace summary_and_next.md with the v0.22.0 feature record (progress, gate counts,
    # remaining issues, next candidates) — replace, do not append.
    # Fill the v0.22.0 gate counts in AGENTS.md (T30 left them as a placeholder).
    # Backfill 实施状态 in the design doc; note live acceptance is owned by the acceptance stage.
  verify:
    - "all commands above exit 0 with zero failures"
```

---

## Dependency Graph

```mermaid
graph TD
    T0[T0 limiter snapshot] --> T1[T1 /metrics]
    T1 --> T3[T3 spot-check core]
    T3 --> T4[T4 spot-check spawn]
    T2[T2 build dist enumeration]
    T5[T5 e2e public fixture]
    T6[T6 file.rs query + schema] --> T7[T7 file.rs total + literals]
    T6 --> T13[T13 file/list dispatch query]
    T7 --> T8[T8 FileListResult schema]
    T7 --> T10[T10 SessionStatusResult project_root]
    T7 --> T12[T12 file_list search()]
    T7 --> T15[T15 TUI server search]
    T8 --> T9[T9 SessionNewResult project_root]
    T9 --> T10
    T10 --> T11[T11 TS types regen]
    T10 --> T13[T13 file/list dispatch query]
    T10 --> T14[T14 session dispatch project_root]
    T10 --> T16[T16 TUI root key]
    T12 --> T13
    T13 --> T14
    T15 --> T16
    T15 --> T17[T17 mock kernel query]
    T16 --> T17
    T18[T18 roving helpers wrap/HomeEnd] --> T19[T19 TopbarMenu]
    T18 --> T20[T20 SessionListMenu]
    T18 --> T21[T21 SidebarTabs]
    T18 --> T22[T22 SettingsDialog]
    T23[T23 hint source + component] --> T24[T24 hint wiring]
    T25[T25 workspace version + script] --> T26[T26 PROTOCOL_VERSION 0.14.0]
    T25 --> T28[T28 desktop manifests]
    T26 --> T27[T27 sdk version]
    T27 --> T29[T29 desktop lock + version gate]
    T28 --> T29
    T30[T30 docs]
    T0 --> T31[T31 summary + full gates]
    T1 --> T31
    T2 --> T31
    T3 --> T31
    T4 --> T31
    T5 --> T31
    T6 --> T31
    T7 --> T31
    T8 --> T31
    T9 --> T31
    T10 --> T31
    T11 --> T31
    T12 --> T31
    T13 --> T31
    T14 --> T31
    T15 --> T31
    T16 --> T31
    T17 --> T31
    T18 --> T31
    T19 --> T31
    T20 --> T31
    T21 --> T31
    T22 --> T31
    T23 --> T31
    T24 --> T31
    T25 --> T31
    T26 --> T31
    T27 --> T31
    T28 --> T31
    T29 --> T31
    T30 --> T31
```

## Parallel Execution Groups

- **Group A (no deps):** T0, T2, T5, T6, T18, T23, T25, T30
- **Group B (after A):** T1, T7, T19, T20, T21, T22, T24, T26, T28
- **Group C (after B):** T3, T8, T12, T15, T27
- **Group D (after C):** T4, T9, T29
- **Group E (after D):** T10
- **Group F (after E):** T11, T13, T16
- **Group G (after F):** T14, T17
- **Group H (final):** T31

> Workstream note: ① Rust metrics (T0–T1) and ② build/kernel (T2–T5) share `web.rs` (T1 → T3 → T4 strictly sequential) but are otherwise disjoint. ③ protocol/TUI (T6–T17) is a schema-ordered chain (`file.rs` T6 → T7; `session.rs` T9 → T10; each regen scoped so only the listed schema files change) plus `lib.rs` serialization (T7 → T10 → T13 → T14) and `app.tsx` serialization (T15 → T16) — both now encoded as explicit `depends_on` edges. ④ Web (T18–T24) is fully independent of ①②③. The version chain (T25 → T26 → T27 / T28 → T29) only asserts `check-version.sh` at T29; docs (T30) can land any time; T31 must be last.
