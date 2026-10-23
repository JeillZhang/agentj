#!/usr/bin/env bash
# Repo invariants for CI workflows, governance files and documentation.
# Usage: assert-ci.sh [--list|all|<check>]
# Requires python3 with PyYAML for the YAML parsing checks.
set -euo pipefail

ROOT="${AGENTJ_ROOT:-$(cd -- "$(dirname "$0")/../.." && pwd)}"
CHECKS=(harness ci-jobs coverage-gate frontend-job governance-files hygiene docs-summaries docs-agents)

# --- Check inventory contract (maintained by hand, NOT a cache) ---------------
# EXPECTED_* lists the check inventory each harness is required to expose. It is a
# contract, not a cache: adding or removing a check in ANY of the three scripts
# (assert-ui.sh / assert-cargo.sh / assert-ci.sh) MUST update the matching list
# below in the same commit. check_harness fails when the `--list` output, the
# `check_<name>()` implementations and these lists disagree in either direction,
# so "one check silently dropped" can no longer look like "everything green".
EXPECTED_UI_CHECKS="ui-manifests lockfile-root single-lockfile"
EXPECTED_CARGO_CHECKS="cargo-root crate-inheritance licenses deny-config lockfile-locked"
EXPECTED_CI_CHECKS="harness ci-jobs coverage-gate frontend-job governance-files hygiene docs-summaries docs-agents"

bad() { printf 'FAIL %s: %s\n' "$1" "$2"; return 1; }
ok() { printf 'PASS %s\n' "$1"; }
# Exit code 3 means "not implemented yet"; `all` reports it as SKIP so the
# harness can be wired into CI before every assertion has landed.
pending() { printf 'SKIP %s: not implemented yet\n' "$1"; return 3; }

have_pyyaml() {
  python3 -c 'import yaml' >/dev/null 2>&1
}

# verify_inventory <script> <expected-space-separated> <reporting-check-name>
# Silent on success. Fails when scripts/tests/<script> is missing, does not parse,
# disagrees with its pinned inventory, or lists a check it never implements.
verify_inventory() {
  local script=$1 expected=$2 name=$3
  local listing sorted_expected sorted_listing missing extra item fn
  if [ ! -f "$ROOT/scripts/tests/$script" ]; then
    bad "$name" "missing scripts/tests/$script"
    return 1
  fi
  if ! bash -n "$ROOT/scripts/tests/$script" >/dev/null 2>&1; then
    bad "$name" "syntax error in scripts/tests/$script"
    return 1
  fi
  if ! listing=$(bash "$ROOT/scripts/tests/$script" --list); then
    bad "$name" "scripts/tests/$script --list exited non-zero"
    return 1
  fi
  sorted_expected=$(printf '%s\n' $expected | sort -u)
  sorted_listing=$(printf '%s\n' $listing | sort -u)
  missing=$(comm -23 <(printf '%s\n' "$sorted_expected") <(printf '%s\n' "$sorted_listing") \
    | tr '\n' ' ' | sed 's/ *$//')
  extra=$(comm -13 <(printf '%s\n' "$sorted_expected") <(printf '%s\n' "$sorted_listing") \
    | tr '\n' ' ' | sed 's/ *$//')
  if [ -n "$missing" ] || [ -n "$extra" ]; then
    printf 'FAIL %s: %s --list disagrees with the pinned inventory (missing: %s; unexpected: %s)\n' \
      "$name" "$script" "${missing:-none}" "${extra:-none}"
    return 1
  fi
  for item in $expected; do
    fn="check_${item//-/_}"
    if ! grep -q "^${fn}() {" "$ROOT/scripts/tests/$script"; then
      bad "$name" "$script lists '$item' but defines no ${fn}()"
      return 1
    fi
  done
}

check_harness() {
  local name=harness
  verify_inventory assert-ui.sh "$EXPECTED_UI_CHECKS" "$name" || return 1
  verify_inventory assert-cargo.sh "$EXPECTED_CARGO_CHECKS" "$name" || return 1
  verify_inventory assert-ci.sh "$EXPECTED_CI_CHECKS" "$name" || return 1
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


def coverage_step_runs():
    """Yield each coverage step's `run` on its own, newlines folded to spaces.

    Joining the steps first turns the list into a single string, which makes any
    ``startswith`` test over ``splitlines()`` vacuous (BUG-002).
    """
    for step in jobs["coverage"].get("steps") or []:
        run = str(step.get("run", ""))
        yield " ".join(line.strip() for line in run.splitlines() if line.strip())


cov_runs = " ".join(coverage_step_runs())
if "--fail-under-lines" not in cov_runs or "${{ env.COVERAGE_THRESHOLD }}" not in cov_runs:
    err("coverage job must gate on ${{ env.COVERAGE_THRESHOLD }}")

saw_llvm_cov = False
saw_fixture_build = False
for run in coverage_step_runs():
    if run.startswith("cargo llvm-cov"):
        saw_llvm_cov = True
        if "--locked" not in run:
            err(f"cargo llvm-cov must run with --locked (got: {run})")
    elif run.startswith("cargo build") and "agentj-plugin-fixture" in run:
        saw_fixture_build = True
        if "--locked" not in run:
            err(f"the plugin-fixture build must run with --locked (got: {run})")
if not saw_llvm_cov:
    err("coverage job must run `cargo llvm-cov`")
if not saw_fixture_build:
    err("coverage job must build the plugin fixture (`cargo build ... -p agentj-plugin-fixture`)")
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
    "npm --prefix agentj-ui test --workspace=@agentj/protocol",
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

# Ordered contract: the 17 gate steps must appear in exactly this order,
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
  # The local gates must be the CI gates, verbatim, or "CI runs the same commands"
  # is a lie and contributors get a green local run followed by a red CI run.
  grep -qF 'cargo deny check advisories licenses bans sources' "$ROOT/CONTRIBUTING.md" \
    || { bad "$name" "CONTRIBUTING.md must run 'cargo deny check advisories licenses bans sources' (the CI deny gate)"; return 1; }
  local cov_out
  if ! cov_out=$(python3 - "$ROOT" <<'PY'
import pathlib
import re
import sys

root = pathlib.Path(sys.argv[1])
ci = (root / ".github" / "workflows" / "ci.yml").read_text()
contrib = (root / "CONTRIBUTING.md").read_text()

ci_match = re.search(r"COVERAGE_THRESHOLD:\s*(\d+)", ci)
if not ci_match:
    print("ci.yml defines no COVERAGE_THRESHOLD for the coverage ratchet")
    sys.exit(1)
ci_value = ci_match.group(1)

doc_values = re.findall(r"--fail-under-lines\s+(\d+)", contrib)
if not doc_values:
    print("CONTRIBUTING.md documents no --fail-under-lines coverage gate")
    sys.exit(1)
for value in doc_values:
    if value != ci_value:
        print(
            "CONTRIBUTING.md --fail-under-lines "
            f"{value} != CI COVERAGE_THRESHOLD {ci_value}"
            " (the local coverage gate must match the CI ratchet)"
        )
        sys.exit(1)
print("ok")
PY
  ); then
    bad "$name" "$cov_out"
    return 1
  fi
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
  local out
  if ! out=$(python3 - "$ROOT" <<'PY'
import pathlib
import re
import sys

root = pathlib.Path(sys.argv[1])
SE = root / ".agents" / "summary" / "summary_and_next_se.md"
FE = root / ".agents" / "summary" / "summary_and_next_fe.md"
LOG = root / "docs" / "changelog" / "v0.1.md"

ERRORS = []


def fail(msg):
    ERRORS.append(msg)


def pending_section(path, label):
    if not path.is_file():
        fail(f"missing {path.relative_to(root)}")
        return ""
    text = path.read_text()
    match = re.search(r"^## 待实施.*$", text, re.M)
    if match is None:
        fail(f"{label}: no `## 待实施` section")
        return ""
    rest = text[match.end():]
    nxt = re.search(r"^## ", rest, re.M)
    return rest[: nxt.start()] if nxt else rest


se_all = SE.read_text() if SE.is_file() else ""
fe_all = FE.read_text() if FE.is_file() else ""
se = pending_section(SE, "se summary")
fe = pending_section(FE, "fe summary")

FORBIDDEN_COMMON = [
    "check-version.sh 保持红",
    "GitHub 权威",
    "与 AGENTS.md 的远端命名相反",
    # T22 renamed the remotes; a summary claiming origin=GitCode is stale on sight.
    "命名**相反**",
    "`origin` = `git@gitcode.com",
]
for label, text in (("se summary", se_all), ("fe summary", fe_all)):
    for stale in FORBIDDEN_COMMON:
        if stale in text:
            fail(f"{label}: stale statement still present: {stale}")
    if "gitcode" not in text:
        fail(f"{label}: must document that `gitcode` is the read-only mirror")
    if "origin" not in text:
        fail(f"{label}: must document that `origin` is the authoritative remote")

se_keep = ["⑥", "⑩", "⑪", "⑫", "⑬", "⑯", "⑰", "㉗", "㉘", "㉙", "㉚"]
se_drop = ["⑮", "⑳", "㉛", "㉜"]
for symbol in se_keep:
    if symbol not in se:
        fail(f"se 待实施 must keep row {symbol}")
for symbol in se_drop:
    if symbol in se:
        fail(f"se 待实施 must drop implemented row {symbol}")
for token in ("dmg", "msi"):
    if token not in se.lower():
        fail(f"se 待实施 must carry the desktop packaging row ({token})")

fe_keep = ["③", "㉓", "㉔"]
fe_drop = ["㉑", "㉒", "㉕", "㉝", "㉞"]
for symbol in fe_keep:
    if symbol not in fe:
        fail(f"fe 待实施 must keep row {symbol}")
for symbol in fe_drop:
    if symbol in fe:
        fail(f"fe 待实施 must drop implemented row {symbol}")

if not LOG.is_file():
    fail("missing docs/changelog/v0.1.md")
else:
    log = LOG.read_text()
    for section in ("### Added", "### Changed", "### Fixed"):
        if section not in log:
            fail(f"changlog v0.1.md is missing {section}")
    for topic, needle in (
        ("npm lockfile consolidation", "lockfile"),
        ("workspace cargo policy", "workspace.dependencies"),
        ("workspace lint policy", "workspace.lints"),
        ("licensing", "LICENSE-MIT"),
        ("contribution guide", "CONTRIBUTING.md"),
        ("security policy", "SECURITY.md"),
        ("dependabot", "dependabot.yml"),
        ("cargo-deny policy", "deny.toml"),
        ("assertion harness", "scripts/tests/assert-"),
        ("hygiene", ".opencode"),
        ("remote naming", "gitcode"),
    ):
        if needle not in log:
            fail(f"changlog v0.1.md is missing the {topic} entry ({needle})")
    if "0.2.0" in log:
        fail("changlog v0.1.md must not claim a version bump (0.2.0)")
    if re.search(r"版本号升至|bump(ped)? (the )?version", log):
        fail("changlog v0.1.md must not claim a version bump")

if ERRORS:
    print("; ".join(ERRORS))
    sys.exit(1)
print("ok")
PY
  ); then
    bad "$name" "$out"
    return 1
  fi
  ok "$name"
}

check_docs_agents() {
  local name=docs-agents
  local agents="$ROOT/AGENTS.md" readme="$ROOT/README.md"
  local failed=0

  for f in "$agents" "$readme"; do
    if [ ! -f "$f" ]; then
      bad "$name" "missing ${f#"$ROOT"/}"
      return 1
    fi
  done

  require() {
    local file=$1 needle=$2 why=$3
    if ! grep -qF -- "$needle" "$file"; then
      bad "$name" "$(basename "$file"): missing $why ($needle)"
      failed=1
    fi
  }
  forbid() {
    local file=$1 needle=$2 why=$3
    if grep -qF -- "$needle" "$file"; then
      bad "$name" "$(basename "$file"): $why ($needle)"
      failed=1
    fi
  }

  require "$agents" 'cargo deny check advisories licenses bans sources' 'the cargo-deny gate command'
  require "$agents" 'npm --prefix agentj-ui run lint --workspace=@agentj/web' 'the oxlint gate command'
  require "$agents" 'scripts/tests/assert-' 'the repository assertion harness'
  require "$agents" 'AGENTJ_ROOT' 'the check-version.sh override'
  require "$agents" '93.67%' 'the measured coverage baseline'
  require "$agents" '2026-10-23' 'the coverage measurement date'
  require "$agents" 'PROTOCOL_VERSION' 'the protocol baseline'

  local job_out
  if ! job_out=$(python3 - "$agents" <<'PY2'
import pathlib
import sys

text = pathlib.Path(sys.argv[1]).read_text()
marker = "### CI Pipelines"
if marker not in text:
    print("AGENTS.md has no CI Pipelines section")
    sys.exit(1)
section = text[text.index(marker):]
section = section.split("\n## ")[0]
missing = [job for job in ("frontend", "deny", "cross-platform") if job not in section]
if missing:
    print("CI Pipelines does not list: " + ", ".join(missing))
    sys.exit(1)
PY2
  ); then
    bad "$name" "$job_out"
    failed=1
  fi

  require "$readme" 'CONTRIBUTING.md' 'the contributing guide pointer'
  if ! grep -qE 'MIT OR Apache-2\.0|Apache-2\.0' "$readme"; then
    bad "$name" "README.md: missing the MIT OR Apache-2.0 licence statement"
    failed=1
  fi

  forbid "$agents" '0.2.0' 'a version bump claim is not allowed here'
  forbid "$readme" '0.2.0' 'a version bump claim is not allowed here'

  [ "$failed" -eq 0 ] || return 1
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
