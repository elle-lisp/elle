# The implementation suite

<!-- audited: 2026-09-26 -->

Elle programs that check this implementation: its gauges, its tiers, its crashes
and its mechanisms, each run on the rig.

A file here is an implementation test ([spec](../../docs/spec.md) § Two
suites). Its claim holds for this implementation and may be false of another
correct one: a region count, a page claim, whether the JIT compiled a
function, whether a release ran twice. The Rust suite under
[tests](../AGENTS.md) holds the rest of the implementation suite, and a test
that has to build its input beneath the compiler — a call written as LIR or
bytecode — belongs there.

## What a file here is

A file is a whole program that asserts with `(assert expr msg)`, the same as a
language test. What makes it an implementation test is what it reads or needs:

- a gauge — `arena/count`, `arena/region-count`, `arena/page-claims`,
  `fn/bytecode-size`;
- a tier — `jit?`, `jit/rejections`, `compile/run-on`, `vm/tier`,
  `(vm/config :jit)`;
- an artifact — `fn/disasm`, `fn/cfg`, `compile/dumps`;
- a mode — the JIT off or eager, `--trace=guardfree`, `--trace=syncjit`;
- a mechanism as its subject — a calling convention, a tail move, an owned
  parameter, a region release — even where its assertions read values.

[docs/analysis/testing.md](../../docs/analysis/testing.md) is the decision
tree, and [tests/lang](../lang/overview.md) holds the other suite.

## The sidecar

A file that needs a mode names it in a TOML file beside it with the same stem:
`region-tail-move-toplevel-uaf.toml` beside
`region-tail-move-toplevel-uaf.lisp`. The rig reads it before it compiles the
file ([rig](../../rig/overview.md) owns the keys). A file with no sidecar runs
under the defaults of the build.

```toml
# A released page is unmapped, so a stale read faults at its deref.
trace = ["guardfree"]
```

The sidecar says what the file needs; the file's header comment says why.

## Directories

The suite runs the files at the top of this directory. The directories under it
hold what those files read, and the suite runs none of them:

- `lib/` holds the leak estimator the two dashboards import.
- `probe/` holds the oracle's rows, one module per shape family, which
  [oracle.lisp](oracle.lisp) includes.
- `tailexit/` holds the tail-exit ledgers, which
  [region-tail-frame-exit-uaf.lisp](region-tail-frame-exit-uaf.lisp)
  includes.
- `profiles/` holds the rig profiles a pass applies to both suites.

## The dashboards

[oracle.lisp](oracle.lisp) measures the leak rate of each residual class, and
[plumb.lisp](plumb.lisp) measures the I/O leak rates. Each loops a shape
under a heap gauge and reports a verdict per class through the measurement
channel ([test-store](../../docs/test-store.md) § Measurements).
[docs/impl/region/diagnostics.md](../../docs/impl/region/diagnostics.md) owns
their instruments.

## Running

`make smoke-impl` runs every file here as its own child of `elle test --host
elle-rig`, so each verdict lands in the session store. It then runs the
suites once more under each profile the pass names. A new file is picked up
by being here; there is nothing to register.

One file at a time, `elle-rig tests/impl/NAME.lisp` runs it with its sidecar.
