#!/bin/sh
set -eu
grep -q 'version = "0.27.0"' Cargo.toml || exit 1
grep -q 'export const VERSION = "0.27.0"' agentj-ui/packages/sdk/src/config.ts || exit 1
grep -q 'PROTOCOL_VERSION = "0.17.0"' agentj-ui/packages/sdk/src/config.ts || exit 1
grep -q 'PROTOCOL_VERSION: &str = "0.17.0"' crates/agentj-protocol/src/lib.rs || exit 1
grep -q '"protocol_version": "0.17.0"' protocol/methods.json || exit 1
grep -q '"version": "0.27.0"' agentj-ui/apps/desktop/src-tauri/tauri.conf.json || exit 1
grep -q '"version": "0.27.0"' agentj-ui/apps/desktop/package.json || exit 1
! grep -q '"version": "0.26.0"' agentj-ui/apps/desktop/src-tauri/tauri.conf.json || exit 1
! grep -q '"version": "0.26.0"' agentj-ui/apps/desktop/package.json || exit 1
grep -q 'version = "0.27.0"' agentj-ui/apps/desktop/src-tauri/Cargo.toml || exit 1
echo "version-ok"
