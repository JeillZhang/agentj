#!/bin/sh
# Single version baseline: workspace 0.1.0 + protocol 0.20.0 must agree across
# Rust, the SDK, the protocol registry and every UI package. `AGENTJ_ROOT`
# overrides the repository root (used by scripts/test-check-version.sh).
set -eu
ROOT="${AGENTJ_ROOT:-.}"

fail() { printf 'version mismatch: %s (expected %s)\n' "$1" "$2" >&2; exit 1; }

# <relative path> <grep pattern> <description of what was expected>
require_line() {
  [ -f "$ROOT/$1" ] || fail "$1" "existing file"
  grep -q -- "$2" "$ROOT/$1" || fail "$1" "$3"
}

require_line Cargo.toml 'version = "0.1.0"' 'version = "0.1.0"'
require_line agentj-ui/packages/sdk/src/config.ts 'export const VERSION = "0.1.0"' \
  'VERSION = "0.1.0"'
require_line agentj-ui/packages/sdk/src/config.ts 'PROTOCOL_VERSION = "0.20.0"' \
  'PROTOCOL_VERSION = "0.20.0"'
require_line crates/agentj-protocol/src/lib.rs 'PROTOCOL_VERSION: &str = "0.20.0"' \
  'PROTOCOL_VERSION = "0.20.0"'
require_line protocol/methods.json '"protocol_version": "0.20.0"' '"protocol_version": "0.20.0"'
require_line agentj-ui/package.json '"version": "0.1.0"' '"version": "0.1.0"'
require_line agentj-ui/apps/web/package.json '"version": "0.1.0"' '"version": "0.1.0"'
require_line agentj-ui/apps/tui/package.json '"version": "0.1.0"' '"version": "0.1.0"'
require_line agentj-ui/apps/desktop/package.json '"version": "0.1.0"' '"version": "0.1.0"'
require_line agentj-ui/apps/desktop/src-tauri/tauri.conf.json '"version": "0.1.0"' \
  '"version": "0.1.0"'
require_line agentj-ui/apps/desktop/src-tauri/Cargo.toml 'version = "0.1.0"' 'version = "0.1.0"'

LOCKFILE=agentj-ui/package-lock.json
[ -f "$ROOT/$LOCKFILE" ] || fail "$LOCKFILE" 'existing file'
if ! jq -e '.packages[""].name == "agentj-ui" and .packages[""].version == "0.1.0"' \
  "$ROOT/$LOCKFILE" >/dev/null 2>&1; then
  fail "$LOCKFILE" 'root entry name "agentj-ui" version "0.1.0"'
fi

# The desktop crate used to ship an independent 0.29.x line. Any leftover 0.2x
# numeric version literal in a version-bearing file is a regression.
for file in agentj-ui/apps/desktop/package.json \
  agentj-ui/apps/desktop/src-tauri/tauri.conf.json \
  agentj-ui/apps/desktop/src-tauri/Cargo.toml; do
  [ -f "$ROOT/$file" ] || fail "$file" 'existing file'
  hit=$(grep -Eo '0\.2[0-9]\.[0-9]+' "$ROOT/$file" | head -1 || true)
  [ -z "$hit" ] || fail "$file" "no stale 0.2x version literal (found $hit)"
done

echo "version-ok"
