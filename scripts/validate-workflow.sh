#!/usr/bin/env bash
# Usage: scripts/validate-workflow.sh <workflow-file> <job1> [job2...]
# Validates: file exists, has 'on:', has 'jobs:', contains required jobs.
set -euo pipefail
FILE="${1:?usage: validate-workflow.sh <workflow-file> <jobs...>}"
shift
[ -f "$FILE" ] || { echo "FAIL: $FILE missing"; exit 1; }
grep -q '^on:' "$FILE" || { echo "FAIL: $FILE missing 'on:'"; exit 1; }
grep -q '^jobs:' "$FILE" || { echo "FAIL: $FILE missing 'jobs:'"; exit 1; }
for job in "$@"; do
  grep -Eq "^  $job:" "$FILE" || { echo "FAIL: $FILE missing job '$job'"; exit 1; }
done
if python3 -c "import yaml" 2>/dev/null; then
  FILE="$FILE" python3 -c "import os, yaml, sys; yaml.safe_load(open(os.environ['FILE']))" \
    || { echo "FAIL: $FILE invalid YAML"; exit 1; }
fi
echo "PASS: $FILE"
