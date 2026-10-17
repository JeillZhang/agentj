#!/bin/sh
set -eu
grep -q 'version = "0.29.9"' Cargo.toml || exit 1
grep -q 'export const VERSION = "0.29.9"' agentj-ui/packages/sdk/src/config.ts || exit 1
grep -q 'PROTOCOL_VERSION = "0.18.0"' agentj-ui/packages/sdk/src/config.ts || exit 1
grep -q 'PROTOCOL_VERSION: &str = "0.18.0"' crates/agentj-protocol/src/lib.rs || exit 1
grep -q '"protocol_version": "0.18.0"' protocol/methods.json || exit 1
grep -q '"version": "0.29.9"' agentj-ui/apps/desktop/src-tauri/tauri.conf.json || exit 1
grep -q '"version": "0.29.9"' agentj-ui/apps/desktop/package.json || exit 1
! grep -q '"version": "0.29.1"' agentj-ui/apps/desktop/src-tauri/tauri.conf.json || exit 1
! grep -q '"version": "0.29.1"' agentj-ui/apps/desktop/package.json || exit 1
grep -q 'version = "0.29.9"' agentj-ui/apps/desktop/src-tauri/Cargo.toml || exit 1
echo "version-ok"
