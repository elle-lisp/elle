# Property tests

<!-- audited: 2026-11-25 -->

Proptest checks an invariant over generated inputs, and shrinks a failing input to the smallest that still fails.

## What belongs here

An invariant that must hold for every valid input:

- roundtrip fidelity: value encoding, reader parse and display, type
  conversions
- mathematical laws: commutativity, associativity, identity, inverse
- type discrimination: exactly one type predicate is true for any `Value`
- determinism: the same source always produces the same result
- signal algebra: combine is commutative, associative and idempotent
- value ordering: `Eq`, `Hash` and `Ord` agree
- a defect's regression across an input range, not only the case that found it

A test of one example is an integration test. A test of one module in
isolation is a unit test. A test written as an Elle program belongs in the
[language suite](../lang/overview.md) or the
[implementation suite](../impl/overview.md).

[The tests guide](../AGENTS.md) says how to register a file and how to pick a
case count.

## The files

| File | Coverage |
|------|----------|
| [strategies.rs](strategies.rs) | Shared proptest strategies |
| [nanboxing.rs](nanboxing.rs), [nanboxing/](nanboxing/) | Value encoding roundtrips |
| [reader.rs](reader.rs) | Reader parse and display roundtrips |
| [signals.rs](signals.rs) | Signal combine laws and predicates |
| [ordering.rs](ordering.rs) | `Eq`, `Hash` and `Ord` consistency for `Value` |
| [quoting.rs](quoting.rs) | Either spelling of a quoting form means one thing |
| [ffi.rs](ffi.rs), [ffi/](ffi/) | FFI type marshalling |

Each public strategy in [strategies.rs](strategies.rs) says in its docstring
what it generates. A file that needs a strategy for its own domain defines it
locally, as [reader.rs](reader.rs) does with `arb_source()`, which generates
valid Elle source.

## Writing a test

Evaluate source through `eval_reuse_bare`, or through `eval_reuse` when the
test needs the stdlib. Each caches one core per thread and resets the fiber
between cases, so a case pays no bootstrap. The result stays valid after the
call, so `prop_assert!` works at the level of the test function.

```rust
use crate::common::eval_reuse_bare as eval_source;
use crate::common::proptest_cases;
use elle::Value;
use proptest::prelude::*;

proptest! {
    #![proptest_config(proptest_cases(200))]

    #[test]
    fn add_commutative(a in -1000i64..1000, b in -1000i64..1000) {
        let ab = eval_source(&format!("(+ {} {})", a, b)).unwrap();
        let ba = eval_source(&format!("(+ {} {})", b, a)).unwrap();
        prop_assert_eq!(ab, ba);
    }
}
```

- Generate Elle source with a format string over generated parameters, such
  as `format!("(+ {} {})", a, b)`. Do not generate a syntax tree.
- Give a recursive generator a depth parameter, so that a case stays small.
- In `prop_oneof!`, weight leaf values above compound values.
- Exclude a small invalid space with `prop_filter` or `prop_assume!`.
- Use `proptest_cases`, never a config of your own. It keeps the case count
  that CI sets, and it writes a failing case to
  [tests/proptest-regressions](../proptest-regressions), which every later
  run tries first.
- Keep a case under one second. Lower the case count for an expensive case.

A property file opens with a comment that names the invariants it tests. Local
helpers follow, then the `proptest!` blocks, grouped by invariant. A plain
`#[test]` for a constant or an edge case goes at the bottom.

## Running

[The suite table](../README.md) holds the command that runs every property
test.

| Command | What it does |
|---------|--------------|
| `cargo test --test lib property::reader::` | One file's tests |
| `cargo test --test lib property:: -- --nocapture` | Every property test, with its output |

When a test fails, read the minimal failing input that proptest prints after
it shrinks. That input names the cause more directly than the first failure.
