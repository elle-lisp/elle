# Elle Script Tests

Tests written in Elle itself, executed by the test harness. These tests verify Elle semantics and language features using Elle code.

## Test Structure

Each `.lisp` file in this directory is a test. Tests use assertions from [`assertions.lisp`](assertions.lisp):

```lisp
(assert-eq (+ 1 2) 3)
(assert-true (> 5 3))
(assert-false (< 5 3))
(assert-error (+ 1 "string"))
```

## Running Elle Tests

```bash
# Run all Elle script tests
cargo test --test elle

# Run a specific test file
cargo test --test elle test_name

# Run with output
cargo test --test elle -- --nocapture
```

## Adding New Tests

1. Create a `.lisp` file in this directory
2. Use assertions from [`assertions.lisp`](assertions.lisp)
3. Exit with code 0 on success, non-zero on failure
4. Run `cargo test --test elle` to verify

Example test file:

```lisp
;; Test list operations
(assert-eq (length '(1 2 3)) 3)
(assert-eq (first '(a b c)) 'a)
(assert-eq (rest '(a b c)) '(b c))
(assert-true (empty? '()))
(assert-false (empty? '(1)))
```

## Assertion Functions

| Function | Purpose |
|----------|---------|
| `assert-eq` | Assert equality |
| `assert-true` | Assert value is truthy |
| `assert-false` | Assert value is falsy |
| `assert-error` | Assert expression raises error |
| `assert-match` | Assert value matches pattern |

## Differences from Examples

| Aspect | Elle Tests | Examples |
|--------|-----------|----------|
| **Purpose** | Verify language semantics | Document language features |
| **Assertions** | Yes — verify behavior | Yes — verify behavior |
| **Size** | Small, focused | Larger, more complex |
| **Output** | Test results | Demonstration output |

## See Also

- [AGENTS.md](AGENTS.md) - technical reference for LLM agents
- [`tests/`](../) - test suite overview
- [`assertions.lisp`](assertions.lisp) - assertion functions
- [`examples/`](../../examples/) - executable semantics documentation
