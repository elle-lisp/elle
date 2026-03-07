# tests/elle

Elle script tests: behavioral tests in Elle that verify language semantics.

## Responsibility

Test language behavior by running Elle code directly. Each `.lisp` file in this directory is a self-contained test that:
1. Imports `examples/assertions.lisp` for assertion helpers
2. Runs assertions to verify behavior
3. Exits with code 0 on success, 1 on failure

Does NOT:
- Test Rust APIs (that's unit tests)
- Test invariants across random inputs (that's property tests)
- Test individual modules in isolation (that's integration tests)

## Test structure

Each Elle script follows this pattern:

```janet
#!/usr/bin/env elle
## Test description

(import "examples/assertions.lisp")

## Test cases
(assert-eq (+ 1 2) 3)
(assert-true (> 5 3))
(assert-false (< 5 3))

## Exit with code 0 on success
```

The `examples/assertions.lisp` library provides:
- `assert-eq` — Assert equality
- `assert-true` — Assert truthy
- `assert-false` — Assert falsy
- `assert-error` — Assert error is signaled
- `assert-contains` — Assert string contains substring

All assertions call `(exit 1)` on failure, causing the script to exit with code 1.

## Test organization

Tests are organized by feature area:

| File | Coverage |
|------|----------|
| `eval.lisp` | Evaluation and basic forms |
| `prelude.lisp` | Prelude macros (defn, let*, when, unless, etc.) |
| `destructuring.lisp` | Destructuring patterns |
| `core.lisp` | Core language features |
| `splice.lisp` | Splice syntax |
| `blocks.lisp` | Block and break control flow |
| `functional.lisp` | Functional programming (map, filter, fold, etc.) |
| `arithmetic.lisp` | Arithmetic operations |
| `determinism.lisp` | Deterministic behavior |
| `property-eval.lisp` | Property-based evaluation |
| `convert.lisp` | Type conversions |
| `sequences.lisp` | List and array operations |
| `macros.lisp` | Macro behavior |
| `strings.lisp` | String operations |
| `tables.lisp` | Table operations |
| `fibers.lisp` | Fiber operations |
| `coroutines.lisp` | Coroutine behavior |
| `effects.lisp` | Effect system |
| `closures.lisp` | Closure behavior |
| `recursion.lisp` | Recursive functions |
| `higher-order.lisp` | Higher-order functions |
| `match.lisp` | Pattern matching |
| `parameters.lisp` | Dynamic parameters |
| `ports.lisp` | I/O ports |
| `json.lisp` | JSON serialization |
| `regex.lisp` | Regular expressions |
| `bytes.lisp` | Bytes operations |
| `buffer.lisp` | Buffer operations |

## Running Elle scripts

Elle scripts are run via the `integration::elle_scripts` harness in `tests/integration/elle_scripts.rs`:

```bash
cargo test elle_scripts::eval    # Run tests/elle/eval.lisp
cargo test elle_scripts          # Run all Elle scripts
```

Each script is executed with the `elle` binary:

```bash
./target/debug/elle tests/elle/eval.lisp
```

If the script exits with code 0, the test passes. If it exits with code 1, the test fails.

## Writing a new Elle script

1. Create `tests/elle/myfeature.lisp`
2. Add a test function to `tests/integration/elle_scripts.rs`:
   ```rust
   #[test]
   fn myfeature() {
       run_elle_script("myfeature");
   }
   ```
3. Write the script:
   ```janet
   #!/usr/bin/env elle
   ## My feature test

   (import "examples/assertions.lisp")

   (assert-eq (my-feature 42) 42)
   ```

## Assertion helpers

From `examples/assertions.lisp`:

| Helper | Usage | Notes |
|--------|-------|-------|
| `assert-eq` | `(assert-eq actual expected)` | Equality check |
| `assert-true` | `(assert-true expr)` | Truthy check |
| `assert-false` | `(assert-false expr)` | Falsy check |
| `assert-error` | `(assert-error (expr))` | Error check (expr must be quoted) |
| `assert-contains` | `(assert-contains string substring)` | Substring check |

All assertions print a message on failure and call `(exit 1)`.

## Files

| File | Lines | Content |
|------|-------|---------|
| `mod.rs` | ~140 | Test harness that runs Elle scripts |
| (individual test files) | ~50-200 each | Feature-specific Elle scripts |

## Invariants

1. **Scripts are self-contained.** Each script imports `examples/assertions.lisp` and runs independently.

2. **Scripts exit with code 0 on success, 1 on failure.** The test harness checks the exit code.

3. **Scripts use assertions, not print statements.** Assertions provide clear failure messages and exit codes.

4. **Scripts are deterministic.** Same script always produces same result. No randomness or timing dependencies.

5. **Scripts test language semantics, not implementation details.** They verify what the language does, not how it does it.

## When to add a test

- **New language feature**: Add a script that exercises the feature
- **Bug regression**: Add a script that reproduces the bug
- **Behavioral change**: Add a script that verifies the new behavior
- **Documentation example**: Add a script that demonstrates the feature

## Common pitfalls

- **Using print instead of assertions**: Use `assert-eq`, `assert-true`, etc. instead of `(print ...)` for clear failure messages.
- **Not importing assertions.lisp**: Every script must import `examples/assertions.lisp` to use assertion helpers.
- **Forgetting to register the test**: New scripts must be added to `tests/integration/elle_scripts.rs` with a test function.
- **Testing implementation details**: Test language semantics, not internal behavior (e.g., don't test bytecode structure).
- **Non-deterministic tests**: Don't use `time::now()` or other non-deterministic functions (except in dedicated time tests).

## Decision tree for test placement

Use this decision tree to decide where to place a new test:

1. **Does it test Elle language semantics?** → Elle script (`tests/elle/`)
2. **Does it test an invariant across all inputs?** → Property test (`tests/property/`)
3. **Does it test end-to-end pipeline behavior?** → Integration test (`tests/integration/`)
4. **Does it test a Rust API in isolation?** → Unit test (`tests/unittests/` or inline in `src/`)

See `docs/testing.md` for the full decision tree.
