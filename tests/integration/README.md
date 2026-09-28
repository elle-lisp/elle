# Integration Tests

<!-- audited: 2026-09-28 -->

Integration tests verify the full compilation pipeline and runtime behavior. They test how different components work together, from source code to execution.

## What Belongs Here

Integration tests should verify:

- **Full pipeline**: Source → bytecode → execution
- **Cross-module interactions**: How different subsystems work together
- **Runtime behavior**: Actual execution results
- **Error handling**: Error messages and recovery

## What Belongs in the Elle Suites

Write an Elle file instead for:

- **Language semantics**: what a construct or a primitive does, in the
  [language suite](../lang/AGENTS.md)
- **Implementation behavior a program can measure**: a gauge, a leak rate, or
  a runtime mode, in the [implementation suite](../impl/AGENTS.md)

## Test Structure

Integration tests use `eval_source()` from [tests/common](../common/AGENTS.md) to compile and execute Elle code:

```rust
#[test]
fn a_let_bound_closure_is_a_closure() {
    eval_source("(let [x 10] (fn () (+ x 1)))", |result| {
        assert!(result.unwrap().is_closure());
    });
}
```

`eval_source` hands the result to a closure while the runtime's heap is still
alive, because a heap value dangles once the runtime is gone.

## Running Integration Tests

```bash
# Run all integration tests
cargo test --test lib integration::

# Run a specific test
cargo test --test lib integration::test_name

# Run with output
cargo test --test lib integration:: -- --nocapture
```

## Finding a test

[mod.rs](mod.rs) registers every file, and each file opens with a call-out
saying what it covers ([AGENTS.md](AGENTS.md) § Finding a test).

## See Also

- [AGENTS.md](AGENTS.md) - technical reference for LLM agents
- [tests](../AGENTS.md) - test suite overview
- [tests/common](../common/AGENTS.md) - shared test helpers
- [tests/lang](../lang/AGENTS.md) and [tests/impl](../impl/AGENTS.md) - the Elle suites
