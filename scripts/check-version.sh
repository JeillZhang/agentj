#!/bin/sh
set -eu
grep -q 'version = "0.14.1"' Cargo.toml || exit 1
grep -q 'PROTOCOL_VERSION = "0.12.1"' agentj-ui/packages/sdk/src/config.ts || exit 1
grep -q '"version": "0.14.1"' agentj-ui/apps/desktop/src-tauri/tauri.conf.json || exit 1
grep -q '"version": "0.14.1"' agentj-ui/apps/desktop/package.json || exit 1
! grep -q '"version": "0.10.0"' agentj-ui/apps/desktop/src-tauri/tauri.conf.json || exit 1
! grep -q '"version": "0.10.0"' agentj-ui/apps/desktop/package.json || exit 1
grep -q 'version = "0.14.1"' agentj-ui/apps/desktop/src-tauri/Cargo.toml || exit 1
echo "version-ok"
