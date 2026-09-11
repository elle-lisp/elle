# tests/elle

<!-- audited: 2026-09-11 -->

The Elle corpus: one self-contained `.lisp` program per subject, each asserting
what the language does and exiting non-zero when an assertion fails.

## What a file here is

A file is a whole program. It asserts with the built-in `(assert expr msg)`,
which signals `:failed-assertion` and exits 1, so a file that runs to the end
has passed. It carries no harness, no setup, and no dependency on another file.

Name the file for its subject, and keep one subject per file. The directory is
its own index: `ls tests/elle` names every subject the corpus covers.

Does NOT belong here:

- A test that inspects Rust types or a Rust API.
- Source that must fail to compile.
- An error-message substring match.
- A property over generated inputs.

[docs/analysis/testing.md](../../docs/analysis/testing.md) is the decision tree
that says which kind of test a thing wants.

## Shape

```lisp
(elle/epoch 12)
# What this file pins, and the trap or counter-factual behind it.

(assert (= (+ 1 2) 3) "addition")
(assert (not (< 5 3)) "not less than")
```

Open every file with `(elle/epoch N)` for the current epoch — `CURRENT_EPOCH`
in [src/epoch/rules.rs](../../src/epoch/rules.rs).

## Running

The agent-first runner owns this directory. `elle test`, which
`make smoke-elle` drives, compiles and runs every file once per JIT policy
(`:off` → `vm`, `:eager` → `jit`), plus per-tier divergence for a single-form
file. A new file is picked up by being here; there is nothing to register.

[docs/testing.md](../../docs/testing.md) covers the runner and the commands,
and [docs/test-runner.md](../../docs/test-runner.md) is its specification.

One file at a time, `elle tests/elle/NAME.lisp` runs it as a plain program.

[tests/integration/elle_scripts.rs](../integration/elle_scripts.rs) pins the
few files that need a process-global mode the runner cannot vary per file — the
page-guard oracle, the I/O backend, a backend toggle paired with the adaptive
JIT. A file that needs no such mode does not go there, because the runner
already runs it under more policies than one subprocess call would.

## Invariants

1. **A file is self-contained.** It asserts directly and runs on its own.
2. **Exit 0 is pass, 1 is fail.** Every runner reads the exit code.
3. **A file is deterministic.** No clock, no randomness, no dependence on how
   fast a background thread happens to be. Where a result depends on work that
   is in flight, drain it first — `(jit/rejections)` drains pending JIT
   compilations, and
   [jit-compiled-caller-promotes-callee.lisp](jit-compiled-caller-promotes-callee.lisp)
   shows the shape.
4. **A file tests what the language does**, not how the implementation does it.
   An exception a file must earn in its own comment: a tier probe such as
   `(jit? f)`, where the behavior under test IS which tier ran the code.
