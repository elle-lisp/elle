# Values

Every Elle value is a 16-byte tagged union: an 8-byte tag and an 8-byte
payload.

## Layout

```text
struct Value {
    tag: u64,      # type discriminant
    payload: u64,  # type-specific data
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
`HeapObject` lives in a bump-arena page owned by the fiber's `FiberHeap`
(or the parent's `SharedAllocator` for yielding fibers). `Value` is `Copy` —
it is just a tag + pointer, not a reference-counted handle.

```text
Tag                  HeapObject variant
──────────────────────────────────────────────────────────
TAG_STRING (10)      LString { s: InlineSlice<u8>, traits }
TAG_STRING_MUT (11)  LStringMut { data: Rc<RefCell<Vec<u8>>>, traits }
TAG_ARRAY (12)       LArray { elements: InlineSlice<Value>, traits }
TAG_ARRAY_MUT (13)   LArrayMut { data: Rc<RefCell<Vec<Value>>>, traits }
TAG_STRUCT (14)      LStruct { data: Vec<(TableKey, Value)>, traits }
TAG_STRUCT_MUT (15)  LStructMut { data: Rc<RefCell<BTreeMap<TableKey, Value>>>, traits }
TAG_CONS (16)        Pair { first: Value, rest: Value, traits }
TAG_CLOSURE (17)     Closure { closure: Closure, traits }
TAG_NATIVE_FN (18)   NativeFn (no traits field)
TAG_BYTES (19)       LBytes { data: InlineSlice<u8>, traits }
TAG_BYTES_MUT (20)   LBytesMut { data: Rc<RefCell<Vec<u8>>>, traits }
TAG_SET (21)         LSet { data: InlineSlice<Value>, traits }
TAG_SET_MUT (22)     LSetMut { data: Rc<RefCell<BTreeSet<Value>>>, traits }
TAG_FIBER (23)       Fiber { handle: FiberHandle, traits }
TAG_LBOX (24)        LBox { cell: Rc<RefCell<Value>>, traits }
TAG_PARAMETER (25)   Parameter { id: u32, default: Value, traits }
TAG_SYNTAX (26)      Syntax { syntax: Rc<Syntax>, traits }
```

Additional heap types not shown above: `CaptureCell`, `Float` (heap NaN),
`LibHandle`, `ThreadHandle`, `FFISignature`, `FFIType`, `ManagedPointer`,
`External`. See `src/value/heap.rs` for the complete list.

### Heap allocation

`HeapObject` is a Rust enum — a fixed-size tagged union. All variants
occupy the same number of bytes (the size of the largest variant). Each
`HeapObject` lives in a `BumpArena` page owned by the fiber's `SlabPool`.

The arena stores `HeapObject` shells. Many variants contain inner Rust
heap data — a `Vec<Value>` inside a mutable array, an `Rc<RefCell<...>>`
inside a closure, a `BTreeMap` inside a struct. The `needs_drop()` function
tracks which `HeapTag` variants have inner heap allocations that require
`Drop`. On scope exit or fiber death, destructors run on the `HeapObject`
(freeing inner data), and the arena position rewinds.

This two-level structure means:
- **Arena allocation is O(1)** — bump a byte offset within the current page
- **Pointer stability** — a `Value`'s payload pointer never moves; pages are
  `Box<[MaybeUninit<u8>]>` at fixed addresses
- **Batch deallocation** — fiber death runs all destructors then clears the
  arena (keeps one page for reuse)
- **Scope reclamation** — `RegionExit` runs destructors and rewinds the arena
  to the scope-entry position (gated by escape analysis)

### Immutable types use InlineSlice

Immutable collections (arrays, strings, bytes, sets) store their data inline
in the bump arena via `InlineSlice<T>` — a fat pointer to arena-allocated
bytes. This avoids inner `Vec` or `Box<str>` allocations for the common case.
Mutable types use `Rc<RefCell<...>>` for cross-fiber live-update semantics.

### Trait tables

Every user-facing heap variant (19 types) carries a `traits: Value` field
initialized to `NIL`. Only an immutable struct (`LStruct`) may be stored here.
The field is invisible to equality, ordering, and hashing.

## Closures

A `Closure` stores:
- Pointer to compiled function (bytecode or JIT code)
- Captured values array
- Arity descriptor
- Optional docstring
- Signal profile
- Location map (bytecode offset → source location)
- Optional syntax object (for `eval` reconstruction)

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
src/value/repr/           Value struct, tag constants, constructors, accessors
src/value/types.rs         Arity, SymbolId, NativeFn, TableKey
src/value/heap.rs          HeapObject, HeapTag, Pair, ExternalObject
src/value/closure.rs       Closure struct
src/value/fiberheap/       FiberHeap, SlabPool, BumpArena, routing
src/value/shared_alloc.rs  SharedAllocator for inter-fiber exchange
src/value/arena.rs         alloc/deref, ArenaMark, ArenaGuard
src/value/inline_slice.rs  InlineSlice<T> for inline arena data
src/value/allocator.rs     ElleAllocator trait, AllocatorBox
```

---

## See also

- [impl/vm.md](vm.md) — VM that operates on Values
- [types.md](../types.md) — user-facing type system
- [memory.md](../memory.md) — memory model, reclamation, and leak-free idioms
