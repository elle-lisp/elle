# Common Test Utilities

Shared helpers and utilities for all test tiers. These functions provide a consistent interface for compiling and executing Elle code in tests.

## Core Functions

| Function | Purpose |
|----------|---------|
| `eval_source(code)` | Compile and execute Elle code, return result or error |
| `eval_file(path)` | Load and execute an Elle file |
| `assert_eq_value(actual, expected)` | Compare values with helpful error messages |
| `proptest_cases()` | Get number of property test cases to generate |

## eval_source

Compiles and executes Elle code in a fresh VM:

```rust
let result = eval_source("(+ 1 2)")?;
assert_eq!(result.as_int(), Some(3));

// Errors are returned as Err
assert!(eval_source("(+ 1 \"string\")").is_err());
```

## Property Test Helpers

Property tests use `proptest` to generate random inputs. Control the number of cases:

```bash
# Run 1000 test cases (default)
cargo test --test property

# Run 10000 test cases
PROPTEST_CASES=10000 cargo test --test property

# Run 100 cases (quick check)
PROPTEST_CASES=100 cargo test --test property
```

Failing cases are recorded in `proptest-regressions/` for reproducibility. Delete these files to reset.

## Test Strategies

Property tests use `proptest` strategies to generate random values:

- `any::<i64>()` — Random integers
- `any::<f64>()` — Random floats
- `"[a-z]+"` — Random strings matching regex
- `prop_oneof!` — Choose randomly from multiple strategies

See [`strategies.rs`](../property/strategies.rs) for Elle-specific strategies.

## See Also

- [AGENTS.md](AGENTS.md) - technical reference for LLM agents
- [`tests/`](../) - test suite overview
- [`tests/integration/`](../integration/) - integration tests
- [`tests/property/`](../property/) - property tests
