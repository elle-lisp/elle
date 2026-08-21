#!/usr/bin/env python3
"""Mechanically add the NativeCtx parameter to every fn the compiler says
is used where a PrimFn is expected (E0308 'found fn item ... {name}')."""
import json, re, subprocess, sys, pathlib

ROOT = pathlib.Path("/home/adavidoff/git/tmp/s14")

def collect_names():
    out = subprocess.run(
        ["cargo", "check", "--all-features", "--message-format=json"],
        cwd=ROOT, capture_output=True, text=True)
    names = set()
    for line in out.stdout.splitlines():
        try:
            msg = json.loads(line)
        except json.JSONDecodeError:
            continue
        if msg.get("reason") != "compiler-message":
            continue
        m = msg["message"]
        if m.get("code", {}) and m["code"].get("code") == "E0308":
            text = m.get("rendered") or ""
            for mm in re.finditer(r"found fn item `for<'a> fn\(&'a \[repr::Value\]\) -> \(signalbits::SignalBits, repr::Value\) \{([a-zA-Z0-9_:]+)\}", text):
                names.add(mm.group(1).split("::")[-1])
    return names

SIG_RE_TMPL = (
    r"(fn\s+{name}\s*\(\s*)"                         # 'fn name('
    r"((?:_?[a-zA-Z0-9_]+)\s*:\s*&\[\s*(?:crate::value::)?Value\s*\]\s*,?\s*\))"  # 'args: &[Value])'
)

CTX_PARAM = "_ctx: &mut crate::primitives::ctx::NativeCtx<'_>, "

def rewrite_file(path: pathlib.Path, names):
    src = path.read_text()
    orig = src
    hit = []
    for name in names:
        pat = re.compile(SIG_RE_TMPL.format(name=re.escape(name)), re.S)
        def repl(m):
            hit.append(name)
            return m.group(1) + CTX_PARAM + m.group(2)
        src, n = pat.subn(repl, src, count=1)
    if src != orig:
        path.write_text(src)
    return hit

def main():
    names = collect_names()
    print(f"{len(names)} fn names to flip", file=sys.stderr)
    if not names:
        return
    # find definition files
    remaining = set(names)
    flipped = []
    for path in sorted(ROOT.glob("src/**/*.rs")):
        if not remaining:
            break
        src = path.read_text()
        present = {n for n in remaining if re.search(r"\bfn\s+" + re.escape(n) + r"\s*\(", src)}
        if not present:
            continue
        hit = rewrite_file(path, present)
        for h in hit:
            flipped.append((str(path.relative_to(ROOT)), h))
            remaining.discard(h)
    print(f"flipped {len(flipped)}; unresolved: {sorted(remaining)}", file=sys.stderr)
    for f, n in flipped[:10]:
        print(f"  {f}: {n}", file=sys.stderr)

if __name__ == "__main__":
    main()
