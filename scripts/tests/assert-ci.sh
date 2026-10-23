#!/usr/bin/env bash
# Repo invariants for CI workflows, governance files and documentation.
# Usage: assert-ci.sh [--list|all|<check>]
# Requires python3 with PyYAML for the YAML parsing checks.
set -euo pipefail

ROOT="${AGENTJ_ROOT:-$(cd -- "$(dirname "$0")/../.." && pwd)}"
CHECKS=(harness ci-jobs coverage-gate frontend-job governance-files hygiene docs-summaries docs-agents)

bad() { printf 'FAIL %s: %s\n' "$1" "$2"; return 1; }
ok() { printf 'PASS %s\n' "$1"; }

have_pyyaml() {
  python3 -c 'import yaml' >/dev/null 2>&1
}

check_harness() {
  local name=harness
  local f
  for f in assert-ui.sh assert-cargo.sh; do
    if [ ! -f "$ROOT/scripts/tests/$f" ]; then
      bad "$name" "missing scripts/tests/$f"
      return 1
    fi
    if ! bash -n "$ROOT/scripts/tests/$f" >/dev/null 2>&1; then
      bad "$name" "syntax error in scripts/tests/$f"
      return 1
    fi
  done
  if ! bash -n "$ROOT/scripts/tests/assert-ci.sh" >/dev/null 2>&1; then
    bad "$name" "syntax error in scripts/tests/assert-ci.sh"
    return 1
  fi
  local expected_ui="ui-manifests lockfile-root single-lockfile "
  local expected_cargo="cargo-root crate-inheritance licenses deny-config lockfile-locked "
  local got
  got=$(bash "$ROOT/scripts/tests/assert-ui.sh" --list | tr '\n' ' ')
  if [ "$got" != "$expected_ui" ]; then
    bad "$name" "assert-ui.sh --list mismatch: $got"
    return 1
  fi
  got=$(bash "$ROOT/scripts/tests/assert-cargo.sh" --list | tr '\n' ' ')
  if [ "$got" != "$expected_cargo" ]; then
    bad "$name" "assert-cargo.sh --list mismatch: $got"
    return 1
  fi
  if bash "$ROOT/scripts/tests/assert-ui.sh" nope >/dev/null 2>&1; then
    bad "$name" "unknown check must exit 2"
    return 1
  fi
  ok "$name"
}

check_ci_jobs() {
  local name=ci-jobs
  bad "$name" "not implemented yet"
  return 1
}

check_coverage_gate() {
  local name=coverage-gate
  bad "$name" "not implemented yet"
  return 1
}

check_frontend_job() {
  local name=frontend-job
  bad "$name" "not implemented yet"
  return 1
}

check_governance_files() {
  local name=governance-files
  bad "$name" "not implemented yet"
  return 1
}

check_hygiene() {
  local name=hygiene
  local stale
  for stale in "$ROOT"/agentj-ui/node_modules/.old_modules-*; do
    if [ -d "$stale" ]; then
      bad "$name" "stale install residue present: ${stale#$ROOT/}"
      return 1
    fi
  done
  if [ -d "$ROOT/node_modules" ]; then
    bad "$name" "root node_modules/ should be gone"
    return 1
  fi
  if [ -d "$ROOT/.playwright-mcp" ]; then
    bad "$name" ".playwright-mcp/ should be gone"
    return 1
  fi
  if ! grep -qxF '.opencode/' "$ROOT/.gitignore"; then
    bad "$name" ".gitignore must carry a tracked .opencode/ rule"
    return 1
  fi
  ok "$name"
}

check_docs_summaries() {
  local name=docs-summaries
  bad "$name" "not implemented yet"
  return 1
}

check_docs_agents() {
  local name=docs-agents
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
