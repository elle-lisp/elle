# Integration Tests

Integration tests verify the full compilation pipeline and runtime behavior. They test how different components work together, from source code to execution.

## What Belongs Here

Integration tests should verify:

- **Full pipeline**: Source → bytecode → execution
- **Cross-module interactions**: How different subsystems work together
- **Runtime behavior**: Actual execution results
- **Error handling**: Error messages and recovery

## What Belongs in Elle Tests

Use [`tests/elle/`](../elle/) instead for:

- **Language semantics**: Behavior of language constructs
- **Built-in functions**: Primitive operations
- **Simple features**: Single-feature tests

## Test Structure

Integration tests use `eval_source()` from [`tests/common/`](../common/) to compile and execute Elle code:

```rust
#[test]
fn test_closure_capture() {
    let result = eval_source("
        (let ((x 10))
          (fn () (+ x 1)))
    ").unwrap();
    
    // Verify the closure was created
    assert!(result.is_closure());
}
```

## Running Integration Tests

```bash
# Run all integration tests
cargo test --test integration

# Run a specific test
cargo test --test integration test_name

# Run with output
cargo test --test integration -- --nocapture
```

## Test Organization

Tests are organized by feature:

- **Closures**: Capture, mutation, cell boxing
- **Control flow**: If, while, match, break
- **Binding forms**: Let, def, var, destructuring
- **Macros**: Expansion, hygiene, quasiquote
- **Effects**: Inert, yields, polymorphic
- **Error handling**: Error propagation, recovery

## See Also

- [AGENTS.md](AGENTS.md) - technical reference for LLM agents
- [`tests/`](../) - test suite overview
- [`tests/common/`](../common/) - shared test helpers
- [`tests/elle/`](../elle/) - Elle script tests
