# Property Tests

Property tests use `proptest` to generate random inputs and verify that invariants hold across a wide range of cases. They catch edge cases that manual tests might miss.

## How Property Tests Work

1. **Strategy**: Define how to generate random inputs
2. **Property**: Write a function that checks an invariant
3. **Shrinking**: If a test fails, proptest shrinks the input to find the minimal failing case
4. **Regression**: Failing cases are saved in `proptest-regressions/` for reproducibility

## Running Property Tests

```bash
# Run all property tests
cargo test --test property

# Run with verbose output
cargo test --test property -- --nocapture

# Run with custom number of cases
PROPTEST_CASES=10000 cargo test --test property

# Run a specific test
cargo test --test property test_name
```

## Test Strategies

Property tests use `proptest` strategies to generate random values. Common strategies are defined in [`strategies.rs`](strategies.rs):

- `any::<i64>()` — Random integers
- `any::<f64>()` — Random floats
- `"[a-z]+"` — Random strings matching regex
- `prop_oneof!` — Choose randomly from multiple strategies

## Regression Files

When a property test fails, proptest saves the failing input in `proptest-regressions/`. These files ensure the same case is tested on future runs:

```bash
# Delete regression files to reset
rm -rf proptest-regressions/

# Run tests again
cargo test --test property
```

## Example Property Test

```rust
proptest! {
    #[test]
    fn test_list_length(items in prop::collection::vec(any::<i64>(), 0..100)) {
        let code = format!("(length '{:?})", items);
        let result = eval_source(&code).unwrap();
        prop_assert_eq!(result.as_int(), Some(items.len() as i64));
    }
}
```

## See Also

- [AGENTS.md](AGENTS.md) - technical reference for LLM agents
- [`tests/`](../) - test suite overview
- [`tests/common/`](../common/) - shared test helpers
- [`strategies.rs`](strategies.rs) - Elle-specific test strategies
- [proptest documentation](https://docs.rs/proptest/)
