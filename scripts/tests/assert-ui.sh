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
  local root="$ROOT/agentj-ui/package.json"
  local want
  [ -f "$root" ] || { bad "$name" "missing agentj-ui/package.json"; return 1; }
  want=$(jq -r '.name // ""' "$root")
  [ "$want" = agentj-ui ] || { bad "$name" "name must be agentj-ui (got '$want')"; return 1; }
  want=$(jq -r '.version // ""' "$root")
  [ "$want" = 0.1.0 ] || { bad "$name" "version must be 0.1.0 (got '$want')"; return 1; }
  want=$(jq -r '.packageManager // ""' "$root")
  [ "$want" = npm@11.11.0 ] || { bad "$name" "packageManager must be npm@11.11.0 (got '$want')"; return 1; }
  want=$(jq -r '.engines.node // ""' "$root")
  [ "$want" = '>=24' ] || { bad "$name" "engines.node must be >=24 (got '$want')"; return 1; }
  want=$(jq -r '.private // false' "$root")
  [ "$want" = true ] || { bad "$name" "private must be true (got '$want')"; return 1; }
  local pkg
  for pkg in apps/web apps/tui; do
    [ -f "$ROOT/agentj-ui/$pkg/package.json" ] || { bad "$name" "missing agentj-ui/$pkg/package.json"; return 1; }
    want=$(jq -r '.version // ""' "$ROOT/agentj-ui/$pkg/package.json")
    [ "$want" = 0.1.0 ] || { bad "$name" "$pkg version must be 0.1.0 (got '$want')"; return 1; }
  done
  ok "$name"
}

check_lockfile_root() {
  local name=lockfile-root
  local lock="$ROOT/agentj-ui/package-lock.json"
  local want
  [ -f "$lock" ] || { bad "$name" "missing agentj-ui/package-lock.json"; return 1; }
  want=$(jq -r '.packages[""].name // ""' "$lock")
  [ "$want" = agentj-ui ] || { bad "$name" "root entry name must be agentj-ui (got '$want')"; return 1; }
  want=$(jq -r '.packages[""].version // ""' "$lock")
  [ "$want" = 0.1.0 ] || { bad "$name" "root entry version must be 0.1.0 (got '$want')"; return 1; }
  want=$(jq -r '.packages["apps/desktop"].version // ""' "$lock")
  [ "$want" = 0.1.0 ] || { bad "$name" "apps/desktop version must be 0.1.0 (got '$want')"; return 1; }
  ok "$name"
}

check_single_lockfile() {
  local name=single-lockfile
  local locks count
  locks=$(git -C "$ROOT" ls-files \
    | grep -E '^agentj-ui/.*(package-lock\.json|bun\.lock|yarn\.lock|pnpm-lock\.yaml)$' || true)
  count=$(printf '%s' "$locks" | grep -c . || true)
  if [ "$count" -ne 1 ] || [ "$locks" != agentj-ui/package-lock.json ]; then
    bad "$name" "expected exactly agentj-ui/package-lock.json, got: $(printf '%s' "$locks" | tr '\n' ' ')"
    return 1
  fi
  ok "$name"
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
