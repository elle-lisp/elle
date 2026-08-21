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
Tag                 Payload
──────────────────────────────────────
TAG_INT (0)         i64 value
TAG_FLOAT (1)       f64 bits
TAG_NIL (2)         unused
TAG_TRUE (3)        unused
TAG_FALSE (4)       unused
TAG_EMPTY_LIST (5)  unused
TAG_SYMBOL (6)      SymbolId (interned)
TAG_KEYWORD (7)     keyword hash (intern_keyword)
TAG_UNDEFINED (8)   unused (compiler sentinel; never user-visible)
TAG_CPOINTER (9)    raw C pointer address
TAG_NATIVE_FN (10)  prim_id (index into the primitive registry)
```

A **native-fn is an immediate**: its tag is below `TAG_HEAP_START` and its
payload is a `prim_id` — a dense `u32` index minted by `prim_id_of(def)` against
the global primitive registry (`PRIM_REGISTRY`, seeded from `ALL_TABLES` plus the
ffi tables). `prim_def(id)` is the inverse, resolving a native-fn back to its
`&'static PrimitiveDef`. A native-fn allocates nothing and belongs to no region
(it is not heap, so `region_of` is `None`), and two native-fns are equal iff
their `prim_id`s match. Because the identity is a position-stable index rather
than a pointer, the immediate rides the `Immediate` arm of `send` unchanged
across the process boundary, and `prim_table_snapshot()` materializes an
indexable table agreeing with these payloads for the WASM host's dispatch.

## Heap types

Heap types store a raw pointer to a `HeapObject` in the payload. The
`HeapObject` lives in a region page owned by the fiber's `FiberHeap`, whose
backing is a `RegionStore`. `Value` is `Copy` — it is just a tag + pointer, not
a reference-counted handle.

```text
Tag                   HeapObject variant
──────────────────────────────────────────────────────────
TAG_STRING_MUT (11)   LStringMut { data: Rc<RefCell<Vec<u8>>>, traits }
TAG_ARRAY (12)        LArray { elements: RegionSlice<Value>, traits }
TAG_ARRAY_MUT (13)    LArrayMut { data: Rc<RefCell<Vec<Value>>>, traits }
TAG_STRUCT (14)       LStruct { data: Vec<(TableKey, Value)>, traits }
TAG_STRUCT_MUT (15)   LStructMut { data: Rc<RefCell<BTreeMap<TableKey, Value>>>, traits }
TAG_CONS (16)         Pair { first: Value, rest: Value, traits }
TAG_CLOSURE (17)      Closure { closure: Closure, traits }
TAG_BYTES (18)        LBytes { data: RegionSlice<u8>, traits }
TAG_BYTES_MUT (19)    LBytesMut { data: Rc<RefCell<Vec<u8>>>, traits }
TAG_SET (20)          LSet { data: RegionSlice<Value>, traits }
TAG_SET_MUT (21)      LSetMut { data: Rc<RefCell<BTreeSet<Value>>>, traits }
TAG_LBOX (22)         LBox { cell: Rc<RefCell<Value>>, traits }
TAG_FIBER (23)        Fiber { handle: FiberHandle, traits }
TAG_SYNTAX (24)       Syntax { syntax: Box<Syntax>, traits }
TAG_STRING (26)       LString { s: RegionSlice<u8>, traits }
TAG_FFI_SIG (27)      FFISignature(Signature, CifCache)
TAG_FFI_TYPE (28)     FFIType(TypeDesc)
TAG_LIB_HANDLE (29)   LibHandle(u32)
TAG_MANAGED_PTR (30)  ManagedPointer { addr, traits }
TAG_EXTERNAL (31)     External { obj: ExternalObject, traits }
TAG_PARAMETER (32)    Parameter { id: u32, default: Value, traits }
TAG_THREAD (33)       ThreadHandle { handle, traits }
TAG_CAPTURE_CELL (34) CaptureCell { cell: Rc<RefCell<Value>>, traits }
TAG_CLOSURE_TEMPLATE (35) ClosureTemplate(ClosureTemplate)  # never user-visible
```

The tag numbers are not contiguous and are not in HeapObject declaration order
(`TAG_STRING` was swapped to 26 so `TAG_NATIVE_FN` could sit at 10, just below
`TAG_HEAP_START` = 11; nothing hardcodes the numeric values — all uses are by
name). `Float` (the heap-NaN variant) has no live tag: all floats are immediate
(`TAG_FLOAT`), and `HeapObject::Float` is never allocated. See
`src/value/heap.rs` for the authoritative list.

### Heap allocation

`HeapObject` is a Rust enum — a fixed-size tagged union. All variants
occupy the same number of bytes (the size of the largest variant). Each
`HeapObject` lives in a region page owned by the fiber's `FiberHeap`
(backed by a `RegionStore`).

The pages store `HeapObject` shells. Many variants contain inner Rust
heap data — a `Vec<Value>` inside a mutable array, an `Rc<RefCell<...>>`
inside a closure, a `BTreeMap` inside a struct. The `needs_drop()` function
(`src/value/fiberheap/mod.rs`) tracks which `HeapTag` variants have inner heap
allocations that require `Drop`. When a region is reclaimed or the fiber dies,
destructors run on those `HeapObject`s (freeing inner data) before the pages are
released.

This structure means:
- **Allocation is O(1)** — bump a byte offset within the current region page
- **Pointer stability** — a `Value`'s payload pointer never moves while its
  region is live; pages sit at fixed addresses
- **Batch deallocation** — fiber death runs all destructors then releases the
  pages
- **Region reclamation** — a region is a set of pages with a reference count
  minted per allocation; `DecrefRegion` decrements that RC, and when it hits 0
  the region's pages are freed and the contained destructors run (see
  `docs/regions.md`). This is RC-driven, not tied to any lexical scope.

### Immutable types use RegionSlice

Immutable collections (arrays, strings, bytes, sets) store their data inline
in their region's pages via `RegionSlice<T>` — a `(ptr, len)` view into
region-owned bytes, usually adjacent to the containing `HeapObject` header. This
avoids inner `Vec` or `Box<str>` allocations for the common case. Mutable types
use `Rc<RefCell<...>>` for cross-fiber live-update semantics.

### Trait tables

Every user-facing heap variant (19 types) carries a `traits: Value` field
initialized to `NIL`. The five infrastructure variants (`Float`, `LibHandle`,
`FFISignature`, `FFIType`, `ClosureTemplate`) do not. `with-traits` accepts only
a struct (`LStruct` or `LStructMut`) as the table to store here. The field is
invisible to equality, ordering, and hashing.

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
compare by reference identity; native-fns by `prim_id`.

## Hashing

`hash` is deterministic. Equal values hash identically, including
across mutability boundaries (`hash [1 2]` = `hash @[1 2]`).

## Files

```text
src/value/repr/            Value struct, tag constants, constructors, accessors
src/value/types.rs         Arity, SymbolId, NativeFn, TableKey
src/value/heap.rs          HeapObject, HeapTag, Pair, ExternalObject
src/value/closure.rs       Closure and ClosureTemplate structs
src/value/fiberheap/       FiberHeap, RegionStore, PagePool, routing
src/value/arena.rs         alloc/deref, region_of, region RC operations
src/value/region_slice.rs  RegionSlice<T> for inline region data
src/value/allocator.rs     ElleAllocator trait, AllocatorBox
```

---

## See also

- [impl/vm.md](vm.md) — VM that operates on Values
- [types.md](../types.md) — user-facing type system
- [regions.md](../regions.md) — region-based memory: per-region RC, `IncrefRegion`/`DecrefRegion`, merging
