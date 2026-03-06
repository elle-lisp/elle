# Testing Strategy

This document describes where every test in Elle belongs and how to run them.
Follow the decision tree when writing new tests.

## Test execution order

Both locally and in CI, tests run in this order. Fail fast: if a cheaper tier
fails, skip the expensive ones.

| Tier | What | Time | Purpose |
|------|------|------|---------|
| 1 | `examples/*.lisp` | ~2s | Smoke test across the whole language surface |
| 2 | `tests/elle/*.lisp` | seconds–minutes | Behavioral tests (407 tests) |
| 3 | `cargo test` (unit + integration) | ~5min | Rust tests (compile errors, error messages, type inspection) |
| 4 | `cargo test property::` | ~10min | Property tests (102 proptest blocks) |

Examples are the cheapest full-pipeline smoke test: reader, expander, analyzer,
lowerer, emitter, VM, and a broad swath of primitives in ~2 seconds. If an
example fails, nothing else is worth running.

## Decision tree

For any test you need to write, answer these questions in order:

**1. Does the test need access to Rust types, APIs, or compiler internals?**

Examples: inspecting `HirKind` variants, checking `Effect` values, calling
`analyze()` or `compile()` directly, testing `Value` constructors, examining
`Lexer`/`Reader` output, verifying bytecode disassembly, testing JIT internals.

→ **Rust test.** Go to "Which Rust test category?" below.

**2. Does the test assert that something fails at compile time?**

Code that should be rejected by the analyzer or lowerer before the VM ever
runs — undefined variables, break across function boundaries, invalid
destructuring syntax, arity mismatches at known call sites.

→ **Rust integration test.** The code cannot be run as an Elle script because
it does not compile. Use `eval_source(input).is_err()` and inspect the error
message.

**3. Does the test assert that something fails at runtime and need to inspect
the error message for specific content?**

Example: checking that a division-by-zero error message contains
"division by zero", or that an undefined variable error includes the
variable name.

→ **Rust integration test IF** the assertion requires substring matching on
the error message that `try`/`catch` in Elle cannot express. If the test only
needs to confirm that an error occurs (not inspect its message), it can be
Elle — use `protect` and check the error kind keyword.

**4. Does the test evaluate Elle source and check the resulting value?**

This is the vast majority of tests. The pattern is:
`assert_eq!(eval_source("(some-expr)").unwrap(), Value::int(42))`.

→ **Elle test script** in `tests/elle/`. Translate to:
`(assert-eq (some-expr) 42 "description")`.

**5. Does the test verify a runtime error occurs (not a compile error)
and only needs to check the error kind, not the full message?**

Example: confirming division by zero signals an error with kind
`:division-by-zero`.

→ **Elle test script.** Use `protect`:
```
(def [ok? err] (protect (/ 1 0)))
(assert-false ok? "division by zero should error")
(assert-eq (get err 0) :division-by-zero "error kind")
```

## Which Rust test category?

| Need | Location | When |
|------|----------|------|
| Access to private items (`pub(crate)` or less) | Inline `#[cfg(test)]` in the source file | Testing implementation details of a single module |
| Access to public Rust APIs, no pipeline | `tests/unittests/` | Testing `Value`, `SymbolTable`, primitives via Rust calls |
| Access to intermediate pipeline stages | `tests/integration/` | Testing `analyze()`, `compile()`, HIR/LIR structure, effects |
| Compile-time rejection | `tests/integration/` | Code that must not compile |
| Runtime error message inspection | `tests/integration/` | Substring matching on error strings |
| VM internals (scope stack, frames) | `tests/vm/` | Below integration, above unit |
| Invariants across generated inputs | `tests/property/` | Property-based tests with proptest |

For Rust integration tests that don't call stdlib functions (map, filter,
fold, etc.), prefer `eval_source_bare` over `eval_source` — it skips stdlib
initialization and is faster. Prelude macros (defn, let*, ->, etc.) are
still available with `eval_source_bare`.

## Elle test scripts

Elle test scripts live in `tests/elle/` — one `.lisp` file per feature area.
Each file is a self-contained test that imports `examples/assertions.lisp`
and exits non-zero on failure.

### Structure

```lisp
(import-file "./examples/assertions.lisp")

# Description of what this file tests

(assert-eq (+ 1 2) 3 "basic addition")
(assert-eq (- 10 3) 7 "basic subtraction")
# ... more assertions ...
```

### Assertion library

Elle test scripts use `examples/assertions.lisp` which provides:
`assert-eq`, `assert-true`, `assert-false`, `assert-list-eq`,
`assert-not-nil`, `assert-string-eq`.

For runtime error checking, use `protect`:

```lisp
(def [ok? err] (protect (/ 1 0)))
(assert-false ok? "division by zero should error")
(assert-eq (get err 0) :division-by-zero "error kind")
```

### Naming

Files are named for the feature they test: `core.lisp`, `booleans.lisp`,
`destructuring.lisp`, `blocks.lisp`, `closures.lisp`, etc.

### Granularity

One file should cover a coherent feature area. A file can contain hundreds
of assertions. The overhead is one pipeline initialization per file instead
of one per assertion.

### Relationship to `examples/`

`examples/` files are curated documentation that happens to be tested.
`tests/elle/` files are test suites that happen to be readable. Different
audiences, different goals:

- `examples/` — teaches the language, shows idiomatic patterns, includes
  `display`/`print` output, organized as themed demonstrations
- `tests/elle/` — verifies behavior, maximizes coverage, no output noise,
  organized by feature area

Do not merge test scripts into examples or vice versa.

### What NOT to put in Elle scripts

- Tests that require Rust type inspection (see decision tree)
- Compile-time rejection tests
- Property-based tests
- Tests for the Rust API surface (`Value` methods, `SymbolTable` API)

## Property tests

Property tests use proptest to verify invariants across generated inputs.
They live in `tests/property/` and answer: "Does this invariant hold for
*all* valid inputs?"

### The PROPTEST_CASES knob

All 102 `proptest!` blocks use the `proptest_cases()` helper from
`tests/common/mod.rs`:

```rust
proptest! {
    #![proptest_config(crate::common::proptest_cases(200))]

    #[test]
    fn my_property(n in -1000i64..1000) {
        // ...
    }
}
```

The `default` parameter (200 in this example) is the per-test tuning. The
`PROPTEST_CASES` environment variable overrides all tests uniformly when set.

### Case count guidelines

| Cost per case | Default cases | Example |
|---------------|---------------|---------|
| Cheap (pure Rust, no eval) | 1000 | NaN-boxing roundtrips, effect combine |
| Medium (single eval) | 200 | Arithmetic properties, reader roundtrips |
| Expensive (multiple evals, fibers, coroutines) | 10–50 | Pipeline properties, fiber determinism |

### Running property tests

```bash
# Fast smoke (development, CI fast tier)
PROPTEST_CASES=8 cargo test property::

# Default case counts (CI thorough tier, pre-merge)
cargo test property::

# Targeted
cargo test property::arithmetic
```

## CI structure

| Job | Trigger | What | Proptest cases |
|-----|---------|------|----------------|
| examples | Always | All `.lisp` files in `examples/` | — |
| test-rust | After examples | Unit + integration tests (skip property) | — |
| test-property | After test-rust | All property tests | 8 (PR) / 16 (merge queue) |
| toolchain-check | Weekly | Full suite on beta/nightly | 128 |

Examples gate everything. Fast tier (PR): ~5 minutes. Thorough tier (merge
queue): ~30 minutes. Weekly: full coverage on beta/nightly.

## Local development workflow

```bash
# Smoke test (what agents should run first)
make smoke

# Fast feedback (examples + elle scripts + unit tests)
make smoke

# Run only Elle scripts
cargo test elle::

# Run only property tests, reduced
PROPTEST_CASES=8 cargo test property::

# Run a specific Elle test script
cargo run -- tests/elle/core.lisp

# Full suite (before opening PR, or let CI handle it)
cargo test --workspace
```

## What stays in Rust

These tests cannot be expressed in Elle and must remain in Rust:

| Category | Files | Reason |
|----------|-------|--------|
| Compile-time errors | `core.rs` (17), `destructuring.rs` (3), `splice.rs` (6), `blocks.rs` (3) | Code that must not compile |
| Error message inspection | `error_reporting.rs` | Substring matching on error strings |
| CLI subprocess tests | `dispatch.rs` (7) | Testing exit codes and subprocess behavior |
| Float precision | `core.rs` | Testing exact float values (NaN, Inf, precision) |
| Type introspection | `effect_enforcement.rs`, `hir_debug.rs`, `lir_debug.rs` | Inspecting HIR/LIR/Effect types |
| Pipeline internals | `pipeline.rs`, `pipeline_property.rs`, `new_pipeline_property.rs` | Intermediate pipeline stages |
| LSP protocol | `lsp.rs` | Language server protocol implementation |
| JIT internals | `jit.rs` | JIT compilation pipeline |
| FFI marshalling | `ffi.rs` | FFI type/value roundtrips |

## Test helpers

### `common/mod.rs`

**`eval_source(input: &str) -> Result<Value, String>`** — The canonical test
eval. Evaluates Elle source through the full pipeline with stdlib. Use this
for any test that needs to run Elle code.

```rust
use crate::common::eval_source;
let result = eval_source("(+ 1 2)").unwrap();
assert_eq!(result, Value::int(3));
```

**`eval_source_bare(input: &str) -> Result<Value, String>`** — Same as
`eval_source` but skips stdlib initialization. Use this for tests that never
call stdlib functions. Prelude macros are still available.

**`setup() -> (SymbolTable, VM)`** — Returns an initialized pair with
primitives and stdlib registered. Use this when you need direct access to the
VM or symbol table (e.g., calling `analyze()` or `compile()` directly).

**`proptest_cases(default: u32) -> ProptestConfig`** — Create a proptest
config that respects the `PROPTEST_CASES` environment variable. When set, it
overrides the given default uniformly across all tests.

### `property/strategies.rs`

8 public strategies for generating Elle values and FFI types:

| Strategy | Generates |
|----------|-----------|
| `arb_immediate()` | nil, empty_list, true, false, ints, floats, symbols |
| `arb_value()` | Everything + strings, cons, arrays (depth 3) |
| `arb_primitive_type()` | FFI primitive TypeDesc variants |
| `arb_type_desc(depth)` | Primitive + compound types |
| `arb_flat_struct()` | StructDesc with 1-6 primitive fields |
| `arb_value_for_type(desc)` | Value matching a given TypeDesc |
| `arb_typed_value()` | (TypeDesc, Value) pair |
| `arb_struct_and_values()` | (StructDesc, Value::array) pair |

## Running tests

```bash
# Full test suite
cargo test --workspace

# Just the main crate
cargo test

# Specific test by name
cargo test test_name

# All tests in a category
cargo test property::          # All property tests
cargo test integration::       # All integration tests
cargo test unittests::         # All unit tests
cargo test vm::                # All VM tests
cargo test elle::              # All Elle script tests

# Run all examples as tests
cargo test --test '*'

# Run with output
cargo test test_name -- --nocapture

# Run a single example file
cargo run -- examples/closures.lisp

# Run a single Elle script
cargo run -- tests/elle/core.lisp
```

## Adding a new test

### Elle test script

1. Add assertions to an existing `tests/elle/*.lisp` or create a new file
2. Import `examples/assertions.lisp` at the top
3. Use `assert-eq`, `assert-true`, etc. from the library
4. Run: `cargo run -- tests/elle/myfile.lisp`

### Rust integration test

1. Create `tests/integration/myfeature.rs`
2. Add to `tests/integration/mod.rs`:
   ```rust
   mod myfeature {
       include!("myfeature.rs");
   }
   ```
3. Import `crate::common::eval_source` and write tests
4. Run: `cargo test integration::myfeature`

### Property test

1. Create `tests/property/myfeature.rs`
2. Add to `tests/property/mod.rs`:
   ```rust
   mod myfeature {
       include!("myfeature.rs");
   }
   ```
3. Use `proptest!` with `#![proptest_config(crate::common::proptest_cases(N))]`
4. Run: `PROPTEST_CASES=8 cargo test property::myfeature`

### Unit test

1. Create `tests/unittests/mymodule.rs`
2. Add to `tests/unittests/mod.rs`:
   ```rust
   mod mymodule {
       include!("mymodule.rs");
   }
   ```
3. Import Rust APIs directly — no `eval_source` needed
4. Run: `cargo test unittests::mymodule`

### Inline test

Add a `#[cfg(test)]` module at the bottom of the `src/` file you're testing.
This gives access to private items. No registration needed.

## Naming conventions

- Test files: lowercase, hyphenated concepts joined with underscores
  (e.g., `closures_and_lambdas.rs`, `effect_enforcement.rs`)
- Test functions: `test_` prefix for example-based, descriptive name for
  property tests (e.g., `fn int_roundtrip(...)`, `fn add_commutative(...)`)
- Property test names describe the invariant, not the implementation

## Fixtures

`tests/fixtures/` contains static data files used by tests:

- `naming-good.lisp` — Elle source with correct kebab-case naming
- `naming-bad.lisp` — Elle source with camelCase/PascalCase/snake_case naming

Used by `integration/lint.rs` to test the linter against real files.

## Failure triage

| Failure | Symptom | Likely cause | Fix |
|---------|---------|--------------|-----|
| **elle-doc generation** | `docs` job fails on `./target/release/elle elle-doc/generate.lisp` | Using `nil?` to check end-of-list. Lists terminate with `EMPTY_LIST`, not `NIL`. | Use `empty?` for list termination checks. Check `elle-doc/generate.lisp` and `elle-doc/lib/`. |
| **Examples fail** | `examples` job fails | Runtime error in `.lisp` file. Assertions use `assert-eq`, `assert-true`, etc. from `examples/assertions.lisp`. | Run `cargo run -- examples/failing.lisp` locally. Check assertion message. |
| **Elle scripts fail** | `examples` job fails on Elle script tests | Runtime error in `tests/elle/*.lisp`. | Run `cargo run -- tests/elle/failing.lisp` locally. Check assertion message. |
| **Property tests fail** | `test-property` job fails with shrunk counterexample | The shrunk output shows the *minimal* failing input. | Reproduce with the exact shrunk values as a unit test. Check `proptest-regressions/` files. |
| **Integration tests fail** | `test-rust` job fails | Tests use `eval_source()` which runs the full pipeline. | Read the assertion. Check whether the test expects `.unwrap()` (success) or `.is_err()` (error). |
| **Clippy** | `clippy` job fails | Any Rust warning. CI runs with `-D warnings`. | Run `cargo clippy --workspace --all-targets -- -D warnings` locally. |
| **Formatting** | `fmt` job fails | Unformatted Rust code. | Run `cargo fmt`. |
| **Rustdoc** | `docs` job fails on `cargo doc` step | Broken intra-doc links or malformed doc comments. | Run `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` locally. |
