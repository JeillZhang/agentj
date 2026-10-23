#!/usr/bin/env bash
# Repo invariants for the Rust workspace (manifests / policy / licences).
# Usage: assert-cargo.sh [--list|all|<check>] [args]
#   --list : print every check name, one per line
#   all    : run every check, print PASS/FAIL per check, then a summary line
#   <check>: run a single check (unknown name exits 2);
#            `crate-inheritance [crate...]` accepts one or more crate names
set -euo pipefail

ROOT="${AGENTJ_ROOT:-$(cd -- "$(dirname "$0")/../.." && pwd)}"
CHECKS=(cargo-root crate-inheritance licenses deny-config lockfile-locked)

bad() { printf 'FAIL %s: %s\n' "$1" "$2"; return 1; }
ok() { printf 'PASS %s\n' "$1"; }

check_cargo_root() {
  local name=cargo-root
  local out
  if ! out=$(python3 - "$ROOT" <<'PY'
import pathlib
import sys
import tomllib

root = pathlib.Path(sys.argv[1])


def err(msg):
    print(msg)
    sys.exit(1)


with open(root / "Cargo.toml", "rb") as fh:
    cargo = tomllib.load(fh)
ws = cargo.get("workspace", {})
deps = ws.get("dependencies", {})
needed = [
    "serde", "serde_json", "tokio", "thiserror", "tracing", "uuid",
    "async-trait", "futures", "tempfile", "base64", "wiremock", "libloading",
]
missing = [d for d in needed if d not in deps]
if missing:
    err("workspace.dependencies missing: " + ", ".join(missing))
clippy_all = ws.get("lints", {}).get("clippy", {}).get("all")
want = {"level": "warn", "priority": -1}
if clippy_all != want:
    err(f"workspace.lints.clippy.all must be {want} (got {clippy_all!r})")
release = cargo.get("profile", {}).get("release", {})
if release.get("lto") != "thin":
    err(f"profile.release.lto must be 'thin' (got {release.get('lto')!r})")
if release.get("codegen-units") != 1:
    err(f"profile.release.codegen-units must be 1 (got {release.get('codegen-units')!r})")
if "panic" in release:
    err("profile.release must not set panic (catch_unwind must stay usable)")
pkg = ws.get("package", {})
if pkg.get("license") != "MIT OR Apache-2.0":
    err(f"workspace.package.license must be MIT OR Apache-2.0 (got {pkg.get('license')!r})")
msrv = pkg.get("rust-version")
clippy_toml = root / "clippy.toml"
if not clippy_toml.is_file():
    err("clippy.toml missing")
with open(clippy_toml, "rb") as fh:
    clippy_cfg = tomllib.load(fh)
if str(clippy_cfg.get("msrv")) != str(msrv):
    err(f"clippy.toml msrv {clippy_cfg.get('msrv')!r} != workspace rust-version {msrv!r}")
print("ok")
PY
  ); then
    bad "$name" "$out"
    return 1
  fi
  ok "$name"
}

check_crate_inheritance() {
  local name=crate-inheritance
  bad "$name" "not implemented yet"
  return 1
}

check_licenses() {
  local name=licenses
  bad "$name" "not implemented yet"
  return 1
}

check_deny_config() {
  local name=deny-config
  bad "$name" "not implemented yet"
  return 1
}

check_lockfile_locked() {
  local name=lockfile-locked
  bad "$name" "not implemented yet"
  return 1
}

run_check() {
  local name="$1"
  shift
  local fn="check_${name//-/_}"
  if ! declare -F "$fn" >/dev/null; then
    printf 'unknown check: %s\n' "$name" >&2
    exit 2
  fi
  "$fn" "$@"
}

cmd="${1:-all}"
shift || true
case "$cmd" in
  --list)
    printf '%s\n' "${CHECKS[@]}"
    exit 0
    ;;
  all)
    passed=0
    failed=0
    for name in "${CHECKS[@]}"; do
      if run_check "$name"; then passed=$((passed + 1)); else failed=$((failed + 1)); fi
    done
    printf '%s passed, %s failed\n' "$passed" "$failed"
    [ "$failed" -eq 0 ] || exit 1
    ;;
  *)
    run_check "$cmd" "$@"
    ;;
esac
