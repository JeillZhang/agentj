#!/usr/bin/env bash
# Repo invariants for the Rust workspace (manifests / policy / licences).
# Usage: assert-cargo.sh [--list|all|<check>] [args]
#   --list : print every check name, one per line
#   all    : run every check, print PASS/FAIL per check, then a summary line
#   <check>: run a single check (unknown name exits 2);
#            `crate-inheritance [crate...]` accepts one or more crate names
set -euo pipefail

ROOT="${AGENTJ_ROOT:-$(cd -- "$(dirname "$0")/../.." && pwd)}"
CHECKS=(cargo-root crate-inheritance licenses deny-config lockfile-locked fn-length)

bad() { printf 'FAIL %s: %s\n' "$1" "$2"; return 1; }
ok() { printf 'PASS %s\n' "$1"; }
# Exit code 3 means "not implemented yet"; `all` reports it as SKIP so the
# harness can be wired into CI before every assertion has landed.
pending() { printf 'SKIP %s: not implemented yet\n' "$1"; return 3; }

check_cargo_root() {
  local name=cargo-root
  local out
  if ! out=$(python3 - "$ROOT" <<'PY'
import pathlib
import re
import subprocess
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
  local out
  if ! out=$(python3 - "$ROOT" <<'PY'
import pathlib
import re
import subprocess
import sys
import tomllib

root = pathlib.Path(sys.argv[1])
managed = {
    "serde", "serde_json", "tokio", "thiserror", "tracing", "uuid",
    "async-trait", "futures", "tempfile", "base64", "wiremock", "libloading",
}
sections = ("dependencies", "dev-dependencies", "build-dependencies")

manifests = sorted((root / "crates").glob("*/Cargo.toml"))
if len(manifests) != 18:
    print(f"expected 18 crate manifests, found {len(manifests)}")
    sys.exit(1)

for path in manifests:
    rel = path.relative_to(root)
    with open(path, "rb") as fh:
        doc = tomllib.load(fh)
    pkg = doc.get("package", {})
    for key in ("version", "edition", "rust-version", "license"):
        if pkg.get(key) != {"workspace": True}:
            print(f"{rel}: [package] {key}.workspace = true is required")
            sys.exit(1)
    if doc.get("lints") != {"workspace": True}:
        print(f"{rel}: [lints] workspace = true is required")
        sys.exit(1)
    for section in sections:
        for dep, spec in (doc.get(section) or {}).items():
            if dep not in managed:
                continue
            if not isinstance(spec, dict) or spec.get("workspace") is not True:
                print(f"{rel}: [{section}] {dep} must use {{ workspace = true }}")
                sys.exit(1)
            if "version" in spec:
                print(f"{rel}: [{section}] {dep} must not pin a version")
                sys.exit(1)
print("ok")
PY
  ); then
    bad "$name" "$out"
    return 1
  fi
  ok "$name"
}

check_licenses() {
  local name=licenses
  local f
  for f in LICENSE LICENSE-MIT LICENSE-APACHE; do
    [ -s "$ROOT/$f" ] || { bad "$name" "missing or empty $f"; return 1; }
  done
  grep -q 'MIT License' "$ROOT/LICENSE-MIT" \
    || { bad "$name" "LICENSE-MIT must contain the MIT licence text"; return 1; }
  grep -q 'Apache License' "$ROOT/LICENSE-APACHE" \
    || { bad "$name" "LICENSE-APACHE must contain the Apache licence text"; return 1; }
  grep -q 'Version 2.0' "$ROOT/LICENSE-APACHE" \
    || { bad "$name" "LICENSE-APACHE must be version 2.0"; return 1; }
  grep -q 'MIT' "$ROOT/LICENSE" && grep -q 'Apache' "$ROOT/LICENSE" \
    || { bad "$name" "LICENSE must state the dual MIT/Apache-2.0 licence"; return 1; }
  ok "$name"
}

check_deny_config() {
  local name=deny-config
  local out deny_out unmatched
  if ! out=$(python3 - "$ROOT" <<'PY'
import pathlib
import re
import subprocess
import sys
import tomllib

root = pathlib.Path(sys.argv[1])


def err(msg):
    print(msg)
    sys.exit(1)


path = root / "deny.toml"
if not path.is_file():
    err("deny.toml missing")
raw = path.read_text()
with open(path, "rb") as fh:
    cfg = tomllib.load(fh)
# A pointer to a gitignored document is a dead link for every fresh clone.
comments = " ".join(
    line for line in raw.splitlines() if line.lstrip().startswith("#")
)
if "docs/superpowers" in comments:
    err("deny.toml rationale must not point at the gitignored docs/superpowers/ tree")
# A pointer at a tracked-but-silent document is just as dead as a gitignored one.
# Repo-relative paths only: `docs/...` or a capitalised root document.
# URLs and bare lowercase filenames (`v0.1.md`) are mentions, not pointers.
pointers = sorted(set(re.findall(r"(?<![\w./:-])(docs/[\w./-]+|[A-Z][A-Z0-9_-]*\.md)", comments)))
if not pointers:
    err("deny.toml must point at the documentation that explains the policy")
work_tree = subprocess.run(
    ["git", "-C", str(root), "rev-parse", "--is-inside-work-tree"],
    capture_output=True,
    text=True,
)
tracked = work_tree.returncode == 0 and work_tree.stdout.strip() == "true"
documented = ""
for pointer in pointers:
    target = root / pointer
    if not target.is_file():
        err(f"deny.toml points at a missing file: {pointer}")
    if tracked:
        listed = subprocess.run(
            ["git", "-C", str(root), "ls-files", "--error-unmatch", "--", pointer],
            capture_output=True,
            text=True,
        )
        if listed.returncode != 0:
            err(f"deny.toml points at {pointer}, which git does not track")
    documented += target.read_text().lower()
for topic in ("allowlist", "licence", "duplicate"):
    if topic not in documented:
        err(f"deny.toml points at {', '.join(pointers)}, which never mentions {topic}")
for section in ("advisories", "licenses", "bans", "sources"):
    if section not in cfg:
        err(f"deny.toml must configure [{section}]")
# cargo-deny denies yanked releases by default; writing the key at all is only
# worth it to relax it, which would downgrade a hard block to a warning.
advisories = cfg["advisories"]
yanked = advisories.get("yanked", "deny")
if yanked != "deny":
    err(
        "advisories.yanked must be 'deny' (cargo-deny's default); got "
        f"{yanked!r} — yanked releases must block, not warn"
    )
licenses = cfg["licenses"]
if "default" in licenses:
    err("licenses.default was removed in cargo-deny 0.20 (unlisted licences are denied)")
allow = licenses.get("allow") or []
for needed in ("MIT", "Apache-2.0"):
    if needed not in allow:
        err(f"licenses.allow must include {needed}")
# The documented allowlist and the enforced one must not drift apart.
contributing = root / "CONTRIBUTING.md"
if contributing.is_file():
    doc = contributing.read_text()
    bullet = re.search(
        r"\*\*Licence allowlist\*\*(.*?)(?=\n- \*\*|\Z)", doc, re.S
    )
    if bullet is None:
        err("CONTRIBUTING.md must document the licence allowlist bullet")
    # The list itself stops at the trailing prose ("Anything else fails ...").
    listed = bullet.group(1).split("Anything else")[0]
    documented_allow = sorted(re.findall(r"`([^`]+)`", listed))
    if documented_allow != sorted(allow):
        err(
            "CONTRIBUTING.md allowlist "
            f"{documented_allow} != deny.toml allow {sorted(allow)}"
        )
bans = cfg["bans"]
if bans.get("multiple-versions") != "deny":
    err(f"bans.multiple-versions must be 'deny' (got {bans.get('multiple-versions')!r})")
if bans.get("wildcards") != "deny":
    err(f"bans.wildcards must be 'deny' (got {bans.get('wildcards')!r})")
sources = cfg["sources"]
crates_io = ["https://github.com/rust-lang/crates.io-index"]
if sources.get("allow-registry") != crates_io:
    err(f"sources.allow-registry must be {crates_io} (got {sources.get('allow-registry')!r})")
for key in ("unknown-registry", "unknown-git"):
    if sources.get(key) != "deny":
        err(f"sources.{key} must be 'deny' (got {sources.get(key)!r})")
print("ok")
PY
  ); then
    bad "$name" "$out"
    return 1
  fi
  # A skip entry that matches no duplicate is a silent blind spot: the duplicate
  # it claimed to explain is gone (or the rationale is stale). Require every
  # entry in bans.skip to still hit, i.e. no `unmatched-skip` warning.
  if ! command -v cargo-deny >/dev/null 2>&1 && ! cargo deny --version >/dev/null 2>&1; then
    printf 'SKIP %s: cargo-deny unavailable, cannot verify bans.skip entries still hit\n' "$name"
    return 3
  fi
  if ! deny_out=$(cd "$ROOT" && cargo deny --color never check bans 2>&1); then
    bad "$name" "cargo deny check bans failed: $(printf '%s' "$deny_out" | tr '\n' ' ' | cut -c1-400)"
    return 1
  fi
  unmatched=$(printf '%s\n' "$deny_out" | grep -F 'unmatched-skip' || true)
  if [ -n "$unmatched" ]; then
    printf '%s\n' "$unmatched"
    bad "$name" "bans.skip has entries matching no duplicate (unmatched-skip); every skip must still hit"
    return 1
  fi
  ok "$name"
}

check_lockfile_locked() {
  local name=lockfile-locked
  local out
  if ! out=$(cargo metadata --manifest-path "$ROOT/Cargo.toml" --locked --format-version 1 2>&1 >/dev/null); then
    bad "$name" "Cargo.lock is out of date with the manifests: $(printf '%s' "$out" | tr '\n' ' ' | cut -c1-200)"
    return 1
  fi
  ok "$name"
}

check_fn_length() {
  local name=fn-length
  local out
  if ! out=$(python3 "$ROOT/scripts/tests/fn-length.py" "$ROOT/crates" 2>&1); then
    bad "$name" "$(printf '%s' "$out" | tr '\n' ' ' | cut -c1-400)"
    return 1
  fi
  ok "$name: $(printf '%s\n' "$out" | tail -n 1)"
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
