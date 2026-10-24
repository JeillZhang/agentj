#!/usr/bin/env python3
"""Report Rust functions longer than 50 lines (spec §3.2 counting rules).

What is counted: the line breaks between the `fn` keyword and the closing brace of
the body, i.e. **closing-brace line − `fn` line**. A signature wrapped over several
lines is therefore charged to the function: `crates/agentj-cli/src/main.rs`
`serve_tcp` spans lines 511–561 and reports 50, where a brace-to-brace measurement
would report 43. Declarations without a body (`fn f();` in a trait) are not
counted, and neither are attribute lines above the signature.

Recognised item shapes: `pub`, `pub(crate)`, `pub(in path)`, `default`, `const`,
`async`, `unsafe` and `extern "C"` in any legal order, plus generic and indented
impl methods.

Test-only code is exempt and the exemption is reported, never silent:
inline `#[cfg(test)] mod … { … }` blocks are cut brace-balanced (items below them
are still scanned) and files with a `/tests/` path component
(`crates/*/tests/*.rs`, `src/tests/*.rs`) are skipped.

Directories are expanded to every `*.rs` file beneath them; `target/` is ignored.
An invocation that leaves nothing to scan (`no .rs files scanned`) or names a
missing path exits 2 — a green run over an empty tree would be a lie.
"""
import pathlib
import re
import sys

MAX, ROOT = 50, pathlib.Path(__file__).resolve().parents[2]

RAW = re.compile(r'(?:b|br)?r(#*)"')
CHAR = re.compile(r"'(?:\\.|[^\\'])'")
TEST_MOD = re.compile(
    r'#\[cfg\(test\)\]\s*(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?mod\s+\w+\s*\{'
)
FN = re.compile(
    r'(?m)^[ \t]*(?:pub(?:\([^)]*\))? )?(?:default )?(?:const )?(?:async )?'
    r'(?:unsafe )?(?:extern "[^"]*" )?fn\s+(\w+)'
)


def newlines(text):
    return '\n' * text.count('\n')


def literal_end(src, i):
    """Return (end, placeholder) if a literal starts at `i`, else None.

    Literals collapse to an empty placeholder of the same line height: braces and
    quotes inside them cannot desync the scan, while line numbers stay exact and
    an `extern "C" fn` keeps the shape the item regex looks for.
    """
    raw = RAW.match(src, i)
    if raw:
        close = '"' + raw.group(1)
        j = src.find(close, raw.end())
        end = len(src) if j < 0 else j + len(close)
        return end, '""' + newlines(src[i:end])
    if src[i] == '"':
        j = i + 1
        while j < len(src) and src[j] != '"':
            j += 2 if src[j] == '\\' else 1
        end = min(j + 1, len(src))
        return end, '""' + newlines(src[i:end])
    char = CHAR.match(src, i)                # 'x' / '\n'; lifetimes stay put
    if char:
        return char.end(), "''"
    return None


def strip(src):
    """Collapse comments and literals so braces and item shapes count reliably."""
    out, i, n = [], 0, len(src)
    while i < n:
        if src.startswith('//', i):
            j = src.find('\n', i)
            i = n if j < 0 else j
            continue
        if src.startswith('/*', i):
            j = src.find('*/', i + 2)
            end = n if j < 0 else j + 2
            out.append(newlines(src[i:end]))
            i = end
            continue
        literal = literal_end(src, i)
        if literal:
            i, placeholder = literal
            out.append(placeholder)
            continue
        out.append(src[i])
        i += 1
    return ''.join(out)


def cut_test_modules(code):
    """Blank out inline `#[cfg(test)] mod` blocks, keeping line numbers stable."""
    out, pos = [], 0
    for m in TEST_MOD.finditer(code):
        if m.start() < pos:                  # nested inside a block already cut
            continue
        out.append(code[pos:m.start()])
        depth, i = 0, m.end() - 1
        while i < len(code):
            if code[i] == '{':
                depth += 1
            elif code[i] == '}':
                depth -= 1
                if depth == 0:
                    i += 1
                    break
            i += 1
        out.append('\n' * code.count('\n', m.start(), i))
        pos = i
    out.append(code[pos:])
    return ''.join(out)


def body_length(code, start):
    """Return (lines, closed) for the body opening after `start`."""
    i, depth, seen = start, 0, False
    while i < len(code):
        ch = code[i]
        if ch == '{':
            seen = True
            depth += 1
        elif ch == '}':
            depth -= 1
            if seen and depth == 0:
                return code[start:i].count('\n'), True
        elif ch == ';' and not seen:
            return 0, False                  # trait/method declaration, no body
        i += 1
    return 0, False


def offenders(path):
    code = cut_test_modules(strip(path.read_text(encoding='utf-8', errors='replace')))
    for m in FN.finditer(code):
        body, closed = body_length(code, m.end())
        if not closed or body <= MAX:
            continue
        try:
            rel = path.relative_to(ROOT)
        except ValueError:
            rel = path
        keyword = m.end() - len(m.group(1))
        yield rel, code.count('\n', 0, keyword) + 1, m.group(1), body


def is_test_only(path):
    return 'tests' in path.parts


def expand(arg):
    path = pathlib.Path(arg).resolve()
    return sorted(path.rglob('*.rs')) if path.is_dir() else [path]


def main(argv):
    args = argv or [str(ROOT / 'crates')]
    missing = [a for a in args if not pathlib.Path(a).exists()]
    if missing:
        print(f'fn-length: no such path: {", ".join(missing)}')
        return 2
    targets = [t for arg in args for t in expand(arg) if 'target/' not in t.as_posix()]
    if not targets:
        print('fn-length: no .rs files scanned — refusing to report a vacuous pass')
        return 2
    scanned = [t for t in targets if not is_test_only(t)]
    hits = [h for t in scanned for h in offenders(t)]
    for f, line, name, body in hits:
        print(f'{f}:{line}: fn {name} = {body} lines')
    print(f'scanned {len(scanned)} file(s), skipped {len(targets) - len(scanned)} test file(s)')
    print(f'{len(hits)} functions > {MAX} lines')
    return 1 if hits else 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
