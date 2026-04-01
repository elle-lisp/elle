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

Heap types store a raw pointer to a `HeapObject` in the payload. The
`HeapObject` lives in a slab slot owned by the fiber's `FiberHeap` (or
the parent's `SharedAllocator` for yielding fibers). `Value` is `Copy` —
it is just a tag + pointer, not a reference-counted handle.

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

### Heap allocation

`HeapObject` is a Rust enum — a fixed-size tagged union. All variants
occupy the same number of bytes (the size of the largest variant). Each
`HeapObject` lives in a slot in the fiber's `RootSlab`, a chunk-based
typed slab allocator with 256 slots per chunk.

The slab stores `HeapObject` shells. Many variants contain inner Rust
heap data — a `Vec<Value>` inside an array, a `BTreeMap` inside a struct,
an `Rc<Vec<u8>>` inside a closure's bytecode. The `needs_drop()` function
tracks which `HeapTag` variants have inner heap allocations that require
`Drop`. On scope exit or fiber death, destructors run on the `HeapObject`
(freeing inner data), then the slab slot returns to the free list.

This two-level structure means:
- **Slab allocation is O(1)** — reuse a free-list slot or bump a cursor
- **Pointer stability** — a `Value`'s payload pointer never moves
- **Batch deallocation** — fiber death drops all chunks without per-object traversal
- **Scope reclamation** — `RegionExit` returns slab slots to the free list
  for non-escaping allocations (gated by escape analysis)

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
src/value/fiberheap/     FiberHeap, RootSlab, routing
src/value/shared_alloc.rs SharedAllocator for inter-fiber exchange
src/value/arena.rs       alloc/deref, ArenaMark, ArenaGuard
```

---

## See also

- [impl/vm.md](vm.md) — VM that operates on Values
- [types.md](../types.md) — user-facing type system
- [memory.md](../memory.md) — memory model and ownership topology
