# Testing Strategy

Which *kind* of test to write, and where it belongs.

> For how the test systems work and how to run them — the agent-first runner
> (`elle test`), the session DB, `make smoke`/`make test`, reading results — see
> [`docs/testing.md`](../testing.md). This document is the **decision tree**:
> given a thing to test, is it an Elle script or a Rust test, and which kind?

## Test execution order

`make test` gates in this order; fail fast, cheapest first:

| Tier | What | Purpose |
|------|------|---------|
| 1 | `make smoke` → `elle test tests/elle/*.lisp` | The Elle corpus (semantics) across the vm and jit policies, in one process, recorded to a session DB |
| 2 | doctests + the embedding demo | Documentation examples and the C/Rust host |
| 3 | `cargo test` (fmt, clippy, crosscheck, rustdoc, unit, integration) | Rust tests (compile errors, error messages, type inspection); the cross-check compiles the macOS `cfg(target_os)` arms this box never builds |
| 4 | `cargo test property::` | Property tests (invariants across generated inputs) |

The Elle corpus is the cheapest full-pipeline check: reader, expander, analyzer,
lowerer, emitter, VM, JIT, and a broad swath of primitives. If it fails, the
session DB names every failing form (`elle test --summary`).

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

The same rule covers Unicode generation tests: corpus files compile on the
shared runner VM, which uses the default generation, so a file that selects
another generation with `(unicode! N)` cannot join the corpus. Build a
`Runtime::with_unicode(...)` in a Rust integration test instead.

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

For the Elle corpus and the runner (`elle test`, `--summary`, `--query`,
`make smoke`), see [`docs/testing.md`](../testing.md). The Rust suite:

```bash
# Full Rust suite
cargo test --workspace

# Just the main crate
cargo test

# Specific test by name
cargo test test_name

# A category
cargo test property::          # property tests
cargo test integration::       # integration tests
cargo test unittests::         # unit tests

# With output
cargo test test_name -- --nocapture

# A single Elle file directly (not via the runner)
cargo run -- tests/elle/core.lisp
```


## Adding a new test


### Elle test script

1. Add `(assert COND "message")` forms to an existing `tests/elle/*.lisp` or a
   new file (the runner scavenges the first assert message as the label).
2. Gate optional dependencies in-file with `:gated` — never `(exit 0)`.
3. Run via the runner: `elle test tests/elle/myfile.lisp`.

See [`docs/testing.md`](../testing.md) § Adding an Elle test for gating, native
teardown, and reading results.

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
