# Tests

<!-- audited: 2026-09-29 -->

Where each kind of test lives, the helpers they share, and how to add one.

Which kind of test to write is the decision tree in
[docs/analysis/testing.md](../docs/analysis/testing.md). How the Elle corpus
and its runner work is [docs/testing.md](../docs/testing.md).

## Directory structure

```
tests/
├── lib.rs              # The shared binary: includes unittests/, integration/, property/
├── common/mod.rs       # Shared helpers (eval_source, setup, the Makefile and workflow readers)
├── fixtures/           # Static files the Rust tests read
├── elle/               # The Elle corpus, run by `elle test`
├── property/           # Property-based tests (proptest)
├── integration/        # Full-pipeline and repository tests
├── unittests/          # Rust APIs tested directly
├── io_copies/          # A helper module two standalone binaries share
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

### Elle test scripts (`tests/elle/`)

Behavioral tests that evaluate Elle source and check values, written with the
`(assert COND "message")` idiom. The agent-first runner (`elle test`, through
`make smoke` or `make smoke-elle`) compiles each file and runs it once per JIT
policy (`:off`→`vm`, `:eager`→`jit`), with per-tier divergence for single-form
files. `integration::elle_scripts` is *not* the driver: it keeps only the files
that need a process-global runtime mode the runner cannot vary per file
(`--trace=guardfree`, `--mlir=off`+adaptive).

An Elle script answers "does this Elle expression produce the expected value?"
Leave these to Rust: a test that needs a Rust type, a compile-time rejection
that needs a Rust type, and a property test.

### Property tests (`tests/property/`)

Invariants that must hold across all generated inputs: roundtrip fidelity,
mathematical laws, type discrimination, determinism, signal inference
soundness, and a defect's regression across an input range.
[property/AGENTS.md](property/AGENTS.md) covers the directory.

### Integration tests (`tests/integration/`)

End-to-end behavior through the whole pipeline (Reader → Expander → Analyzer →
Lowerer → Emitter → VM), plus the tests that check the repository itself: the
documents, the CI workflow, and the corpus runner.
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
- **`make_var`, `make_dry_run`, `workflow_jobs`** — what the Makefile and the
  workflows say, for the tests that check them.

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
| `make smoke` | ~30 min, release | The corpus, the doctests, and the embedding demo |
| `make test` | smoke + ~5 min | What the PR gate runs, locally |
| `cargo test --workspace` | ~30 min | Everything — ask before running it |

[CONTRIBUTING.md](../CONTRIBUTING.md) holds the full table and the policy
around it.

## Fixtures

`tests/fixtures/` holds static files the Rust tests read:
`naming-good.lisp` and `naming-bad.lisp` for the linter
(`integration/lint.rs`), `gated-toplevel.lisp` for the dispatch tests, and
`unicode16.lisp` for the Unicode generation tests.
