#!/usr/bin/env bash
set -euo pipefail
SCRIPT="$(dirname "$0")/validate-workflow.sh"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
if "$SCRIPT" "$TMP/nope.yml" lint >/dev/null 2>&1; then
  echo "FAIL: missing file should exit nonzero"; exit 1
fi
cat > "$TMP/valid.yml" <<'EOF'
on:
  push:
    branches: [main]
jobs:
  lint:
    runs-on: ubuntu-latest
EOF
"$SCRIPT" "$TMP/valid.yml" lint || { echo "FAIL: valid file should pass"; exit 1; }
cat > "$TMP/invalid.yml" <<'EOF'
on:
  push:
jobs:
  test:
    runs-on: ubuntu-latest
EOF
if "$SCRIPT" "$TMP/invalid.yml" lint >/dev/null 2>&1; then
  echo "FAIL: missing job should exit nonzero"; exit 1
fi
echo "PASS: validate-workflow tests"
