# Value representation

<!-- audited: 2026-10-24 -->

The tagged-union `Value`: its tags, its constructors, its accessors, and its
equality, hashing and order.

[values.md](../../../docs/impl/values.md) owns the encoding: every tag, what
each payload holds, and which tags are heap pointers. Each source here opens
with what it holds.

A heap value has no `Value::*` constructor. It is built through
[build.rs](../build.rs) or a primitive's `ctx`, which name the heap and the
region it is born in.

One traversal in [eq.rs](eq.rs) serves both equality relations, `identical?`
and `=`, so the two cannot disagree about structure. A read of a mutable
container takes a guard or copies the value out
([accessors/stores.rs](accessors/stores.rs)), and only the funnels in
[arena.rs](../arena.rs) write one ([mutable stores](../../../docs/impl/mutable.md)).

## Invariants

1. **`Value` is `Copy`.** All 16 bytes are the tag and the payload; heap data
   lives in regions.
2. **Trait tables are invisible to equality, hashing and order.** `PartialEq`,
   `Hash` and `Ord` never read a heap object's `traits` field.
3. **`nil` ≠ empty list.** `Value::NIL` is falsy; `Value::EMPTY_LIST` is
   truthy and ends every list.
4. **Floats are bit-exact.** `Value::float(f).as_float()` answers the same
   bits, NaN, infinity and -0.0 included.
5. **Integers are full-range i64.** The payload holds the whole value.
