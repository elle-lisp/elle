# Exception System Design

## Status

Complete. All four phases implemented.

## Problem

Elle has three error propagation channels that were never designed to work
together:

1. **`Err(String)`** — VM instruction handlers (`handle_car`, `handle_add`,
   etc.) return `Result<(), String>`. Propagated via `?`. Cannot be caught by
   Elle code. Used for both VM bugs (stack underflow) and data errors (type
   mismatch on `car`).

2. **`Err(LError)`** — Primitives (`NativeFn`, `VmAwareFn`) return
   `Result<Value, LError>`. The `Call` handler translates some `LError` variants
   to `Condition` objects. `TailCall` does not — it converts to `Err(String)`
   via `.description()`.

3. **`current_exception`** (`Option<Rc<Condition>>`) — The Elle-level exception
   channel. Set directly by `handle_div_int`, `handle_load_global`, and
   `prim_div_vm`. Checked at the bottom of every instruction dispatch. Caught by
   `handler-case`/`handler-bind`.

These don't compose. `(handler-case (+ 1 "a") (error e 99))` — the `+`
produces `Err("Expected integer")` through channel 1. The `?` operator
propagates it past the handler-case machinery. The handler never fires.

Only division-by-zero and undefined-variable were special-cased to use
channel 3.

## Design Principles

**Two failure modes exist and must remain distinct:**

- **VM bugs** (stack underflow, bad bytecode, corrupted state) — the compiler
  emitted bad code or the VM has a defect. Elle code must not catch them. They
  stay as `Err(String)` in the Rust channel. Uncatchable.

- **Runtime errors** (type mismatch, arity error, division by zero, undefined
  variable) — program behavior on bad data. Elle code should catch and handle
  them via `handler-case`.

**Primitives are already coupled to the runtime.** They construct `Value::int()`,
`Value::cons()`, `Value::string()`. Asking them to construct `Condition` objects
for errors is no different in kind.

## Considered Alternatives

### A: Everything becomes an exception

All errors from all sources flow through `current_exception`.

Rejected. Conflates VM bugs with program behavior.

### B: Only explicit raises become exceptions

Keep `Err(LError)` for primitives. Add explicit `current_exception` setup at
each instruction handler site.

Rejected. Primitives are `fn(&[Value]) -> LResult<Value>` — they don't have
access to the VM, so they can't set `current_exception`. Either every fallible
primitive becomes a `VmAwareFn` (massive API change), or primitive errors remain
uncatchable.

### C: Convert at the Call boundary

Primitives keep returning `Err(LError)`. The `Call` handler converts to
`Condition` and sets `current_exception`.

Rejected for three reasons:

1. **Semantic inconsistency.** `(+ 1 "a")` compiled as `Add` produces an
   uncatchable Rust error. `(apply + (list 1 "a"))` compiled as `Call` produces
   a catchable exception. Same operation, different catchability.

2. **Translation seam.** A mapping from every `ErrorKind` variant to condition
   fields must be maintained at the `Call` boundary. This seam rots.

3. **Inconsistent state window.** After conversion, `Call` pushes NIL and
   returns `Ok(())`. One instruction of limbo where the operation failed but
   execution continues with a garbage value on the stack.

## Chosen Design

### `NativeFn`: `Result<Value, Condition>`

Primitives return `Condition` directly as their error type. The `Call` and
`TailCall` handlers receive it and set `current_exception`. No translation, no
inconsistency, no gap.

```rust
// Call handler for NativeFn
Err(cond) => {
    self.current_exception = Some(Rc::new(cond));
    Value::NIL  // pushed to keep stack consistent
}
```

### `VmAwareFn`: keeps `Result<Value, String>`, sets exceptions directly

`VmAwareFn` has `&mut VM`. For user-facing errors, it sets
`vm.current_exception` directly and returns `Ok(Value::NIL)`. For VM bugs
(e.g., bytecode execution failure inside `coroutine-resume`), it returns
`Err(String)`. The Call handler propagates `Err(String)` as a VM bug.

This is the pattern `prim_div_vm` already uses. No signature change needed.

### Instruction handlers: two exit paths

Instruction handlers return `Result<(), String>`. The convention:

- **`Err(String)`** — VM bug. Stack underflow, bad bytecode, corrupted state.
  Propagated via `?`. Uncatchable.

- **`Ok(())` with `current_exception` set** — Runtime error on bad data. The
  handler constructs a `Condition`, sets `vm.current_exception`, pushes
  `Value::NIL` to keep the stack consistent, and returns `Ok(())`. The
  interrupt mechanism at the bottom of the instruction loop handles it.

This is the existing `handle_div_int` / `handle_load_global` pattern, extended
to all instruction-level data errors (`Add`, `Sub`, `Mul`, `Car`, `Cdr`,
`VectorRef`, `VectorSet`).

### VM bugs remain as `Err(String)`

Stack underflow, instruction pointer out of bounds, "cannot call non-function",
bytecode corruption — these stay in the Rust error channel. They are not
`Condition` objects. They cannot be caught by `handler-case`.

## Condition Struct

### Message is mandatory

Every `Condition` carries a human-readable message. An error without a message
is a defect in the error producer.

```rust
pub struct Condition {
    pub exception_id: u32,
    pub message: String,
    pub fields: HashMap<u32, Value>,
    pub backtrace: Option<String>,
    pub location: Option<SourceLoc>,
}
```

`message` is a struct field, separate from `fields`. The `fields` map carries
structured data for programmatic introspection (expected type, got type,
dividend, divisor, etc.). Field semantics are per-exception-type.

### Named constructors are the only public API

`Condition::new(id)` is private. All construction goes through named
constructors that enforce the message requirement:

```rust
Condition::type_error(msg)          // ID 3
Condition::division_by_zero(msg)    // ID 4
Condition::undefined_variable(msg)  // ID 5
Condition::arity_error(msg)         // ID 6
Condition::generic(msg)             // ID 0
Condition::error(msg)               // ID 2
```

Primitives never see a numeric ID. If the hierarchy changes, the constructors
change in one place.

### Exception hierarchy is data

The parent-child relationships live in `condition.rs` as a data table, not a
match statement scattered across modules. Both `Condition::is_instance_of` and
the VM's `MatchException` instruction use the same source of truth.

```
condition (1)
├── error (2)
│   ├── type-error (3)
│   ├── division-by-zero (4)
│   ├── undefined-variable (5)
│   └── arity-error (6)
├── warning (7)
│   └── style-warning (8)
└── generic (0) — legacy, treated as child of condition
```

### Structured fields

Named constructors can attach structured data via builder methods:

```rust
Condition::type_error("car: expected pair, got integer")
    .with_field(FIELD_EXPECTED, Value::string("pair"))
    .with_field(FIELD_GOT, Value::string("integer"))

Condition::division_by_zero("division by zero")
    .with_field(FIELD_DIVIDEND, Value::int(42))
    .with_field(FIELD_DIVISOR, Value::int(0))
```

Field IDs are constants on `Condition`. Structured fields are optional — the
message is always present and sufficient for display.

## Migration Plan

### Phase 1: Condition struct rework (current)

1. Add `message: String` as a struct field on `Condition`.
2. Make `Condition::new` private (`pub(crate)` during transition).
3. Add named constructors: `type_error`, `division_by_zero`,
   `undefined_variable`, `arity_error`, `generic`, `error`.
4. Move exception hierarchy (`exception_parent`, `is_exception_subclass`) from
   `vm/core.rs` to `value/condition.rs`. Single source of truth.
5. Remove `FIELD_MESSAGE` constant — message is no longer in the fields map.
6. Update all `Condition::new(N)` call sites to use named constructors.
7. Update tests.

### Phase 2: NativeFn signature change

1. Change `NativeFn` from `fn(&[Value]) -> Result<Value, LError>` to
   `fn(&[Value]) -> Result<Value, Condition>`.
2. Migrate primitives mechanically: `LError::type_mismatch(...)` →
   `Condition::type_error(...)`. The compiler catches every site.
3. In the `Call` handler: `Err(cond)` → set `current_exception`, push NIL.
   Remove the `ErrorKind` → exception ID translation.
4. In the `TailCall` handler: same treatment. `Err(cond)` → set
   `current_exception`, return `Ok(VmResult::Done(Value::NIL))`. **TailCall
   must not convert to `Err(String)` — that bypasses all handlers.**

### Phase 3: Instruction handler migration

1. In instruction handlers (`Add`, `Sub`, `Mul`, `Car`, `Cdr`, `VectorRef`,
   `VectorSet`): on data error, construct `Condition` via named constructor,
   set `current_exception`, push NIL, return `Ok(())`.
2. Stack underflow and bytecode errors remain as `Err(String)`.
3. Top-level `execute()` checks `current_exception` after execution. If set,
   formats using the condition's message and returns `Err(String)`.

### Phase 4: Cleanup

1. Remove `ErrorKind` → exception ID translation from `Call` handler.
2. `LError` remains for VM-internal use. It is never exposed to Elle code.
3. Update tests that checked for specific `Err(String)` messages.

## What This Does NOT Change

- The interrupt mechanism (check `current_exception` at bottom of instruction
  loop, jump to handler offset).
- `PushHandler`/`PopHandler`/`CheckException`/`MatchException`/`ClearException`/
  `ReraiseException` bytecode instructions.
- Handler isolation (save/restore `exception_handlers` across call boundaries).
- The exception hierarchy (condition → error → type-error, etc.).
- The `handler-case` / `handler-bind` surface syntax.
- `NativeFn` remains a bare function pointer (`fn`), not a trait object.

## Resolved Questions

- **Message is mandatory.** `String`, not `Option<String>`. An error without a
  message is a defect in the error producer.

- **Structured fields coexist with the message.** `fields: HashMap<u32, Value>`
  carries structured data (expected type, dividend, etc.). The message is for
  humans; the fields are for programs. Field semantics are per-exception-type.

- **FFI errors go through the same path.** FFI is just another primitive from
  Elle's perspective.

- **`VmAwareFn` keeps its current return type.** It has `&mut VM` and sets
  `current_exception` directly for user-facing errors. `Err(String)` is
  reserved for VM bugs.

- **TailCall is in scope.** It's a separate code path from Call and must
  convert `Err(Condition)` to `current_exception`, not to `Err(String)`.
