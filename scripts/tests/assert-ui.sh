#!/usr/bin/env bash
# Repo invariants for the agentj-ui workspace (manifests / lockfiles).
# Usage: assert-ui.sh [--list|all|<check>]
#   --list : print every check name, one per line
#   all    : run every check, print PASS/FAIL per check, then a summary line
#   <check>: run a single check (unknown name exits 2)
set -euo pipefail

ROOT="${AGENTJ_ROOT:-$(cd -- "$(dirname "$0")/../.." && pwd)}"
CHECKS=(ui-manifests lockfile-root single-lockfile)

bad() { printf 'FAIL %s: %s\n' "$1" "$2"; return 1; }
ok() { printf 'PASS %s\n' "$1"; }

check_ui_manifests() {
  local name=ui-manifests
  bad "$name" "not implemented yet"
  return 1
}

check_lockfile_root() {
  local name=lockfile-root
  bad "$name" "not implemented yet"
  return 1
}

check_single_lockfile() {
  local name=single-lockfile
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
