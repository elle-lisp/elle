# Values

Every Elle value is a 16-byte tagged union: an 8-byte tag and an 8-byte
payload.

## Layout

```text
struct Value {
    tag: u64,      // type discriminant
    payload: u64,  // type-specific data
}
```

## Immediates

Immediates store their data directly in the payload — no heap allocation.

```text
Tag            Payload
─────────────────────────────
TAG_INT (0)    i64 value
TAG_FLOAT (1)  f64 bits
TAG_BOOL (2)   0 or 1
TAG_NIL (3)    unused
TAG_SYMBOL (4) SymbolId (interned)
TAG_KEYWORD (5) SymbolId (interned)
TAG_EMPTY_LIST (6) unused
TAG_PTR (7)    raw pointer
```

## Heap types

Heap types store a pointer to a `HeapObject` in the payload.

```text
Tag              Pointed-to type
──────────────────────────────────
TAG_STRING (10)  String (immutable)
TAG_MSTRING (11) MutableString
TAG_ARRAY (12)   Vec<Value> (frozen)
TAG_MARRAY (13)  Vec<Value> (mutable)
TAG_STRUCT (14)  BTreeMap<SymbolId, Value> (frozen)
TAG_MSTRUCT (15) BTreeMap<SymbolId, Value> (mutable)
TAG_CONS (16)    (Value, Value) pair
TAG_CLOSURE (17) Closure
TAG_NATIVE_FN (18) fn pointer + arity
TAG_BYTES (19)   Vec<u8> (frozen)
TAG_MBYTES (20)  Vec<u8> (mutable)
TAG_SET (21)     BTreeSet<Value> (frozen)
TAG_MSET (22)    BTreeSet<Value> (mutable)
TAG_FIBER (23)   Fiber
TAG_BOX (24)     Box<Cell<Value>>
TAG_PARAMETER (25) DynamicParameter
TAG_SYNTAX (26)  Syntax object
```

## Closures

A `Closure` stores:
- Pointer to compiled function (bytecode or JIT code)
- Captured values array
- Arity descriptor
- Optional docstring
- Signal profile

## Arity

```text
Exact(n)       exactly n arguments
AtLeast(n)     n or more (variadic with &)
Range(n, m)    n required, up to m (with &opt)
```

## Equality

`=` performs structural equality. It crosses mutability boundaries
(an array and an @array with the same contents are equal). Closures
compare by reference identity.

## Hashing

`hash` is deterministic. Equal values hash identically, including
across mutability boundaries (`hash [1 2]` = `hash @[1 2]`).

## Files

```text
src/value/repr/mod.rs    Value struct, tag constants
src/value/types.rs       Type predicates and conversions
src/value/heap.rs        HeapObject, HeapTag
src/value/closure.rs     Closure struct
```

---

## See also

- [impl/vm.md](vm.md) — VM that operates on Values
- [types.md](../types.md) — user-facing type system
