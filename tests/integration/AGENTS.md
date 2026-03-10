# tests/integration

Full-pipeline integration tests: end-to-end behavior verification.

## Responsibility

Test end-to-end pipeline behavior by evaluating Elle source code through the full pipeline (Reader → Expander → Analyzer → Lowerer → Emitter → VM) and checking the result. Cover:
- Core language features (arithmetic, conditionals, lists, functions)
- Advanced features (closures, recursion, higher-order functions, match)
- Concurrency (fibers, coroutines, thread transfer)
- Effect enforcement (interprocedural effect tracking)
- Error reporting (error messages include correct source locations)
- Destructuring, blocks, splice, booleans, dispatch
- Prelude macros (defn, let*, when, unless, etc.)
- Lint and LSP features
- FFI integration
- JIT compilation
- REPL exit codes

Does NOT:
- Test individual modules in isolation (that's unit tests)
- Test invariants across random inputs (that's property tests)
- Test Elle scripts (that's `tests/elle/`)

## Key patterns

### Basic test structure

```rust
use crate::common::eval_source;
use elle::Value;

#[test]
fn test_my_feature() {
    assert_eq!(eval_source("(my-feature 42)").unwrap(), Value::int(42));
}
```

### Testing errors

```rust
#[test]
fn test_error_case() {
    let result = eval_source("(undefined-function)");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("undefined"));
}
```

### Testing with setup

```rust
use crate::common::setup;

#[test]
fn test_with_vm_access() {
    let (mut symbols, mut vm) = setup();
    // Direct VM access for advanced testing
    let result = eval_all("(+ 1 2)", &mut symbols, &mut vm, "<test>").unwrap();
    assert_eq!(result.last().unwrap(), &Value::int(3));
}
```

## Test organization

Tests are organized by feature area in separate files:

| File | Coverage |
|------|----------|
| `core.rs` | Basic arithmetic, conditionals, lists, functions |
| `advanced.rs` | Closures, recursion, higher-order functions |
| `concurrency.rs` | Fibers, coroutines, thread transfer |
| `error_reporting.rs` | Error messages with source locations |
| `repl_exit_codes.rs` | REPL exit code behavior |
| ~~`coroutines.rs`~~ | Migrated to `tests/elle/coroutines.lisp` |
| `lexical_scope.rs` | Lexical scoping and closures |
| `new_pipeline.rs` | New pipeline features |
| `new_pipeline_property.rs` | Property-based pipeline tests |
| `pipeline.rs` | Pipeline integration |
| `pipeline_property.rs` | Property-based pipeline tests |
| `pipeline_point.rs` | Specific pipeline points |
| `thread_transfer.rs` | Thread-safe value transfer |
| `effect_enforcement.rs` | Effect system enforcement |
| `effect_unsoundness.rs` | Effect system edge cases |
| `jit.rs` | JIT compilation |
| ~~`fibers.rs`~~ | Migrated to `tests/elle/fibers.lisp` |
| `time_property.rs` | Time-based property tests |
| `time_elapsed.rs` | Time measurement |
| `hygiene.rs` | Macro hygiene |
| ~~`destructuring.rs`~~ | Migrated to `tests/elle/destructuring.lisp` |
| `blocks.rs` | Block and break control flow |
| `primitives.rs` | Primitive function behavior |
| `ffi.rs` | FFI integration |
| `bracket_errors.rs` | Bracket syntax errors |
| `dispatch.rs` | Function dispatch |
| `lint.rs` | Linter behavior |
| `lsp.rs` | LSP features |
| `compliance.rs` | Language compliance |
| `buffer.rs` | Buffer operations |
| `splice.rs` | Splice syntax |
| `bytes.rs` | Bytes operations |
| `regex.rs` | Regular expressions |
| ~~`table_keys.rs`~~ | Migrated to `tests/elle/table-keys.lisp` |
| `glob.rs` | Glob patterns |
| `elle_scripts.rs` | Elle script tests |
| `environment.rs` | Environment variables |
| `escape.rs` | Escape analysis |
| `arena.rs` | Arena allocation |
| `allocator.rs` | Memory allocation |
| ~~`parameters.rs`~~ | Migrated to `tests/elle/parameters.lisp` |
| `ports.rs` | I/O ports |
| `fn_graph.rs` | Function call graphs |
| `fn_flow.rs` | Function control flow |

## Test helpers

All tests use `eval_source()` from `tests/common/mod.rs`:

```rust
use crate::common::eval_source;

let result = eval_source("(+ 1 2)").unwrap();
assert_eq!(result, Value::int(3));
```

For tests that need direct VM access:

```rust
use crate::common::setup;

let (mut symbols, mut vm) = setup();
// Use symbols and vm directly
```

## Naming conventions

- Test files: lowercase, hyphenated concepts joined with underscores (e.g., `closures_and_lambdas.rs`, `effect_enforcement.rs`)
- Test functions: `test_` prefix for example-based, descriptive name for property tests (e.g., `fn test_basic_arithmetic()`, `fn int_roundtrip(...)`)
- Property test names describe the invariant, not the implementation

## Registration

All test files must be registered in `mod.rs` using the `include!()` pattern:

```rust
mod myfile {
    include!("myfile.rs");
}
```

This is required because `tests/lib.rs` uses `include!()` to pull in the `mod.rs` file. Without registration, the test file will be ignored.

## Files

| File | Lines | Content |
|------|-------|---------|
| `mod.rs` | ~140 | Module declarations and includes |
| (individual test files) | ~100-500 each | Feature-specific tests |

## Invariants

1. **Tests are independent.** Each test creates a fresh VM (via `eval_source()`) or uses a cached VM with restored globals (via `eval_reuse()`). No cross-test contamination.

2. **Tests use the full pipeline.** `eval_source()` runs Reader → Expander → Analyzer → Lowerer → Emitter → VM. Tests verify end-to-end behavior, not individual components.

3. **Error tests check error messages.** When testing error cases, use `result.is_err()` and `result.unwrap_err().contains("substring")` to verify the error message.

4. **Tests are deterministic.** Same source always produces same result. No randomness or timing dependencies (except `time_property.rs` and `time_elapsed.rs`).

## When to add a test

- **New language feature**: Add to the appropriate feature file (e.g., `blocks.rs` for block/break)
- **Bug regression**: Add a test that reproduces the bug, then fix the bug
- **Error message improvement**: Add a test that checks the new error message
- **Performance regression**: Add to `time_property.rs` or `time_elapsed.rs`
- **Compliance issue**: Add to `compliance.rs`

## Common pitfalls

- **Using `eval_source()` in property tests**: Creates a fresh VM for every case, which is slow. Use `eval_reuse()` or `eval_reuse_bare()` instead.
- **Not checking error messages**: When testing error cases, verify the error message contains the expected substring
- **Assuming determinism**: Don't use `time::now()` or other non-deterministic functions in tests (except in `time_property.rs`)
- **Forgetting to register new files**: New test files must be added to `mod.rs` with `include!()`
