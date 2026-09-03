# value

Runtime value representation using a tagged union.

## Responsibility

- Define the `Value` type (16-byte tagged-union representation)
- Provide heap-allocated types (Closure, Fiber, Pair, etc.)
- Handle value display and thread-safe transfer

## Submodules

| Module | Purpose |
|--------|---------|
| `repr/mod.rs` | Tagged-union `Value` type, tag encoding |
| `repr/constructors.rs` | Value construction methods |
| `repr/accessors.rs` | Value field access and type checking |
| `repr/traits.rs` | `Display`, `Debug`, `Clone` implementations |
| `types.rs` | `Arity`, `SymbolId`, `NativeFn`, `TableKey`, sorted-struct helpers |
| `closure.rs` | `Closure` (template + env + squelch mask) and the `TemplateRef` seam; submodules `proto` (the compile-time `TemplateProto` blueprint), `payload` (`CodePayload`, the region-inline code data), `header` (`ClosureTemplate`), `cache` (the heap's payload cache). docs/impl/region/template.md owns the argument |
| `fiber.rs` | `Fiber`, `FiberHandle`, `WeakFiberHandle`, `SuspendedFrame`, `Frame`, `FiberStatus`; re-exports `SignalBits` (from `fiber/signalbits.rs`) and the `SIG_*` constants (from `crate::signals`) |
| `fiber/dues.rs` | `ActivationDues` — what one activation owes the region system when it ends: its owner node and the releases it took over from frame-replacing tail calls, carried as one record so a park moves both or neither (docs/impl/region/owner.md § "A deferred tail-call release has the node's life") |
| `fiber/delivery.rs` | `Delivery` — the delivery ledger: how the current park's delivery references are funded, with a method-only surface (docs/impl/region/owner.md § "A park names its funding in the delivery ledger") |
| `error.rs` | `rich_error!` macro plus `error_val_in()`, `error_val_extra_in()`, `match_fail_error_in()`, and `format_error()` for region-coherent error structs (docs/impl/region/errors.md) |
| `ffi.rs` | `LibHandle` for C interop |
| `fiberheap/` | `FiberHeap` over a `RegionStore` (physical region allocator; each region owns its pages via a `PagePool`) plus a custom-allocator stack and object-limit tracking. Submodules: `regionstore`, `regionpool`, `pagepool`, `freelog`. One heap per VM, shared by all of that VM's fibers. |
| `arena.rs` | Heap-explicit allocation funnel over `FiberHeap`: every entry point takes `heap: &mut FiberHeap` — `alloc`, `alloc_in_region`, `deref`, `region_of`, region RC (`incref_region`/`decref_region`), and the tracked mutable-store funnels (`push_with_incref`, `struct_put_with_rebind`, `capture_store_with_rebind`, …). |
| `heap.rs` | `HeapObject` enum, `HeapTag`, `Pair`, `ThreadHandle`, `LSet`, `LSetMut` (re-exports `Closure`, `Arity`, `NativeFn`, `TableKey`) |
| `send/` | `SendValue`/`SendBundle` wrappers for thread-safe transfer |
| `display.rs` | `Display` implementation for values |
| `keyword.rs` | Keyword identity (name hash) and the static keyword vocabulary for display |

## Key types

| Type | Location | Purpose |
|------|----------|---------|
| `Value` | `repr/mod.rs` | 16-byte tagged-union value (Copy) |
| `Closure` | `closure.rs` | `TemplateRef` + env (`RegionSlice<Value>`) + `squelch_mask`. Per-function code lives on the region-resident `ClosureTemplate` behind that seam. |
| `Fiber` | `fiber.rs` | Independent execution context with stack, frames, signal mask |
| `FiberHandle` | `fiber.rs` | `Rc<RefCell<Option<Fiber>>>` — take/put semantics for VM fiber swap |
| `WeakFiberHandle` | `fiber.rs` | Weak reference for parent back-pointers (avoids Rc cycles) |
| `FiberHeap` | `fiberheap/` | The VM's single heap over a `RegionStore` (region-based, RC-driven reclamation) plus a custom-allocator stack; reclamation is `FreeRegion(ρ)` when a region's RC reaches 0. Despite the name it is NOT per-fiber — all fibers share it and isolation is per-region (`value/fiber.rs`) |
| `Parameter` | `heap.rs` | Dynamic parameter with id and default value, looked up at runtime |
| `LSet` | `heap.rs` | Immutable set (`RegionSlice<Value>`, region-inline), no `RefCell` |
| `LSetMut` | `heap.rs` | Mutable set (`Rc<RefCell<BTreeSet<Value>>>`) (type name `:@set`) |
| `TableKey` | `types.rs` | Struct key. `Copy`; string and array keys hold a `Value`, so a key owns no Rust heap memory (docs/impl/values.md § "Struct keys") |
| `SendKey` | `send/mod.rs` | The owning key form `SendValue`'s maps are keyed on — the only key type that crosses a thread or reaches serde |

### Fiber fields for parent/child chain

Fibers maintain cached `Value`s alongside their handle references
so that `fiber/parent` and `fiber/child` return identity-preserving values
(i.e., `(identical? (fiber/parent f) (fiber/parent f))` is `true`):

| Field | Type | Purpose |
|-------|------|---------|
| `parent` | `Option<WeakFiberHandle>` | Weak back-pointer to parent fiber |
| `parent_value` | `Option<Value>` | Cached Value for parent |
| `child` | `Option<FiberHandle>` | Strong pointer to child fiber |
| `child_value` | `Option<Value>` | Cached Value for child |

These are set during the swap protocol in `vm/fiber.rs::with_child_fiber`.
| `SuspendedFrame` | `fiber.rs` | Bytecode/constants/env/IP/stack for resuming a suspended fiber |
| `Frame` | `fiber.rs` | Single call frame (closure + ip + base) |
| `FiberStatus` | `fiber.rs` | Fiber lifecycle: New, Alive, Paused, Dead, Error |
| `SignalBits` | `fiber/signalbits.rs` | Newtype over `u64` (re-exported from `fiber.rs`). The `SIG_*` constants are defined in `crate::signals`: SIG_OK(0), SIG_ERROR(1<<0), SIG_YIELD(1<<1), SIG_DEBUG(1<<2), SIG_RESUME(1<<3), SIG_FFI(1<<4), SIG_PROPAGATE(1<<5), SIG_HALT(1<<8) (among others) |
| `Arity` | `types.rs` | Function arity (Exact, AtLeast, Range) |
| `SymbolId` | `types.rs` | Symbol identity: the FNV-1a hash of the name |
| `SendValue` | `send/` | Thread-safe value wrapper |

## Invariants

0. **The mutable-store seam.** The raw `RefCell` accessors for the
   `Value`-bearing mutable containers (`as_array_mut_raw`,
   `as_struct_mut_raw`, `as_set_mut_raw`, `as_lbox_raw`,
   `as_capture_cell_raw` — conversions.rs) are `pub(in crate::value)`.
   Outside `value/`, reads go through the borrow-only `ReadCell` accessors
   (`as_array_mut` & co.) or copy-outs (`lbox_get`, `capture_cell_get`), and
   every store/remove goes through the tracked funnels in `arena.rs`
   (`push_with_incref`, `struct_put_with_rebind`,
   `capture_store_with_rebind`, …) — docs/impl/region/rules.md Rule 5, mutable store:
   an uncounted container store is a compile error. Membership-neutral
   mutation uses `with_array_mut_neutral`.

1. **`Value` is `Copy`.** All 16 bytes (tag + payload). Heap data is `Rc`.
   The `traits: Value` field on heap variants is also `Copy`.

2. **`traits` field is always NIL or a struct.** The `with-traits`
   primitive validates that the trait table is a struct — either an immutable
   `LStruct` or a mutable `LStructMut` (`src/primitives/traits.rs`
   `prim_with_traits`). `NIL` means "no traits attached". No other type is valid.

3. **`nil` ≠ empty list.** `Value::NIL` is falsy (absence). `Value::EMPTY_LIST`
     is truthy (empty list). Lists terminate with `EMPTY_LIST`, not `NIL`.

4. **Two box types exist as distinct heap variants.** `HeapObject::LBox`
       (user-created via `box`, explicit deref, not auto-unwrapped) and
       `HeapObject::CaptureCell` (compiler-created for mutable captures and
       mutated parameters, auto-unwrapped by `LoadUpvalue`, never user-visible).
       Both wrap `Rc<RefCell<Value>>`. Immutable captured locals do not need a
       cell — they are captured by value.

5. **Per-function code lives on the code object, not on `Closure`.** A `Closure`
     is just a `TemplateRef` + captured env (`RegionSlice<Value>`) + a
     `squelch_mask`. The code object is three things
     (docs/impl/region/template.md): a compile-time `TemplateProto` blueprint,
     a `CodePayload` holding every variable-length field inline in region pages
     (bytecode, constants, the sorted location table over an interned file
     table, the region tables, name and doc), and the two-word `ClosureTemplate`
     header a closure references. One payload serves every header made from one
     blueprint, so `MakeClosure` copies two words whatever the size of the
     function's body. Accessors are methods (`template.bytecode()`,
     `template.arity()`, …), not fields.

6. **Thread transfer uses `SendValue`.** `SendValue` wraps values for safe
     transfer between threads, cloning `Rc` contents as needed. Trait tables
     are serialized as part of the value — they are NOT stripped on cross-thread
     transfer.

7. **`SuspendedFrame` captures everything needed to resume.** A single type
     holds the bytecode, constants, env, IP, and operand stack for a suspended
     fiber. Signal suspension has an empty stack; yield suspension captures the
     stack.

8. **A stored struct key is interned into the struct's region.** `TableKey`
     is `Copy` and its string and array arms hold a `Value`, so a key built by
     `TableKey::from_value` merely borrows what it was built from — correct for
     a probe, wrong to store. Every site that puts a key into a struct calls
     `TableKey::intern_into` first, which copies a string or array payload into
     the destination region; a `Heap` key keeps aliasing, because its identity
     is the point. `SendKey` is the owning form for `send` and serde
     (docs/impl/values.md § "Struct keys").

## Value encoding

The tagged union uses a `(tag: u64, payload: u64)` pair:

- **Immediate**: nil, bool, int (full-range i64), symbol, keyword, float, native-fn (`TAG_NATIVE_FN`, payload = `prim_id`)
- **Heap pointer**: pair, array, struct, closure, fiber, lbox, parameter, syntax, set, etc.

### Syntax objects

`HeapObject::Syntax(Rc<Syntax>)` preserves scope sets through the Value
round-trip during macro expansion. Created by `Value::syntax()`, accessed
by `Value::as_syntax()`. Not sendable across threads (contains `Rc`).
`from_value()` unwraps syntax objects back to `Syntax`, preserving scopes.

**Note:** `Value` depends on `Syntax` (for `HeapObject::Syntax`) and
`Syntax` depends on `Value` (for `SyntaxKind::SyntaxLiteral`). This is
a circular dependency within the same crate, which Rust allows. Both
types are in `src/` — neither is in a separate crate.

### Binding objects

There is no `Binding` heap variant. Compile-time binding metadata lives in
`hir::arena::BindingArena`; `Binding` is a `u32` index, not a `Value`.

### Native functions

There is no `NativeFn` heap variant. Native functions are **immediate** values
(`TAG_NATIVE_FN`, payload = `prim_id`) — no heap cell. The `NativeFn` *type*
in `types.rs` is `&'static PrimitiveDef`, the static primitive definition the
`prim_id` resolves to.

### Set types

Two set types exist, following the immutable/mutable split:

- **`LSet { data: RegionSlice<Value> }`** — immutable set, stored region-inline,
  no `RefCell`. Created via `Value::set()` (takes a `BTreeSet<Value>` and freezes
  it into the slice). Accessed via `Value::as_set()`. Displays as `|1 2 3|`.
  `type_name()` returns `"set"`. Type keyword: `:set`.

- **`LSetMut { data: Rc<RefCell<BTreeSet<Value>>> }`** — mutable set.
  Created via `Value::set_mut()`. Accessed via `Value::as_set_mut()`. Displays
  as `@|1 2 3|`. `type_name()` returns `"@set"`. Type keyword: `:@set`.

Set membership uses structural equality (from `Value: Eq`). When a mutable value
is inserted into a set, it is frozen (converted to its immutable equivalent).
This prevents mutation from breaking set invariants (e.g., changing a key's hash
after insertion).

Predicates: `is_set()` and `is_set_mut()` for type checking.

Create values via methods: `Value::int(42)`, `Value::pair(head, tail)`,
`Value::closure(c)`, `Value::set(btree_set)`,
`Value::set_mut(btree_set)`. Don't construct enum variants directly.

## Trait table field

Every user-facing heap variant carries a `traits: Value` field (16 bytes).
Initialized to `Value::NIL` (meaning "no traits"). Only a struct — `LStruct` or
`LStructMut` — may be stored here; the `with-traits` primitive validates this at
call time.

The field is **invisible to structural equality, ordering, and hashing**:
`PartialEq`, `Ord`, and `Hash` on `Value` ignore the `traits` field.

### Variants that carry `traits` (20 types)

| Variant | Note |
|---------|------|
| `LArray` | immutable array |
| `LArrayMut` | mutable @array — RefCell data shared on `with-traits` |
| `LStruct` | immutable struct |
| `LStructMut` | mutable @struct — RefCell data shared on `with-traits` |
| `LString` | immutable string |
| `LStringMut` | mutable @string — RefCell data shared on `with-traits` |
| `LBytes` | immutable bytes |
| `LBytesMut` | mutable @bytes — RefCell data shared on `with-traits` |
| `LSet` | immutable set |
| `LSetMut` | mutable @set — RefCell data shared on `with-traits` |
| `Pair` | list pair |
| `Closure` | closure |
| `LBox` | mutable box — RefCell data shared on `with-traits` |
| `CaptureCell` | compiler-created mutable-capture cell |
| `Fiber` | fiber — FiberHandle (Rc) cloned on `with-traits` |
| `Syntax` | syntax object — Box<Syntax> cloned on `with-traits` |
| `ManagedPointer` | managed FFI pointer — Cell<Option<usize>> cloned on `with-traits` |
| `External` | opaque plugin object — Rc<dyn Any> cloned on `with-traits` |
| `Parameter` | dynamic parameter |
| `ThreadHandle` | thread handle — Arc<Mutex<...>> cloned on `with-traits` |

### Variants that do NOT carry `traits` (5 infrastructure types)

`Float`, `LibHandle`, `FFISignature`, `FFIType`, `ClosureTemplate`.

`HeapObject::traits()` returns `Value::NIL` for these. (Native-fns are
immediates, not heap objects, so they are not in this list.)

### SendValue behavior

Trait tables are **serialized** on `SendBundle::from_value` via a recursive
`from_value_inner` call, the same as any other `Value` field. Trait tables
are immutable structs whose values may be closures; closures survive
cross-thread transfer intact via the intern table in `SendBundle`. The
receiving thread reconstructs the trait table in `into_value_inner` and
stores the result in the new `traits` field of the heap object.

Each `SendValue` variant for the traitable types carries a
`traits: Box<SendValue>` field (or `traits: SendValue` if not recursive).
