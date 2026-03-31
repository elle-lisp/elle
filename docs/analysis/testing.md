# Testing Strategy

Where every test belongs and how to run them.

## Test execution order


Both locally and in CI, tests run in this order. Fail fast: if a cheaper tier
fails, skip the expensive ones.

| Tier | What | Time | Purpose |
|------|------|------|---------|
| 1 | `examples/*.lisp` | ~2s | Smoke test across the whole language surface |
| 2 | `tests/elle/*.lisp` | ~6s | Behavioral tests (Elle semantics) |
| 3 | `cargo test` (unit + integration) | ~15min | Rust tests (compile errors, error messages, type inspection) |
| 4 | `cargo test property::` | ~30min | Property tests (invariants across generated inputs) |

Examples are the cheapest full-pipeline smoke test: reader, expander, analyzer,
lowerer, emitter, VM, and a broad swath of primitives in ~2 seconds. If an
example fails, nothing else is worth running.

Elle test scripts are the next tier: they verify language semantics by running
Elle code directly, with no Rust-level setup. They're faster than integration
tests because they skip Rust type inspection and error message matching.

Integration tests are slower because they require Rust-level setup (VM
construction, symbol table initialization, error message inspection).

Property tests are the slowest because they run many generated test cases.
However, they're only necessary when random input generation genuinely finds
bugs that concrete cases would miss.


## Decision tree


For any test you need to write, answer these questions in order:

**1. Does the test need access to Rust types, APIs, or compiler internals?**

Examples: inspecting `HirKind` variants, checking `Signal` values, calling
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
(assert-eq (get err :error) :division-by-zero "error kind")
```

**6. Does the test use random input generation to find bugs?**

Property tests use proptest to generate random inputs and verify that an
invariant holds across all of them. This is valuable when randomness genuinely
finds bugs that concrete cases would miss — e.g., testing that a roundtrip
property holds for all possible values, or that a mathematical law (like
commutativity) holds across all inputs.

However, if you're really just testing a fixed set of known-good examples
(e.g., "yield 3 values, resume 3 times, get them back in order"), property
testing is the wrong tool. Write Elle test scripts instead — they're faster
and clearer.

→ **Property test** in `tests/property/` IF random generation genuinely adds
value. Otherwise, write Elle test scripts.


## Which Rust test category?


| Need | Location | When |
|------|----------|------|
| Access to private items (`pub(crate)` or less) | Inline `#[cfg(test)]` in the source file | Testing implementation details of a single module |
| Access to public Rust APIs, no pipeline | `tests/unittests/` | Testing `Value`, `SymbolTable`, primitives via Rust calls |
| Access to intermediate pipeline stages | `tests/integration/` | Testing `analyze()`, `compile()`, HIR/LIR structure, signals |
| Compile-time rejection | `tests/integration/` | Code that must not compile |
| Runtime error message inspection | `tests/integration/` | Substring matching on error strings |
| VM internals (scope stack, frames) | `tests/vm/` | Below integration, above unit |
| Invariants across generated inputs | `tests/property/` | Property-based tests with proptest |

For Rust integration tests that don't call stdlib functions (map, filter,
fold, etc.), prefer `eval_source_bare` over `eval_source` — it skips stdlib
initialization and is faster. Prelude macros (defn, let*, ->, etc.) are
still available with `eval_source_bare`.


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



---

## See also

- [Analysis index](index.md)
