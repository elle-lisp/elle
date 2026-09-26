# The language suite

<!-- audited: 2026-09-26 -->

One self-contained Elle program per subject, each asserting what the language
promises and exiting non-zero when an assertion fails.

A file here is a language test ([spec](../../docs/spec.md) § Two suites). Its
claim holds for every correct implementation of Elle, whatever its tiers,
footprint or optimizations. Every build that CI makes runs every file here and
must pass it.

## What a file here is

A file is a whole program. It asserts with `(assert expr msg)`, which signals
`:failed-assertion` and exits 1, so a file that runs to the end has passed. It
carries no harness, no setup, and no dependency on another file.

Name the file for its subject, and keep one subject per file. The directory is
its own index: `ls tests/lang` names every subject the language suite covers.

A file asserts values, errors, signals, fiber states, and whether a program
compiles. It does not call an implementation extension
([spec](../../docs/spec.md) § Three categories of surface):

- no gauge — `arena/*`, `fn/bytecode-size`, `arena/page-claims`;
- no tier — `jit?`, `jit/rejections`, `compile/run-on`, `vm/tier`,
  `(vm/config :jit)`, `backend?`;
- no artifact — `fn/disasm`, `fn/cfg`, `compile/dumps`;
- no mode — a file here has no sidecar, and the suite passes it no flag.

A test that needs any of these is an implementation test, and it belongs in
[tests/impl](../impl/overview.md).
[docs/analysis/testing.md](../../docs/analysis/testing.md) is the decision
tree.

## Shape

```lisp
(elle/epoch 13)
# What this file pins, and the trap or counter-factual behind it.

(assert (= (+ 1 2) 3) "addition")
(assert (not (< 5 3)) "not less than")
```

Open every file with `(elle/epoch N)` for the current epoch — `CURRENT_EPOCH`
in [src/epoch/rules.rs](../../src/epoch/rules.rs).

## Running

`make smoke-lang` runs every file as its own process, `elle FILE`, through
`elle test --isolate ''`, and records each verdict in the session store
([docs/testing.md](../../docs/testing.md)). A new file is picked up by being
here; there is nothing to register.

One file at a time, `elle tests/lang/NAME.lisp` runs it as a plain program.

## Invariants

1. **A file is self-contained.** It asserts directly and runs on its own.
2. **Exit 0 is pass, 1 is fail.** Every runner reads the exit code.
3. **A file is deterministic.** No clock, no randomness, no dependence on how
   fast a background thread happens to be.
4. **A file tests what the language does**, never how this implementation does
   it. The same file must pass on every build.
