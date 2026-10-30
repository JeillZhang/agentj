#!/bin/sh
# Tests for scripts/check-version.sh: positive path + one negative per assertion family.
set -eu
ROOT=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

mk() { mkdir -p "$TMP/$(dirname "$1")"; printf '%b\n' "$2" > "$TMP/$1"; }

fixture() {
  rm -rf "$TMP"/*
  mk Cargo.toml '[workspace.package]\nversion = "0.1.0"\n'
  mk agentj-ui/packages/sdk/src/config.ts \
    'export const VERSION = "0.1.0";\nexport const PROTOCOL_VERSION = "0.21.0";\n'
  mk crates/agentj-protocol/src/lib.rs 'pub const PROTOCOL_VERSION: &str = "0.21.0";\n'
  mk protocol/methods.json '{"protocol_version": "0.21.0"}'
  mk agentj-ui/package.json '{"version": "0.1.0"}'
  mk agentj-ui/apps/web/package.json '{"version": "0.1.0"}'
  mk agentj-ui/apps/tui/package.json '{"version": "0.1.0"}'
  mk agentj-ui/package-lock.json '{"packages": {"": {"name": "agentj-ui", "version": "0.1.0"}}}'
  mk agentj-ui/apps/desktop/package.json '{"version": "0.1.0"}'
  mk agentj-ui/apps/desktop/src-tauri/tauri.conf.json '{"version": "0.1.0"}'
  mk agentj-ui/apps/desktop/src-tauri/Cargo.toml '[package]\nversion = "0.1.0"\n'
}

fixture
AGENTJ_ROOT="$TMP" "$ROOT/scripts/check-version.sh" | grep -q version-ok \
  || { echo "positive fixture must print version-ok"; exit 1; }

mk agentj-ui/apps/web/package.json '{"version": "9.9.9"}'
out=$(AGENTJ_ROOT="$TMP" "$ROOT/scripts/check-version.sh" 2>&1 || true)
printf '%s' "$out" | grep -q 'apps/web/package.json' \
  || { echo "a wrong apps/web version must be named"; exit 1; }

fixture
mk agentj-ui/package-lock.json '{"packages": {"": {}}}'
out=$(AGENTJ_ROOT="$TMP" "$ROOT/scripts/check-version.sh" 2>&1 || true)
printf '%s' "$out" | grep -q 'package-lock.json' \
  || { echo "a lockfile without a root version must be named"; exit 1; }

# The `version` key stays 0.1.0 so the positive assertion passes; the leak sits elsewhere.
fixture
mk agentj-ui/apps/desktop/src-tauri/tauri.conf.json \
  '{"version": "0.1.0", "bundle": {"shortVersion": "0.29.12"}}'
out=$(AGENTJ_ROOT="$TMP" "$ROOT/scripts/check-version.sh" 2>&1 || true)
printf '%s' "$out" | grep -q 'tauri.conf.json' \
  || { echo "the 0.2x guard must name tauri.conf.json"; exit 1; }
printf '%s' "$out" | grep -q '0\.29' \
  || { echo "the 0.2x guard must name the offending literal"; exit 1; }

"$ROOT/scripts/check-version.sh" | grep -q version-ok \
  || { echo "the real repository must stay consistent"; exit 1; }

echo "check-version-tests-ok"
