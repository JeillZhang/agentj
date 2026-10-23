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
  if ! have_pyyaml; then
    bad "$name" "python3 PyYAML is required to parse .github/workflows/ci.yml"
    return 1
  fi
  local out
  if ! out=$(python3 - "$ROOT" <<'PY'
import math
import pathlib
import re
import sys
import yaml

root = pathlib.Path(sys.argv[1])


def err(msg):
    print(msg)
    sys.exit(1)


path = root / ".github" / "workflows" / "ci.yml"
text = path.read_text()
wf = yaml.safe_load(text)
jobs = wf.get("jobs") or {}
if "coverage" not in jobs:
    err("ci.yml has no coverage job")
threshold = (wf.get("env") or {}).get("COVERAGE_THRESHOLD")
if not isinstance(threshold, int):
    err(f"env.COVERAGE_THRESHOLD must be an integer (got {threshold!r})")
if threshold < 50:
    err(f"COVERAGE_THRESHOLD must never drop below 50 (got {threshold})")

# The ratchet baseline must be recorded next to the value: date + measured X%.
lines = text.splitlines()
comment = ""
for idx, line in enumerate(lines):
    if re.match(r"\s*COVERAGE_THRESHOLD\s*:", line):
        comment = "\n".join(lines[max(0, idx - 4):idx])
        break
if "ratchet" not in comment.lower():
    err("COVERAGE_THRESHOLD needs a ratchet comment above it")
match = re.search(r"(\d{4}-\d{2}-\d{2}).*?(\d+(?:\.\d+)?)%", comment)
if match is None:
    err("the ratchet comment must record the measurement date and the measured coverage X%")
measured = float(match.group(2))
expected = max(50, math.floor(measured) - 1)
if threshold != expected:
    err(f"COVERAGE_THRESHOLD must be max(50, floor({measured}) - 1) = {expected} (got {threshold})")

cov_runs = " ".join(
    str(step.get("run", "")) for step in (jobs["coverage"].get("steps") or [])
)
if "--fail-under-lines" not in cov_runs or "${{ env.COVERAGE_THRESHOLD }}" not in cov_runs:
    err("coverage job must gate on ${{ env.COVERAGE_THRESHOLD }}")
for line in cov_runs.splitlines():
    stripped = line.strip()
    if stripped.startswith("cargo llvm-cov") and "--locked" not in stripped:
        err("cargo llvm-cov must run with --locked")
    if stripped.startswith("cargo build -p agentj-plugin-fixture") and "--locked" not in stripped:
        err("the plugin-fixture build must run with --locked")
print("ok")
PY
  ); then
    bad "$name" "$out"
    return 1
  fi
  ok "$name"
}

check_frontend_job() {
  local name=frontend-job
  if ! have_pyyaml; then
    bad "$name" "python3 PyYAML is required to parse .github/workflows/ci.yml"
    return 1
  fi
  local out
  if ! out=$(python3 - "$ROOT" <<'PY'
import json
import pathlib
import sys
import yaml

root = pathlib.Path(sys.argv[1])

EXPECTED = [
    "npm --prefix agentj-ui ci",
    "npm --prefix agentj-ui run generate --workspace=@agentj/protocol",
    "git diff --exit-code -- agentj-ui/packages/protocol/types.d.ts",
    "npm --prefix agentj-ui run build --workspace=@agentj/protocol",
    "npm --prefix agentj-ui run generate --workspace=@agentj/sdk",
    "git diff --exit-code -- agentj-ui/packages/sdk/src/methods.ts",
    "npm --prefix agentj-ui test --workspace=@agentj/sdk",
    "npm --prefix agentj-ui test --workspace=@agentj/tokens",
    "npm --prefix agentj-ui test --workspace=@agentj/web",
    "npm --prefix agentj-ui run build --workspace=@agentj/web",
    "dist <-> protocol version",
    "npm --prefix agentj-ui run lint --workspace=@agentj/web",
    "bun --cwd agentj-ui/apps/tui test",
    "node agentj-ui/apps/desktop/check-desktop-ui.mjs",
    "./scripts/check-version.sh",
    "node scripts/check-relocate.mjs",
]


def err(msg):
    print(msg)
    sys.exit(1)


path = root / ".github" / "workflows" / "ci.yml"
if not path.is_file():
    err("missing .github/workflows/ci.yml")
wf = yaml.safe_load(path.read_text())
job = (wf.get("jobs") or {}).get("frontend")
if job is None:
    err("ci.yml has no frontend job")
steps = job.get("steps") or []
if job.get("timeout-minutes") != 20:
    err(f"frontend timeout-minutes must be 20 (got {job.get('timeout-minutes')!r})")
if job.get("runs-on") != "ubuntu-latest":
    err(f"frontend must run on ubuntu-latest (got {job.get('runs-on')!r})")

node = [s for s in steps if str(s.get("uses", "")).startswith("actions/setup-node")]
if len(node) != 1:
    err("frontend needs exactly one actions/setup-node step")
node_with = node[0].get("with") or {}
if str(node_with.get("node-version")) != "24":
    err(f"setup-node node-version must be 24 (got {node_with.get('node-version')!r})")
if node_with.get("cache") != "npm":
    err(f"setup-node cache must be npm (got {node_with.get('cache')!r})")
if node_with.get("cache-dependency-path") != "agentj-ui/package-lock.json":
    err(
        "setup-node cache-dependency-path must be agentj-ui/package-lock.json "
        f"(got {node_with.get('cache-dependency-path')!r})"
    )
bun = [s for s in steps if str(s.get("uses", "")).startswith("oven-sh/setup-bun")]
if len(bun) != 1:
    err("frontend needs exactly one oven-sh/setup-bun step")
bun_with = bun[0].get("with") or {}
if str(bun_with.get("bun-version")) != "1.4.0":
    err(f"setup-bun bun-version must be 1.4.0 (got {bun_with.get('bun-version')!r})")

# Ordered contract: the 16 gate steps must appear in exactly this order,
# interleaved only with the two setup actions.
observed = []
for step in steps:
    run = str(step.get("run", "")).strip()
    if not run:
        continue
    flat = " ".join(line.strip() for line in run.splitlines() if line.strip())
    name_field = str(step.get("name", "")).strip()
    if flat.startswith("node -e"):
        observed.append("dist <-> protocol version")
    elif name_field == "dist <-> protocol version":
        observed.append("dist <-> protocol version")
    else:
        observed.append(flat)
if observed != EXPECTED:
    missing = [e for e in EXPECTED if e not in observed]
    if missing:
        err("frontend job is missing steps: " + "; ".join(missing))
    err(f"frontend steps are out of order: {observed}")

pkg = json.loads((root / "agentj-ui" / "packages" / "protocol" / "package.json").read_text())
if pkg.get("scripts", {}).get("test") != "vitest run":
    err("@agentj/protocol package.json must define \"test\": \"vitest run\"")
print("ok")
PY
  ); then
    bad "$name" "$out"
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
