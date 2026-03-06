# Test Suite

Elle has a comprehensive test suite with over 3000 tests covering unit tests, integration tests, and property tests. Tests are organized by tier and can be run selectively.

## Test Organization

| Tier | Location | Purpose | Run with |
|------|----------|---------|----------|
| **Unit** | [`tests/unit/`](unit/) | Test individual modules | `cargo test --lib` |
| **Integration** | [`tests/integration/`](integration/) | Test full pipeline | `cargo test --test integration` |
| **Property** | [`tests/property/`](property/) | Test invariants with random inputs | `cargo test --test property` |
| **Elle Scripts** | [`tests/elle/`](elle/) | Test Elle code directly | `cargo test --test elle` |
| **Examples** | [`examples/`](../examples/) | Executable semantics | `cargo test --test '*'` |

## Running Tests

```bash
# Run all tests (takes ~30 minutes on 32-thread machine)
cargo test --workspace

# Run just the main crate tests
cargo test

# Run specific test tier
cargo test --lib                    # Unit tests only
cargo test --test integration       # Integration tests only
cargo test --test property          # Property tests only
cargo test --test elle              # Elle script tests only

# Run a specific test
cargo test test_name

# Run with output
cargo test -- --nocapture
```

## Test Helpers

Common test utilities are in [`tests/common/`](common/):

- `eval_source()` — Compile and execute Elle code
- `eval_file()` — Load and execute an Elle file
- `assert_eq_value()` — Compare values with helpful error messages
- `proptest_cases()` — Generate test cases for property tests

## Property Tests

Property tests use `proptest` to generate random inputs and verify invariants. Failing cases are recorded in `proptest-regressions/` for reproducibility.

```bash
# Run property tests with verbose output
cargo test --test property -- --nocapture

# Run with custom seed
PROPTEST_CASES=10000 cargo test --test property
```

## CI

The CI pipeline runs:

1. **Tests** on stable, beta, and nightly Rust
2. **Formatting** with `cargo fmt --check`
3. **Linting** with `cargo clippy -- -D warnings`
4. **Examples** with timeout
5. **Benchmarks** with regression reporting
6. **Documentation** generation

All must pass before merging.

## See Also

- [AGENTS.md](AGENTS.md) - technical reference for LLM agents
- [`tests/common/`](common/) - shared test helpers
- [`tests/integration/`](integration/) - integration test documentation
- [`tests/property/`](property/) - property test documentation
