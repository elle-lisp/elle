# tests/property

Property-based tests: invariants that must hold across all inputs.

## Responsibility

Test invariants that must hold for *all* valid inputs using proptest. Cover:
- Roundtrip fidelity (value encoding, reader parse/display, type conversions)
- Mathematical laws (commutativity, associativity, identity, inverse)
- Type discrimination (exactly one type predicate is true for any Value)
- Determinism (same source always produces same result)
- Signal inference soundness (pure expressions never inferred as yielding)
- Bug regression across input ranges (not just the single case that triggered it)

Does NOT:
- Test specific examples (that's integration tests)
- Test individual modules in isolation (that's unit tests)
- Test Elle scripts (that's `tests/elle/`)

## Key patterns

### Basic property test structure

```rust
use crate::common::eval_reuse_bare as eval_source;
use crate::common::proptest_cases;
use elle::Value;
use proptest::prelude::*;

proptest! {
    #![proptest_config(proptest_cases(200))]

    #[test]
    fn int_roundtrip(n in INT_MIN..=INT_MAX) {
        let result = eval_source(&format!("(+ {} 0)", n)).unwrap();
        prop_assert_eq!(result, Value::int(n));
    }
}
```

### Using strategies

```rust
use crate::common::proptest_cases;
use crate::common::eval_reuse_bare as eval_source;
use crate::property::strategies::arb_value;
use proptest::prelude::*;

proptest! {
    #![proptest_config(proptest_cases(200))]

    #[test]
    fn value_type_name(v in arb_value()) {
        let type_name = v.type_name();
        prop_assert!(!type_name.is_empty());
    }
}
```

### Testing with generated Elle source

```rust
proptest! {
    #![proptest_config(proptest_cases(200))]

    #[test]
    fn add_commutative(a in -1000i64..1000, b in -1000i64..1000) {
        let result1 = eval_source(&format!("(+ {} {})", a, b)).unwrap();
        let result2 = eval_source(&format!("(+ {} {})", b, a)).unwrap();
        prop_assert_eq!(result1, result2);
    }
}
```

## Case counts

Choose case counts based on the cost of each test case:

| Cost per case | Cases | Example |
|---------------|-------|---------|
| Cheap (no eval, pure Rust) | 1000 | Value encoding roundtrips, signal combine laws |
| Medium (single eval) | 200 | Arithmetic properties, reader roundtrips |
| Expensive (multiple evals or recursion) | 10-50 | Bug regression, determinism, complex programs |

Set via `#![proptest_config(proptest_cases(N))]` inside the `proptest!` block. The `proptest_cases` helper respects the `PROPTEST_CASES` environment variable — when set, it overrides the given default:

```bash
PROPTEST_CASES=8 cargo test    # fast smoke (development)
cargo test                     # use per-test defaults (CI thorough)
```

## Strategies

Public strategies in `strategies.rs`:

| Strategy | Generates | Use for |
|----------|-----------|---------|
| `arb_immediate()` | nil, empty_list, true, false, ints, floats, symbols | Tests that don't need heap values |
| `arb_value()` | Everything from `arb_immediate()` + strings, cons, arrays (depth 3) | General value testing |
| `arb_primitive_type()` | FFI primitive TypeDesc variants (I8..Ptr) | FFI type testing |
| `arb_type_desc(depth)` | Primitive + compound types (Struct, Array) | FFI compound type testing |
| `arb_flat_struct()` | StructDesc with 1-6 primitive fields | FFI struct testing |
| `arb_value_for_type(desc)` | Value matching a given TypeDesc | FFI roundtrip testing |
| `arb_typed_value()` | (TypeDesc, Value) pair where value matches type | FFI type/value pair testing |
| `arb_struct_and_values()` | (StructDesc, Value::array) pair | FFI struct marshalling testing |

Some property test files define local strategies for their domain:
- `reader.rs` defines `arb_source()` for generating valid Elle source code
- `strings.rs` defines `arb_unicode_string()`

### Writing new generators

- For Elle source code generation, build format strings with generated parameters: `format!("(+ {} {})", a, b)`. This is simpler and more maintainable than generating ASTs.
- For Value generation, use the strategies in `strategies.rs` or compose new ones from proptest primitives.
- Bound recursive generators with a depth parameter to prevent explosion.
- Weight leaf values higher than compound values in `prop_oneof!` to keep test cases manageable.
- Use `prop_filter` or `prop_assume!` to exclude invalid inputs rather than generating only valid ones (when the invalid space is small).

## Test organization

Tests are organized by domain in separate files:

| File | Coverage |
|------|----------|
| `strategies.rs` | Shared proptest strategies |
| `fibers.rs` | Fiber operations and properties |
| `nanboxing.rs` | Value encoding roundtrips |
| `reader.rs` | Reader parse/display roundtrips |
| `signals.rs` | Signal inference soundness |
| `strings.rs` | String operations and properties |
| `ffi.rs` | FFI type marshalling |
| `path.rs` | Path operations |

## Structure

Property test files follow a consistent structure:

1. Module-level comment explaining what invariants are tested
2. Any local helper functions (e.g., `infer_signal()` in `signals.rs`, `syntax_eq()` in `reader.rs`)
3. `proptest!` blocks grouped by invariant category, separated by section headers (`// =========================================================================`)
4. Non-property `#[test]` functions at the bottom for constant/edge cases that don't need generation

## Files

| File | Lines | Content |
|------|-------|---------|
| `mod.rs` | ~23 | Module declarations and includes |
| `strategies.rs` | ~195 | Shared proptest strategies |
| `fibers.rs` | ~100-200 | Fiber property tests |
| `nanboxing.rs` | ~100-200 | Value encoding property tests |
| `reader.rs` | ~100-200 | Reader property tests |
| `signals.rs` | ~100-200 | Signal inference property tests |
| `strings.rs` | ~100-200 | String property tests |
| `ffi.rs` | ~100-200 | FFI property tests |
| `path.rs` | ~100-200 | Path property tests |

## Invariants

1. **Tests are deterministic.** Same input always produces same output. No randomness or timing dependencies.

2. **Tests use cached VMs.** Property tests use `eval_reuse()` or `eval_reuse_bare()` to reuse a cached VM across cases, eliminating per-case bootstrap cost.

3. **Tests are independent.** Each case is independent. Globals are restored between cases.

4. **Shrinking works.** Proptest shrinks failing cases to minimal counterexamples. The shrunk output shows the minimal failing input.

5. **Case counts are configurable.** The `PROPTEST_CASES` env var overrides per-test defaults, allowing uniform control across the suite.

## When to add a test

- **New invariant**: Add a property test that verifies the invariant holds for all inputs
- **Bug regression**: Add a property test that generates inputs in the range that triggered the bug
- **Mathematical law**: Add a property test that verifies the law (e.g., commutativity, associativity)
- **Roundtrip fidelity**: Add a property test that verifies parse/display or encode/decode roundtrips

## Common pitfalls

- **Using `eval_source()` instead of `eval_reuse()`**: Creates a fresh VM for every case, which is slow. Use `eval_reuse()` or `eval_reuse_bare()` instead.
- **Not respecting `PROPTEST_CASES`**: Always use `proptest_cases(default)` to allow CI to control case counts.
- **Generating invalid inputs**: Use `prop_filter` or `prop_assume!` to exclude invalid inputs, or design the strategy to only generate valid inputs.
- **Not shrinking**: Proptest shrinks failing cases automatically. If a test fails, check the "Minimal failing input" line to understand the root cause.
- **Forgetting to register new files**: New test files must be added to `mod.rs` with `include!()`
- **Testing too many cases**: If a test takes >1 second per case, reduce the case count or optimize the test.
