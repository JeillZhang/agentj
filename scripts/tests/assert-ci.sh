#!/usr/bin/env bash
# Repo invariants for CI workflows, governance files and documentation.
# Usage: assert-ci.sh [--list|all|<check>]
# Requires python3 with PyYAML for the YAML parsing checks.
set -euo pipefail

ROOT="${AGENTJ_ROOT:-$(cd -- "$(dirname "$0")/../.." && pwd)}"
CHECKS=(harness ci-jobs coverage-gate frontend-job governance-files hygiene docs-summaries docs-agents)

bad() { printf 'FAIL %s: %s\n' "$1" "$2"; return 1; }
ok() { printf 'PASS %s\n' "$1"; }
# Exit code 3 means "not implemented yet"; `all` reports it as SKIP so the
# harness can be wired into CI before every assertion has landed.
pending() { printf 'SKIP %s: not implemented yet\n' "$1"; return 3; }

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
  if ! have_pyyaml; then
    bad "$name" "python3 PyYAML is required to parse .github/workflows/ci.yml"
    return 1
  fi
  local out
  if ! out=$(python3 - "$ROOT" <<'PY'
import pathlib
import sys
import yaml

root = pathlib.Path(sys.argv[1])


def err(msg):
    print(msg)
    sys.exit(1)


path = root / ".github" / "workflows" / "ci.yml"
if not path.is_file():
    err("missing .github/workflows/ci.yml")
wf = yaml.safe_load(path.read_text())
jobs = wf.get("jobs") or {}
needed = {"lint", "test", "coverage", "deny", "cross-platform"}
missing = sorted(needed - set(jobs))
if missing:
    err("ci.yml is missing jobs: " + ", ".join(missing))


def runs(job):
    return [str(step.get("run", "")) for step in (jobs[job].get("steps") or [])]


deny_runs = " ".join(runs("deny"))
if "cargo deny check" not in deny_runs:
    err("deny job must run `cargo deny check`")
for check in ("advisories", "licenses", "bans", "sources"):
    if check not in deny_runs:
        err(f"deny job must check {check}")

matrix = (jobs["cross-platform"].get("strategy") or {}).get("matrix") or {}
os_list = matrix.get("os")
want_os = ["ubuntu-latest", "macos-latest", "windows-latest"]
if os_list != want_os:
    err(f"cross-platform matrix.os must be {want_os} (got {os_list!r})")
cross = runs("cross-platform")
cargo_steps = [r.strip() for r in cross if r.strip().startswith("cargo")]
if cargo_steps != ["cargo build --workspace --locked"]:
    err(f"cross-platform must only build the workspace (got {cargo_steps})")
for step in cross:
    if "cargo test" in step or "tauri" in step:
        err("cross-platform must not run tests or the tauri bundle")

lint_runs = " ".join(runs("lint"))
for script in ("assert-ui.sh", "assert-cargo.sh", "assert-ci.sh"):
    if f"scripts/tests/{script} all" not in lint_runs:
        err(f"lint job must run scripts/tests/{script} all")
if "pyyaml" not in lint_runs:
    err("lint job must install the PyYAML prerequisite for assert-ci.sh")
print("ok")
PY
  ); then
    bad "$name" "$out"
    return 1
  fi
  ok "$name"
}

check_coverage_gate() {
  local name=coverage-gate
  pending "$name"
}

check_frontend_job() {
  local name=frontend-job
  pending "$name"
}

check_governance_files() {
  local name=governance-files
  local f
  for f in CONTRIBUTING.md SECURITY.md .github/dependabot.yml; do
    [ -s "$ROOT/$f" ] || { bad "$name" "missing or empty $f"; return 1; }
  done
  grep -q 'cargo test --workspace' "$ROOT/CONTRIBUTING.md" \
    || { bad "$name" "CONTRIBUTING.md must document the Rust test command"; return 1; }
  grep -q 'npm --prefix agentj-ui test' "$ROOT/CONTRIBUTING.md" \
    || { bad "$name" "CONTRIBUTING.md must document the frontend test command"; return 1; }
  grep -q 'docs/superpowers' "$ROOT/CONTRIBUTING.md" \
    || { bad "$name" "CONTRIBUTING.md must say docs/superpowers/ is local-only"; return 1; }
  grep -qi 'security advisories' "$ROOT/SECURITY.md" \
    || { bad "$name" "SECURITY.md must name the private advisory channel"; return 1; }
  if ! have_pyyaml; then
    bad "$name" "python3 PyYAML is required to validate .github/dependabot.yml"
    return 1
  fi
  local out
  if ! out=$(python3 - "$ROOT" <<'PY'
import pathlib
import sys
import yaml

root = pathlib.Path(sys.argv[1])


def err(msg):
    print(msg)
    sys.exit(1)


data = yaml.safe_load((root / ".github" / "dependabot.yml").read_text())
if data.get("version") != 2:
    err(f"dependabot version must be 2 (got {data.get('version')!r})")
updates = data.get("updates") or []
ecosystems = {}
for entry in updates:
    ecosystems[entry.get("package-ecosystem")] = entry
want = {"cargo", "npm", "github-actions"}
if set(ecosystems) != want:
    err(f"dependabot ecosystems must be {sorted(want)} (got {sorted(ecosystems)})")
npm = ecosystems["npm"]
if npm.get("directory") != "/agentj-ui":
    err(f"npm ecosystem directory must be /agentj-ui (got {npm.get('directory')!r})")
for eco, entry in ecosystems.items():
    groups = entry.get("groups") or {}
    if not groups:
        err(f"{eco} updates must be grouped")
    types = set()
    for group in groups.values():
        types.update(group.get("update-types") or [])
    if not {"minor", "patch"} <= types:
        err(f"{eco} groups must bundle minor and patch updates")
print("ok")
PY
  ); then
    bad "$name" "$out"
    return 1
  fi
  ok "$name"
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
  pending "$name"
}

check_docs_agents() {
  local name=docs-agents
  pending "$name"
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
  all | --strict)
    passed=0
    skipped=0
    failed=0
    for name in "${CHECKS[@]}"; do
      rc=0
      run_check "$name" || rc=$?
      case "$rc" in
        0) passed=$((passed + 1)) ;;
        3) skipped=$((skipped + 1)) ;;
        *) failed=$((failed + 1)) ;;
      esac
    done
    printf '%s passed, %s skipped, %s failed\n' "$passed" "$skipped" "$failed"
    [ "$failed" -eq 0 ] || exit 1
    if [ "$cmd" = --strict ] && [ "$skipped" -ne 0 ]; then
      printf 'strict mode: %s check(s) still unimplemented\n' "$skipped" >&2
      exit 1
    fi
    ;;
  *)
    run_check "$cmd" "$@"
    ;;
esac
