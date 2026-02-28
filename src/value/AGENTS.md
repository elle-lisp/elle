# value

Runtime value representation using NaN-boxing.

## Responsibility

- Define the `Value` type (NaN-boxed 8-byte representation)
- Provide heap-allocated types (Closure, Fiber, Cons, etc.)
- Handle value display and thread-safe transfer

## Submodules

| Module | Purpose |
|--------|---------|
| `repr/mod.rs` | NaN-boxed `Value` type, tag encoding |
| `repr/constructors.rs` | Value construction methods |
| `repr/accessors.rs` | Value field access and type checking |
| `repr/traits.rs` | `Display`, `Debug`, `Clone` implementations |
| `repr/tests.rs` | NaN-boxing tests |
| `types.rs` | `Arity`, `SymbolId`, `NativeFn`, `TableKey` |
| `closure.rs` | `Closure` struct with bytecode, env, and `location_map` |
| `fiber.rs` | `Fiber`, `FiberHandle`, `WeakFiberHandle`, `SuspendedFrame`, `Frame`, `FiberStatus`, `SignalBits` |
| `error.rs` | `error_val()` and `format_error()` helpers for error tuples |
| `ffi.rs` | `LibHandle` for C interop |
| `heap.rs` | `HeapObject` enum, `Cons`, `ThreadHandle`, `BindingInner`, `BindingScope` |
| `send.rs` | `SendValue` wrapper for thread-safe transfer |
| `display.rs` | `Display` implementation for values |
| `intern.rs` | String interning (used by both strings and keywords) |

## Key types

| Type | Location | Purpose |
|------|----------|---------|
| `Value` | `repr/mod.rs` | NaN-boxed 8-byte value (Copy) |
| `Closure` | `closure.rs` | Bytecode + env + arity + effect + location_map |
| `Fiber` | `fiber.rs` | Independent execution context with stack, frames, signal mask |
| `FiberHandle` | `fiber.rs` | `Rc<RefCell<Option<Fiber>>>` — take/put semantics for VM fiber swap |
| `WeakFiberHandle` | `fiber.rs` | Weak reference for parent back-pointers (avoids Rc cycles) |

### Fiber fields for parent/child chain

Fibers maintain cached NaN-boxed `Value`s alongside their handle references
so that `fiber/parent` and `fiber/child` return identity-preserving values
(i.e., `(eq? (fiber/parent f) (fiber/parent f))` is `true`):

| Field | Type | Purpose |
|-------|------|---------|
| `parent` | `Option<WeakFiberHandle>` | Weak back-pointer to parent fiber |
| `parent_value` | `Option<Value>` | Cached NaN-boxed Value for parent |
| `child` | `Option<FiberHandle>` | Strong pointer to child fiber |
| `child_value` | `Option<Value>` | Cached NaN-boxed Value for child |

These are set during the swap protocol in `vm/fiber.rs::with_child_fiber`.
| `SuspendedFrame` | `fiber.rs` | Bytecode/constants/env/IP/stack for resuming a suspended fiber |
| `Frame` | `fiber.rs` | Single call frame (closure + ip + base) |
| `FiberStatus` | `fiber.rs` | Fiber lifecycle: New, Alive, Suspended, Dead, Error |
| `SignalBits` | `fiber.rs` | u32 bitmask: SIG_OK(0), SIG_ERROR(1), SIG_YIELD(2), SIG_DEBUG(4), SIG_RESUME(8), SIG_FFI(16), SIG_PROPAGATE(32), SIG_CANCEL(64), SIG_HALT(256) |
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

4. **`Closure` has `location_map` and `doc`.** The `location_map: Rc<LocationMap>`
    field maps bytecode offsets to source locations for error reporting. The
    `doc: Option<Value>` field carries the docstring extracted from the function
    body, threaded from HIR through LIR.

5. **Thread transfer uses `SendValue`.** `SendValue` wraps values for safe
    transfer between threads, cloning `Rc` contents as needed.

6. **`SuspendedFrame` replaces both `SavedContext` and `ContinuationFrame`.**
    A single type captures everything needed to resume: bytecode (`Rc<Vec<u8>>`),
    constants (`Rc<Vec<Value>>`), env (`Rc<Vec<Value>>`), IP, and operand stack.
    Signal suspension has an empty stack; yield suspension captures the stack.

## Value encoding

NaN-boxing uses the NaN space of IEEE 754 doubles:

- **Immediate**: nil, bool, int (i48), symbol, keyword, float
- **Heap pointer**: cons, array, table, closure, fiber, cell, syntax, binding, etc.

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

`HeapObject::Binding(RefCell<BindingInner>)` stores compile-time binding
metadata. Created by `Value::binding(name, scope)`, accessed by
`Value::as_binding()`. Never appears at runtime — the VM never sees this type.
The `BindingInner` is mutable during analysis (the analyzer discovers captures
and mutations after creating the binding) and read-only during lowering.

`BindingScope` enum: `Parameter`, `Local`, `Global`. Defined in `heap.rs`.

Create values via methods: `Value::int(42)`, `Value::cons(a, b)`,
`Value::closure(c)`, `Value::binding(name, scope)`. Don't construct enum
variants directly.

## Files

| File | Lines | Content |
|------|-------|---------|
| `mod.rs` | ~40 | Re-exports |
| `repr/mod.rs` | ~280 | NaN-boxed Value type, tag encoding |
| `repr/constructors.rs` | ~250 | Value construction methods |
| `repr/accessors.rs` | ~420 | Value field access and type checking |
| `repr/traits.rs` | ~150 | Display, Debug, Clone implementations |
| `repr/tests.rs` | ~100 | NaN-boxing tests |
| `types.rs` | ~150 | Arity, SymbolId, NativeFn, etc. |
| `closure.rs` | ~70 | Closure struct |
| `fiber.rs` | ~515 | Fiber, FiberHandle, WeakFiberHandle, SuspendedFrame, Frame, SignalBits |
| `error.rs` | ~50 | error_val() and format_error() helpers |
| `ffi.rs` | ~22 | LibHandle |
| `heap.rs` | ~320 | HeapObject, Cons, ThreadHandle, BindingInner, BindingScope |
| `send.rs` | ~150 | SendValue for thread transfer |
| `display.rs` | ~100 | Display formatting |
| `intern.rs` | ~100 | Symbol interning |
