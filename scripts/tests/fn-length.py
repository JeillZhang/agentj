#!/usr/bin/env python3
"""Report Rust functions whose body exceeds 50 lines (spec §3.2 counting rules).

Test-only code is exempt: inline `#[cfg(test)] mod` bodies are cut before scanning,
and modules living in a separate `/tests/` directory (`crates/*/tests/*.rs`,
`src/tests/*.rs`) are skipped by path. Directories may be passed as arguments and are
expanded to every `*.rs` file beneath them.
"""
import re
import sys
import pathlib

MAX, ROOT = 50, pathlib.Path(__file__).resolve().parents[2]


def strip(src):
    out, i, n = [], 0, len(src)
    while i < n:
        c = src[i]
        if src.startswith('//', i):
            j = src.find('\n', i)
            i = n if j < 0 else j
        elif src.startswith('/*', i):
            j = src.find('*/', i + 2)
            i = n if j < 0 else j + 2
        elif c == '"':
            i += 1
            while i < n and src[i] != '"':
                i += 2 if src[i] == '\\' else 1
            i += 1
        elif c == "'" and re.match(r"'(?:\\.|[^\\'])'", src[i:i + 4]):
            i += re.match(r"'(?:\\.|[^\\'])'", src[i:i + 4]).end()
        else:
            out.append(c)
            i += 1
    return ''.join(out)


def cut_tests(code):
    return re.split(r'#\[cfg\(test\)\]\s*(?:pub\s+)?mod\s+\w+', code)[0]


def offenders(path):
    code = cut_tests(strip(path.read_text(encoding='utf-8', errors='replace')))
    for m in re.finditer(r'(?m)^\s*(?:pub(?:\([^)]*\))? )?(?:async )?(?:unsafe )?fn\s+(\w+)', code):
        i, depth, seen = m.end(), 0, False
        while i < len(code):
            ch = code[i]
            if ch == '{':
                seen = True
                depth += 1
            elif ch == '}':
                depth -= 1
                if seen and depth == 0:
                    break
            elif ch == ';' and not seen:
                break                      # trait/method declaration, no body
            i += 1
        if not seen:
            continue
        body = code[m.end():i].count('\n')
        if body > MAX:
            try:
                rel = path.relative_to(ROOT)
            except ValueError:
                rel = path
            yield rel, code.count('\n', 0, m.start()) + 1, m.group(1), body


def is_test_only(path):
    return 'tests' in path.parts


def expand(arg):
    path = pathlib.Path(arg).resolve()
    return sorted(path.rglob('*.rs')) if path.is_dir() else [path]


targets = [t for arg in sys.argv[1:] for t in expand(arg)]
if not targets:
    targets = sorted((ROOT / 'crates').glob('**/src/**/*.rs'))
hits = [
    h
    for t in targets
    if 'target/' not in str(t) and not is_test_only(t)
    for h in offenders(t)
]
for f, line, name, body in hits:
    print(f'{f}:{line}: fn {name} = {body} lines')
print(f'{len(hits)} functions > {MAX} lines')
sys.exit(1 if hits else 0)
