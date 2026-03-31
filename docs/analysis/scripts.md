# Test Scripts

Writing and organizing Elle test scripts.

## Elle test scripts


Elle test scripts live in `tests/elle/` — one `.lisp` file per feature area.
Each file is a self-contained test that imports `examples/assertions.lisp`
and exits non-zero on failure.

### Structure

```janet
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

```janet
(def [ok? err] (protect (/ 1 0)))
(assert-false ok? "division by zero should error")
(assert-eq (get err :error) :division-by-zero "error kind")
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
- Tests for the Rust API surface (`Value` methods, `SymbolTable` API)

Note: Property-based tests belong in `tests/property/` only if random input
generation genuinely finds bugs. If you're testing a fixed set of known-good
examples, write Elle test scripts instead — they're faster and clearer.


## Property tests


Property tests use proptest to verify invariants across generated inputs.
They live in `tests/property/` and answer: "Does this invariant hold for
*all* valid inputs?"

**Use property tests only when random input generation genuinely finds bugs.**
If you're testing a fixed set of known-good examples, write Elle test scripts
instead — they're faster and clearer. Property testing is the wrong tool for
concrete cases.

### The PROPTEST_CASES environment variable

All `proptest!` blocks use the `proptest_cases()` helper from
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

The `default` parameter (200 in this example) is the per-test tuning. **Do NOT
change hardcoded case counts in test files.** Instead, use the `PROPTEST_CASES`
environment variable to override all tests uniformly:

```bash
# Fast smoke during development (8 cases per test)
PROPTEST_CASES=8 cargo test property::

# Full run with per-test defaults (CI)
cargo test property::

# Targeted run with custom case count
PROPTEST_CASES=50 cargo test property::fibers
```

When `PROPTEST_CASES` is set, it overrides the hardcoded default in every
`proptest_cases(N)` call. This allows CI and local development to control
case counts uniformly without modifying test files.

### Case count guidelines

| Cost per case | Default cases | Example |
|---------------|---------------|---------|
| Cheap (pure Rust, no eval) | 1000 | Value roundtrips, signal combine |
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


## What stays in Rust


These tests cannot be expressed in Elle and must remain in Rust:

| Category | Files | Reason |
|----------|-------|--------|
| Compile-time errors | `core.rs` (17), `destructuring.rs` (3), `splice.rs` (6), `blocks.rs` (3) | Code that must not compile |
| Error message inspection | `error_reporting.rs` | Substring matching on error strings |
| CLI subprocess tests | `dispatch.rs` (7) | Testing exit codes and subprocess behavior |
| Float precision | `core.rs` | Testing exact float values (NaN, Inf, precision) |
| Type introspection | `signal_enforcement.rs`, `hir_debug.rs`, `lir_debug.rs` | Inspecting HIR/LIR/Signal types |
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


## Naming conventions


- Test files: lowercase, hyphenated concepts joined with underscores
  (e.g., `closures_and_lambdas.rs`, `signal_enforcement.rs`)
- Test functions: `test_` prefix for example-based, descriptive name for
  property tests (e.g., `fn int_roundtrip(...)`, `fn add_commutative(...)`)
- Property test names describe the invariant, not the implementation


## Fixtures


`tests/fixtures/` contains static data files used by tests:

- `naming-good.lisp` — Elle source with correct kebab-case naming
- `naming-bad.lisp` — Elle source with camelCase/PascalCase/snake_case naming

Used by `integration/lint.rs` to test the linter against real files.



---

## See also

- [Analysis index](index.md)
