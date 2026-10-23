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
  bad "$name" "not implemented yet"
  return 1
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
