#!/bin/sh
# Tests for scripts/tests/fn-length.py: positive path plus one negative per defect
# it is meant to catch (empty tree, missed fn shapes, test-module truncation, raw
# strings) and the documented counting rule.
set -eu
ROOT=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
# FN_LENGTH overrides the script under test (used to re-run this suite against
# an older revision and watch it go red).
SCRIPT="${FN_LENGTH:-$ROOT/scripts/tests/fn-length.py}"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
T="$TMP/crates"

body() { i=0; while [ "$i" -lt "$1" ]; do printf '    let _ = %s;\n' "$i"; i=$((i+1)); done; }

run() { set +e; out=$(python3 "$SCRIPT" "$@" 2>&1); rc=$?; set -e; }

expect_hit() {
  name=$1
  printf '%s' "$out" | grep -q "fn $name = " \
    || { echo "probe $name was not reported (rc=$rc): $out"; exit 1; }
  [ "$rc" -eq 1 ] || { echo "probe $name must exit 1 (rc=$rc)"; exit 1; }
  echo "ok: caught $name"
}

expect_clean() {
  [ "$rc" -eq 0 ] || { echo "probe $1 must exit 0 (rc=$rc): $out"; exit 1; }
  printf '%s' "$out" | grep -q '^0 functions > 50 lines$' \
    || { echo "probe $1 must report no offender: $out"; exit 1; }
  echo "ok: clean $1"
}

# --- defect 1: an empty tree must not look green ------------------------------
mkdir -p "$T"
run "$T"
[ "$rc" -eq 2 ] || { echo "empty tree must exit 2 (rc=$rc)"; exit 1; }
printf '%s' "$out" | grep -q 'no .rs files scanned' \
  || { echo "empty tree must say 'no .rs files scanned': $out"; exit 1; }
echo "ok: empty tree exits 2"

run "$TMP/missing-path"
[ "$rc" -eq 2 ] || { echo "missing path must exit 2 (rc=$rc)"; exit 1; }
printf '%s' "$out" | grep -q 'no such path' \
  || { echo "missing path must be named: $out"; exit 1; }
echo "ok: missing path exits 2"

# --- defect 2: every fn shape the old regex could not see ---------------------
mkdir -p "$T/shapes"
{
  printf 'pub extern "C" fn zz_extern() -> u32 {\n'; body 55; printf '}\n'
  printf 'extern "C" fn zz_bare_extern() {\n'; body 55; printf '}\n'
  printf 'const fn zz_const() -> u32 {\n'; body 55; printf '}\n'
  printf 'pub const unsafe fn zz_const_unsafe() {\n'; body 55; printf '}\n'
  printf 'unsafe fn zz_unsafe() {\n'; body 55; printf '}\n'
  printf 'default fn zz_default() {\n'; body 55; printf '}\n'
  printf 'pub(crate) async fn zz_scoped() {\n'; body 55; printf '}\n'
  printf 'pub async fn zz_generic<T: Clone>(value: T) -> T\nwhere\n    T: Send,\n{\n'
  body 55; printf '}\n'
} > "$T/shapes/shapes.rs"
run "$T/shapes/shapes.rs"
for name in zz_extern zz_bare_extern zz_const zz_const_unsafe zz_unsafe zz_default zz_scoped zz_generic; do
  expect_hit "$name"
done

# --- defect 3: production items below an inline test module stay visible ------
{
  printf 'fn zz_before() {}\n\n'
  printf '#[cfg(test)]\nmod tests {\n    use super::*;\n    #[test]\n    fn t() {\n'
  body 60
  printf '    }\n}\n\n'
  printf 'fn zz_after_tests() {\n'; body 55; printf '}\n'
} > "$T/shapes/after_tests.rs"
run "$T/shapes/after_tests.rs"
expect_hit zz_after_tests
want=$(grep -n 'fn zz_after_tests' "$T/shapes/after_tests.rs" | cut -d: -f1)
printf '%s' "$out" | grep -q "after_tests.rs:$want: fn zz_after_tests" \
  || { echo "after_tests.rs: expected line $want, got: $out"; exit 1; }
echo "ok: reported line survives the test-module cut"

# --- defect 4: a raw string must not desync quote pairing --------------------
{
  printf 'const PATTERN: &str = r#"a " brace { dangle"#;\n'
  printf 'const SECOND: &str = r##"one "# two"##;\n'
  printf 'fn zz_after_raw() {\n'; body 55; printf '}\n'
} > "$T/shapes/raw.rs"
run "$T/shapes/raw.rs"
expect_hit zz_after_raw

# --- defect 5: the counting rule is pinned, not left implicit ----------------
# The measure is closing-brace line minus `fn` line. A signature over 3 lines plus a
# 50-line body therefore reports 53 (brace-to-brace would report 50 and pass), and a
# one-line signature passes at 49 body lines but fails at 50.
{
  printf 'fn zz_rule_probe(\n    a: u32,\n) -> u32 {\n'; body 50; printf '}\n'
  printf 'fn zz_boundary() {\n'; body 49; printf '}\n'
  printf 'fn zz_over() {\n'; body 50; printf '}\n'
} > "$T/shapes/rule.rs"
run "$T/shapes/rule.rs"
expect_hit zz_rule_probe
printf '%s' "$out" | grep -q 'fn zz_rule_probe = 53 lines' \
  || { echo "counting rule drifted from `fn` keyword to closing brace: $out"; exit 1; }
printf '%s' "$out" | grep -q 'fn zz_boundary' \
  && { echo "the last passing size must not be reported: $out"; exit 1; }
expect_hit zz_over

# --- defect 6: the test-path exemption is reported, never silent -------------
mkdir -p "$T/t/lib/tests"
{ printf 'fn zz_production() {}\n'; } > "$T/t/lib/mod.rs"
{ printf 'fn zz_tests_case() {\n'; body 55; printf '}\n'; } > "$T/t/lib/tests/case.rs"
run "$T/t"
expect_clean "tests/ files skipped"
printf '%s' "$out" | grep -q 'skipped 1 test file(s)' \
  || { echo "the test-path exemption must be reported: $out"; exit 1; }
echo "ok: test files counted"

# --- the repository itself must stay clean -----------------------------------
run "$ROOT/crates"
[ "$rc" -eq 0 ] || { echo "repository has long functions: $out"; exit 1; }
echo "fn-length-tests-ok"
