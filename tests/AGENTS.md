# Tests

<!-- audited: 2026-10-05 -->

Where each kind of test lives, the helpers they share, and how to add one.

Which suite a test belongs in is [docs/spec.md](../docs/spec.md), and which
kind of test to write is the decision tree in
[docs/analysis/testing.md](../docs/analysis/testing.md). How the Elle suites
and their runner work is [docs/testing.md](../docs/testing.md).

## Directory structure

```
tests/
├── lib.rs              # The shared binary: includes unittests/, integration/, property/
├── common/mod.rs       # Shared helpers (eval_source, setup, the Makefile and workflow readers)
├── fixtures/           # Static files the Rust tests read
├── lang/               # The language suite: what every implementation must do
├── impl/               # The implementation suite's Elle half, run on the rig
├── runner/             # The `elle test` runner's own acceptance tests and fixtures
├── ledger/             # The ratchet's bounds: one data file per producer (docs/ratchet.md)
├── ratchet/            # Producers that drive a tool outside Elle and read its counts
├── golden/             # The snapshots impl/escape-golden.lisp compares against
├── modules/            # Modules the integration tests import
├── property/           # Property-based tests (proptest)
├── integration/        # Full-pipeline and repository tests
├── unittests/          # Rust APIs tested directly
├── io_copies/          # The measuring helper `io_copies.rs` uses
├── image_boot/         # The modules of image_boot.rs; so too region_process_teardown/, wasm_smoke/
├── README.md           # The suites and the command that runs each
└── *.rs                # One standalone binary each — see below
```

Everything under `lib.rs` shares one binary and one process. A file directly
under `tests/` gets a binary of its own, which is what a test needs when its
subject is process-global: a counter, an rlimit, a signal disposition, a
re-exec, or a fault that would take every other test down with it
(`worker_heap.rs`, `region_process_teardown.rs`, `wasm_smoke.rs`, …). See
[docs/analysis/testing.md](../docs/analysis/testing.md) § "Process-global state
needs its own binary" for when to reach for one.

Outside `tests/`, many `src/` files carry an inline `#[cfg(test)]` module
beside the code it tests.

## The categories

### The Elle suites (`tests/lang/`, `tests/impl/`)

Tests that evaluate Elle source and check values, errors or signals, written
with the `(assert COND "message")` idiom. They include a program that must not
compile, checked through `compile/whole-module` under `protect`. Each `.lisp`
file is a self-contained program that exits non-zero on failure.

A file in [tests/lang](lang/overview.md) answers: "Does every correct Elle
produce this?" Every build runs it with no flag (`make smoke-lang`). A file in
[tests/impl](impl/overview.md) answers: "Does this implementation produce this,
at this cost, under this mode?" It runs on the rig with the mode its sidecar
names (`make smoke-impl`). The agent-first runner (`elle test`) records both.

Leave these to Rust: a test that needs a Rust type, a test that must build its
input beneath the compiler as LIR or bytecode, and a property test.

### Property tests (`tests/property/`)

Invariants that must hold across all generated inputs: roundtrip fidelity,
mathematical laws, type discrimination, determinism, signal inference
soundness, and a defect's regression across an input range.
[property/AGENTS.md](property/AGENTS.md) covers the directory.

### Integration tests (`tests/integration/`)

End-to-end behavior through the whole pipeline (Reader → Expander → Analyzer →
Lowerer → Emitter → VM), plus the tests that check the repository itself: the
documents, the CI workflow, and the suites' runner.
[integration/AGENTS.md](integration/AGENTS.md) names the groups.

### Unit tests (`tests/unittests/`)

Rust APIs called directly, without the full pipeline: `Value` construction and
equality, symbols, primitives through `call_primitive()`, closures, and the
debug renderings of bytecode and HIR.

### Inline tests (`#[cfg(test)]` in `src/`)

Implementation details that need private items. They live in the file they
test and need no registration.

## Test helpers

`common/mod.rs` opens with the rule every eval helper follows, and each helper
carries its own docstring. The ones a new test reaches for first:

- **`eval_source(input, |result| …)`** — the canonical eval: primitives,
  stdlib, and the scheduler, in a fresh `Runtime`. The result is handed to the
  closure while its heap is alive. Return only owned data from the closure,
  never the `Value`.
- **`eval_source_bare`** — the same without stdlib. Prelude macros still work.
- **`eval_reuse` / `eval_reuse_bare`** — a cached runtime per thread, with
  globals restored between calls. Use these in property tests.
- **`setup()`** — a `Runtime` with primitives and stdlib. Take the disjoint
  borrows with `rt.parts()`.
- **`ScratchDir::new(tag)`** — a unique directory under the platform temp
  root, removed when it drops.
- **`make_var`, `make_dry_run`, `passes`, `workflow_jobs`** — what the
  Makefile, a suite target and the workflows say, for the tests that check them
  (`common/repo.rs`, `common/passes.rs`, `common/workflows.rs`).

```rust
use crate::common::eval_source;

#[test]
fn addition_answers_its_sum() {
    eval_source("(+ 1 2)", |r| assert_eq!(r.unwrap(), elle::Value::int(3)));
}
```

`property/strategies.rs` holds the shared proptest strategies
(`arb_immediate`, `arb_value`, and the FFI type and value strategies). A
property file adds local strategies for its own domain.

## Scratch files

Tests that need the filesystem must derive their paths from the platform temp
root and must delete everything they create — including on the failure path
where practical. Never hardcode `/tmp` (shared, size-limited, and not where
`TMPDIR` points); never reuse a fixed filename (concurrent runs collide).
`integration::scratch::no_hardcoded_tmp_paths` enforces the no-`/tmp` half of
this policy over every `.rs` and `.lisp` file in the tree.

- **Elle scripts**: wrap the test in `(with-temp-dir dir ...)` — it binds a
  fresh directory from `file/mktempdir` and removes the whole tree afterwards,
  even when the body errors. Build paths with `(path/join dir "name")`.
- **Rust tests**: use `ScratchDir`, or build paths from
  `std::env::temp_dir()` made unique with `std::process::id()`, and remove them
  before the test returns.

## Adding a test

A file under `unittests/`, `integration/` or `property/` is not compiled until
that directory's `mod.rs` names it, and an unregistered file reports success
having run nothing:

```rust
mod myfeature {
    include!("myfeature.rs");
}
```

The `include!()` shape is what lets `lib.rs` pull each directory in as one
crate. Name a file in lowercase words joined by underscores, and name a test
function for the claim its body proves.

A property test sets its case count with
`#![proptest_config(crate::common::proptest_cases(N))]`. `PROPTEST_CASES`
overrides every default, which is how CI and a quick local run pick their own
count. Pick `N` by the cost of one case:

| Cost per case | Cases | Example |
|---------------|-------|---------|
| Cheap (no eval, pure Rust) | 1000 | Value encoding roundtrips, signal combine laws |
| Medium (single eval) | 200 | Arithmetic properties, reader roundtrips |
| Expensive (multiple evals or recursion) | 10-50 | Bug regression, determinism, complex programs |

## Running tests

| Command | Runtime | What it does |
|---------|---------|-------------|
| `cargo test -p elle --lib` | ~1.5 min | The inline unit tests |
| `cargo test --test lib integration::NAME` | seconds to minutes | One integration file |
| `cargo test --test '*'` | ~10 min | Every integration test and standalone binary |
| `make smoke` | ~30 min, release | Both Elle suites, the doctests, the embedding demo, and the surface gate |
| `make test` | smoke + ~5 min | What the PR gate runs, locally |
| `cargo test --workspace` | ~30 min | Everything — ask before running it |

Pass the release binaries to anything that runs the Elle suites: `make smoke
ELLE=./target/release/elle ELLE_RIG=./target/release/elle-rig
CARGO_PROFILE=--release`. The debug default takes hours. One file runs as
`elle tests/lang/NAME.lisp`, or `elle-rig tests/impl/NAME.lisp` with its sidecar.

[CONTRIBUTING.md](../CONTRIBUTING.md) holds the full table and the policy
around it.

## Fixtures

`tests/fixtures/` holds static files the Rust tests read:
`naming-good.lisp` and `naming-bad.lisp` for the linter
(`integration/lint.rs`), `gated-toplevel.lisp` for the dispatch tests, and
`unicode16.lisp` for the Unicode generation tests.
