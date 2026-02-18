# value

Runtime value representation using NaN-boxing.

## Responsibility

- Define the `Value` type (NaN-boxed 8-byte representation)
- Provide heap-allocated types (Closure, Coroutine, Cons, etc.)
- Handle value display and thread-safe transfer

## Submodules

| Module | Purpose |
|--------|---------|
| `repr.rs` | NaN-boxed `Value` type, tag encoding, immediate values |
| `types.rs` | `Arity`, `SymbolId`, `NativeFn`, `VmAwareFn`, `TableKey` |
| `closure.rs` | `Closure` struct with bytecode, env, and `location_map` |
| `coroutine.rs` | `Coroutine`, `CoroutineState` for suspendable computation |
| `continuation.rs` | `ContinuationData`, `ContinuationFrame` for first-class continuations |
| `condition.rs` | `Condition` for the condition/restart system |
| `ffi.rs` | `LibHandle`, `CHandle` for C interop |
| `heap.rs` | `HeapObject` enum, `Cons`, `ThreadHandle` |
| `send.rs` | `SendValue` wrapper for thread-safe transfer |
| `display.rs` | `Display` implementation for values |
| `intern.rs` | Symbol interning |

## Key types

| Type | Location | Purpose |
|------|----------|---------|
| `Value` | `repr.rs` | NaN-boxed 8-byte value (Copy) |
| `Closure` | `closure.rs` | Bytecode + env + arity + effect + location_map |
| `Coroutine` | `coroutine.rs` | Suspendable computation with continuation |
| `Arity` | `types.rs` | Function arity (Exact, AtLeast, Range) |
| `SymbolId` | `types.rs` | Interned symbol identifier |
| `SendValue` | `send.rs` | Thread-safe value wrapper |

## Invariants

1. **`Value` is `Copy`.** All 8 bytes fit in a register. Heap data is `Rc`.

2. **`nil` ≠ empty list.** `Value::NIL` is falsy (absence). `Value::EMPTY_LIST`
   is truthy (empty list). Lists terminate with `EMPTY_LIST`, not `NIL`.

3. **Two cell types exist.** `Cell` (user-created via `box`, explicit deref)
   and `LocalCell` (compiler-created for mutable captures, auto-unwrapped).
   Distinguished by a bool flag on `HeapObject::Cell`.

4. **`Closure` has `location_map`.** The `location_map: Rc<LocationMap>` field
   maps bytecode offsets to source locations for error reporting.

5. **Thread transfer uses `SendValue`.** `SendValue` wraps values for safe
   transfer between threads, cloning `Rc` contents as needed.

## Value encoding

NaN-boxing uses the NaN space of IEEE 754 doubles:

- **Immediate**: nil, bool, int (i48), symbol, keyword, float
- **Heap pointer**: cons, vector, table, closure, coroutine, cell, etc.

Create values via methods: `Value::int(42)`, `Value::cons(a, b)`,
`Value::closure(c)`. Don't construct enum variants directly.

## Files

| File | Lines | Content |
|------|-------|---------|
| `mod.rs` | ~50 | Re-exports |
| `repr.rs` | ~400 | NaN-boxed Value type |
| `types.rs` | ~150 | Arity, SymbolId, NativeFn, etc. |
| `closure.rs` | ~70 | Closure struct |
| `coroutine.rs` | ~100 | Coroutine, CoroutineState |
| `continuation.rs` | ~200 | ContinuationData, ContinuationFrame |
| `condition.rs` | ~50 | Condition type |
| `ffi.rs` | ~50 | LibHandle, CHandle |
| `heap.rs` | ~300 | HeapObject, Cons, ThreadHandle |
| `send.rs` | ~150 | SendValue for thread transfer |
| `display.rs` | ~100 | Display formatting |
| `intern.rs` | ~100 | Symbol interning |
