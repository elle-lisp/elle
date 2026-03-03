# Testing Strategy

This document defines where every test in Elle belongs. Follow it when writing
new tests or migrating existing ones. When in doubt, apply the decision tree.

## Status

This document describes a target architecture. Some infrastructure exists
today; some must be built. Each section is marked:

- **Current** — exists and works today
- **Proposed** — does not yet exist; must be built before use

Until the proposed infrastructure is built, follow `tests/AGENTS.md` for
current conventions. When a proposed item is implemented, update this
document to mark it as current and update `tests/AGENTS.md` to match.

## Test execution order

Both locally and in CI, tests should run in this order. Fail fast: if a
cheaper tier fails, skip the expensive ones.

| Tier | What | Time | Purpose | Status |
|------|------|------|---------|--------|
| 1 | `examples/*.lisp` | ~2s | Smoke test across the whole language surface | current |
| 2 | `tests/elle/*.lisp` | seconds–minutes | Behavioral tests | proposed |
| 3 | `cargo test` with `PROPTEST_CASES=8` | ~2–5min | Rust unit/integration/property smoke | proposed |
| 4 | `cargo test` with default case counts | ~30min | Thorough property sweep (merge gate only) | proposed |

Examples are the cheapest full-pipeline smoke test: reader, expander,
analyzer, lowerer, emitter, VM, and a broad swath of primitives in ~2
seconds. If an example fails, nothing else is worth running.

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
it does not compile. Use `compile(input, &mut symbols).unwrap_err()` or
`eval_source(input).is_err()` and inspect the error message.

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

This is the vast majority of integration tests. The pattern is:
`assert_eq!(eval_source("(some-expr)").unwrap(), Value::int(42))`.

→ **Elle test script** in `tests/elle/`. (Proposed — see below.) Translate to:
`(assert-eq (some-expr) 42 "description")`.

**5. Does the test verify a runtime error occurs (not a compile error)
and only needs to check the error kind, not the full message?**

Example: confirming division by zero raises an error with kind
`:division-by-zero`.

→ **Elle test script.** Use `protect`:
```
(def [ok? err] (protect (/ 1 0)))
(assert-false ok? "division by zero should error")
(assert-eq (get err 0) :division-by-zero "error kind")
```

(The error kind `:division-by-zero` is verified — see
`src/primitives/arithmetic.rs` and `src/vm/arithmetic.rs`.)

## Which Rust test category?

**Current.** If the decision tree sends you to a Rust test:

| Need | Location | When |
|------|----------|------|
| Access to private items (`pub(crate)` or less) | Inline `#[cfg(test)]` in the source file | Testing implementation details of a single module |
| Access to public Rust APIs, no pipeline | `tests/unittests/` | Testing `Value`, `SymbolTable`, primitives via Rust calls |
| Access to intermediate pipeline stages | `tests/integration/` | Testing `analyze()`, `compile()`, HIR/LIR structure, effects |
| Compile-time rejection | `tests/integration/` | Code that must not compile |
| Runtime error message inspection | `tests/integration/` | Substring matching on error strings |
| VM internals (scope stack, frames) | `tests/vm/` | Below integration, above unit |
| Invariants across generated inputs | `tests/property/` or `tests/integration/` | Property-based tests with proptest |

For Rust integration tests that don't call stdlib functions (map, filter,
fold, etc.), prefer `eval_source_bare` over `eval_source` — it skips stdlib
initialization and is faster. Prelude macros (defn, let*, ->, etc.) are
still available with `eval_source_bare`.

Note: property tests currently exist in both `tests/property/` (pure
domain invariants) and `tests/integration/` (pipeline-level invariants like
`pipeline_property.rs`, `new_pipeline_property.rs`, `time_property.rs`).
Both locations are valid — the distinction is domain vs pipeline scope, not
property vs example.

## Elle test scripts

**Proposed.** This infrastructure does not yet exist.

### Location

`tests/elle/` — one `.lisp` file per feature area or theme.

### Structure

Every Elle test script follows this pattern:

```
(import-file "./examples/assertions.lisp")

# Description of what this file tests

(assert-eq (+ 1 2) 3 "basic addition")
(assert-eq (- 10 3) 7 "basic subtraction")
# ... more assertions ...
```

### Assertion library

Elle test scripts use the existing `examples/assertions.lisp` which provides:
`assert-eq`, `assert-true`, `assert-false`, `assert-list-eq`,
`assert-not-nil`, `assert-string-eq`.

When tests need error-checking assertions, extend `examples/assertions.lisp`
with these functions:

```lisp
# Assert that a thunk raises any error
(defn assert-err [f msg]
  "Assert that (f) raises an error"
  (def [ok? _] (protect (f)))
  (if ok?
    (begin (display "FAIL: ") (display msg) (display "\n  Expected error, got success\n") (exit 1))
    true))

# Assert that a thunk raises an error with a specific kind keyword
(defn assert-err-kind [f expected-kind msg]
  "Assert that (f) raises an error with the given kind"
  (def [ok? err] (protect (f)))
  (if ok?
    (begin (display "FAIL: ") (display msg) (display "\n  Expected error, got success\n") (exit 1))
    (assert-eq (get err 0) expected-kind msg)))
```

These take thunks (zero-argument functions) because `protect` is a macro
that wraps its body. Usage in test scripts:

```lisp
(assert-err (fn [] (/ 1 0)) "division by zero should error")
(assert-err-kind (fn [] (/ 1 0)) :division-by-zero "error kind check")
```

### Naming

Files are named for the feature they test, matching the existing convention:
`core.lisp`, `booleans.lisp`, `destructuring.lisp`, `blocks.lisp`,
`closures.lisp`, etc.

### Granularity

One file should cover a coherent feature area. A file can contain hundreds
of assertions. Each `eval_source` call in the Rust tests becomes a single
`assert-*` call in Elle. The overhead is one pipeline initialization per
file instead of one per assertion.

### Relationship to `examples/`

`examples/` files are curated documentation that happens to be tested.
`tests/elle/` files are test suites that happen to be readable. Different
audiences, different goals:

- `examples/` — teaches the language, shows idiomatic patterns, includes
  `display`/`print` output, organized as themed demonstrations
- `tests/elle/` — verifies behavior, maximizes coverage, no output noise,
  organized by feature area

Do not merge test scripts into examples or vice versa.

### Working directory assumption

Elle test scripts use `import-file` with paths relative to the project
root (e.g., `"./examples/assertions.lisp"`). This works because `cargo test`
and `cargo run` both set the working directory to the project root.

### What NOT to put in Elle scripts

- Tests that require Rust type inspection (see decision tree)
- Compile-time rejection tests
- Property-based tests (until Elle has its own property test library)
- Tests for the Rust API surface (`Value` methods, `SymbolTable` API)

### Stdlib note

Some existing Rust integration tests intentionally skip stdlib
initialization (using `eval_source_bare` or a local `run()` helper that
calls `eval` without `init_stdlib`). Elle test scripts run through the full
interpreter, which always loads stdlib. This is fine — the prelude macros
(defn, let*, ->, etc.) are loaded by the Expander regardless, and stdlib
functions being available doesn't affect tests that don't call them.

## Property tests

### The PROPTEST_CASES knob

**Proposed.** Currently all ~100 `proptest!` blocks hardcode case counts via
`ProptestConfig::with_cases(N)`, which ignores the `PROPTEST_CASES`
environment variable. The CI sets `PROPTEST_CASES=32` but it has no effect.

The fix: a shared helper in `tests/common/mod.rs`:

```rust
pub fn proptest_cases(default: u32) -> ProptestConfig {
    let cases = std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default);
    ProptestConfig::with_cases(cases)
}
```

Usage in test files:

```rust
proptest! {
    #![proptest_config(crate::common::proptest_cases(200))]

    #[test]
    fn my_property(n in -1000i64..1000) {
        // ...
    }
}
```

The `default` parameter preserves the per-test tuning. The environment
variable overrides all tests uniformly when set. This is a mechanical
replacement across ~100 call sites.

When this helper is implemented, update `tests/AGENTS.md` to document
the new convention.

### Running property tests

```bash
# Fast smoke (development, CI fast tier)
PROPTEST_CASES=8 cargo test

# Default case counts (CI thorough tier, pre-merge)
cargo test

# Targeted
cargo test property::arithmetic
```

### Case count guidelines

| Cost per case | Default cases | Example |
|---------------|---------------|---------|
| Cheap (pure Rust, no eval) | 1000 | NaN-boxing roundtrips, effect combine |
| Medium (single eval) | 200 | Arithmetic properties, reader roundtrips |
| Expensive (multiple evals, fibers, coroutines) | 10–50 | Pipeline properties, fiber determinism |

## Test runner

### Rust-side harness (proposed)

A Rust integration test discovers and runs all Elle test scripts:

```rust
// tests/integration/elle_scripts.rs
#[test]
fn run_elle_test_scripts() {
    let test_dir = Path::new("tests/elle");
    if !test_dir.exists() {
        return; // No Elle tests yet
    }
    let mut failures = Vec::new();
    for entry in fs::read_dir(test_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_file()
           && path.extension() == Some("lisp".as_ref()) {
            let output = Command::new(env!("CARGO_BIN_EXE_elle"))
                .arg(&path)
                .output()
                .unwrap();
            if !output.status.success() {
                failures.push(format!(
                    "{}: {}",
                    path.display(),
                    String::from_utf8_lossy(&output.stderr)
                ));
            }
        }
    }
    assert!(failures.is_empty(),
        "{} Elle test(s) failed:\n{}",
        failures.len(),
        failures.join("\n---\n"));
}
```

This runs all scripts and reports all failures, not just the first.
Each script runs as a separate process. The `tests/elle/` directory is
flat — just `.lisp` test files. Assertions come from
`examples/assertions.lisp` via `import-file`.

## CI structure

### Current

The actual CI (`ci.yml`) has separate jobs: test, fmt, clippy, audit,
examples, docs, benchmarks, toolchain-check, and all-checks. The test
job sets `PROPTEST_CASES=32` which is currently ignored.

### Target

```yaml
examples:
  name: Examples (smoke)
  # ... (already exists, runs in ~2s)

test-fast:
  name: Fast Tests
  needs: examples
  run: cargo test --workspace
  env:
    PROPTEST_CASES: 8

test-thorough:
  name: Thorough Property Tests
  needs: test-fast
  run: cargo test --workspace
  # No override — uses per-test defaults
```

Examples gate everything. Fast tier: ~5 minutes, gives red/green on every
push. Thorough tier: ~30 minutes, required for merge to main.

## Migration path

Migrating existing Rust integration tests to Elle scripts:

1. **Pick a file** from `tests/integration/` (e.g., `booleans.rs`).
2. **Apply the decision tree** to each test function in the file.
3. **Tests that go to Elle**: translate to `assert-*` calls in the
   corresponding `tests/elle/*.lisp` file.
4. **Tests that stay in Rust**: leave them. If the file is mostly
   emptied, the remaining Rust tests stay in place.
5. **Delete the Rust test** after the Elle equivalent is verified.
6. **Never duplicate** — a test exists in exactly one place.

### Priority order for migration

Start with files that are nearly 100% translatable:

1. `booleans.rs` — trivial value assertions, fully translatable
2. `core.rs` — ~95% translatable (keep error-message-inspection tests)
3. `destructuring.rs` — ~90% translatable (keep compile-error tests)
4. `blocks.rs` — ~85% translatable (keep compile-error tests)
5. `prelude.rs` — mostly behavioral, high translation rate
6. `splice.rs`, `dispatch.rs`, `eval.rs` — behavioral

Leave these in Rust:
- `effect_enforcement.rs` — inspects HIR/Effect types directly
- `error_reporting.rs` — tests error infrastructure (Lexer, Reader,
  error formatting, LocationMap, VM stack traces) via Rust APIs
- `pipeline.rs`, `pipeline_property.rs`, `new_pipeline_property.rs` —
  intermediate pipeline stages and pipeline-level property tests
- `lsp.rs` — tests LSP protocol implementation
- `jit.rs` — tests JIT internals
- `ffi.rs` — tests FFI marshalling

## Local development workflow

```bash
# Smoke test (what agents should run first)
cargo run -- examples/basics.lisp  # or run all examples

# Fast feedback
PROPTEST_CASES=8 cargo test

# Run only Elle scripts (once infrastructure exists)
cargo test elle_scripts

# Run only property tests, reduced
PROPTEST_CASES=8 cargo test property::

# Run a specific Elle test script
cargo run -- tests/elle/core.lisp

# Full suite (before opening PR, or let CI handle it)
cargo test --workspace
```

## Implementation steps

These must be completed to activate the proposed infrastructure:

1. **Create `tests/elle/` directory** with at least one test file
   (start with `booleans.lisp` — the simplest migration candidate)
2. **Add `assert-err` and `assert-err-kind` to
   `examples/assertions.lisp`** for runtime error checking
3. **Create `tests/integration/elle_scripts.rs`** with the Rust-side
   harness (register in `tests/integration/mod.rs`)
4. **Create `proptest_cases` helper** in `tests/common/mod.rs`
5. **Mechanically replace** all `ProptestConfig::with_cases(N)` with
   `crate::common::proptest_cases(N)` (~100 call sites)
6. **Update `tests/AGENTS.md`** to reference this document and reflect
   the new conventions
7. **Update CI** to add examples as prerequisite and split test tiers
8. **Migrate `booleans.rs`** as the proof-of-concept, delete the Rust
   version once verified

## Checklist for new tests

Before writing a test, run through this:

- [ ] Applied the decision tree — I know whether this is Elle or Rust
- [ ] If Elle: added to an existing `tests/elle/*.lisp` or created a new
      file with the standard header
- [ ] If Rust property test: used `crate::common::proptest_cases(N)`,
      not `ProptestConfig::with_cases(N)` (once the helper exists)
- [ ] If Rust integration: this test genuinely needs Rust (error
      inspection, compile rejection, type introspection)
- [ ] Test is in exactly one place — no duplication across tiers
