# Adding a New Heap Type

<!-- audited: 2026-10-22 -->

A heap type is a new kind of runtime value stored behind a tagged-union
pointer.

[values.md](../impl/values.md) owns the representation. This recipe lists the
places a new variant has to reach, split by whether the compiler finds them.

## The variant and its tags

1. In [heap.rs](../../src/value/heap.rs), give `HeapTag` a variant at an
   unused number. Give `HeapObject` a variant whose discriminant is that tag:

   ```rust
   MyType { data: MyData, traits: Value } = HeapTag::MyType as u64,
   ```

   Do not leave the discriminant out. A variant without one takes the previous
   variant's plus one, and no compile error says so.
2. Keep the variant no larger than `Syntax`. Put a larger payload behind a
   `Box`, or every heap object grows to fit it.
3. Add a `TAG_*` constant for the `Value` tag in
   [repr/mod.rs](../../src/value/repr/mod.rs). When the new tag is the largest,
   raise the bound in `Value::malformed_reason` as well.
4. Carry `traits: Value`, initialized to `Value::NIL`, unless the type is
   infrastructure that no user sees. [values.md](../impl/values.md) lists the
   variants without it.

## What the compiler finds

These matches are exhaustive, so the build fails until each has an arm. Give
each one a decision, never a wildcard.

| File | Matches |
|------|---------|
| [objimpl.rs](../../src/value/heap/objimpl.rs) | `tag`, `traits`, `value_tag`, `type_name`, `Debug` |
| [dropsafety.rs](../../src/value/fiberheap/dropsafety.rs) | `needs_drop`, `holds_value_refs` |
| [introspect.rs](../../src/value/fiberheap/regionpool/introspect.rs) | the cross-region scan, `find_object_cross_refs` |
| [introspect/tests.rs](../../src/value/fiberheap/regionpool/introspect/tests.rs) | the scan pin's exemplar for each tag |
| [census.rs](../../src/value/fiberheap/census.rs) | the sealing class a dump gives each tag, and its `--trace=census` row |
| [ord.rs](../../src/value/repr/traits/ord.rs) | the type rank `Value`'s order uses |
| [ser.rs](../../src/value/send/ser.rs) | what `send` carries or refuses |
| [serializer.rs](../../src/primitives/json/serializer.rs) | what JSON writes or refuses |
| [traits.rs](../../src/primitives/traits.rs) | the `with-traits` copy |

## What the compiler does not find

- A constructor in [build.rs](../../src/value/build.rs), or in
  [build/mutable.rs](../../src/value/build/mutable.rs) for a mutable container,
  and a method on the primitive context in
  [ctx/build.rs](../../src/primitives/ctx/build.rs) when a primitive allocates
  one. Build a heap value there, never from the variant at
  a call site.
- A predicate in
  [predicates.rs](../../src/value/repr/accessors/predicates.rs), and an
  accessor in [conversions.rs](../../src/value/repr/accessors/conversions.rs).
- Display in [display.rs](../../src/value/display.rs). Equality in
  [eq.rs](../../src/value/repr/eq.rs) and hashing in
  [traits.rs](../../src/value/repr/traits.rs) end in a wildcard arm, so a new
  type compares and hashes as that arm says until it has an arm of its own.
- The receiving side of `send` in [de.rs](../../src/value/send/de.rs), for a
  type `send` carries.
- Two lists of every tag: `tag_from_u64` in
  [format/mod.rs](../../src/image/format/mod.rs), and `ALL_TAGS` in the scan
  pin.
- An image. The dumper writes only the variants the layout probe measures
  ([layout/heap.rs](../../src/image/layout/heap.rs)), so a new variant fails a
  dump until [sealing.md](../impl/image/sealing.md) admits it and the probe
  measures it.

Then give the type primitives ([primitives.md](primitives.md)) and a
type-check predicate in [types.rs](../../src/primitives/types.rs).

---

## See also

- [Cookbook index](index.md)
