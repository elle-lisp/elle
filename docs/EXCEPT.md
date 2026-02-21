# Exception System Design

## Status

The exception system is transitioning from the old `handler-case`/`handler-bind`
model to a modern `try`/`catch`/`finally` model backed by fibers and signals.

**Current implementation** (pre-fiber):
- `handler-case` is fully implemented end-to-end
- `handler-bind`, `throw`, restarts, and `finally` are partially or not implemented
- The old `Condition` type and exception hierarchy exist but will be replaced

**Target implementation** (fiber-backed):
- `try`/`catch`/`finally` as the primary exception handling mechanism
- Exceptions are first-class values that can be caught, inspected, and resumed
- Recovery via `signal`/`resume` in the fiber model (no restart machinery)
- Proper cleanup via `finally` blocks
- Non-unwinding recovery patterns via fiber suspension and resumption

This document describes the **target state**. Implementation is ongoing.

## Problem

Elle has three error propagation channels that were never designed to work
together:

1. **`Err(String)`** — VM instruction handlers (`handle_car`, `handle_add`,
   etc.) return `Result<(), String>`. Propagated via `?`. Cannot be caught by
   Elle code. Used for both VM bugs (stack underflow) and data errors (type
   mismatch on `car`).

2. **`Err(LError)`** — Primitives (`NativeFn`, `VmAwareFn`) return
   `Result<Value, LError>`. The `Call` handler translates some `LError` variants
   to `Exception` objects. `TailCall` does not — it converts to `Err(String)`
   via `.description()`.

3. **`current_exception`** (`Option<Rc<Exception>>`) — The Elle-level exception
   channel. Set directly by `handle_div_int`, `handle_load_global`, and
   `prim_div_vm`. Checked at the bottom of every instruction dispatch. Caught by
   `try`/`catch` blocks.

These don't compose. `(try (+ 1 "a") (catch error e 99))` — the `+`
produces `Err("Expected integer")` through channel 1. The `?` operator
propagates it past the try/catch machinery. The handler never fires.

Only division-by-zero and undefined-variable were special-cased to use
channel 3.

## Design Principles

**Two failure modes exist and must remain distinct:**

- **VM bugs** (stack underflow, bad bytecode, corrupted state) — the compiler
  emitted bad code or the VM has a defect. Elle code must not catch them. They
  stay as `Err(String)` in the Rust channel. Uncatchable.

- **Runtime errors** (type mismatch, arity error, division by zero, undefined
  variable) — program behavior on bad data. Elle code should catch and handle
  them via `try`/`catch` blocks.

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
`Exception` and sets `current_exception`.

Rejected for three reasons:

1. **Semantic inconsistency.** `(+ 1 "a")` compiled as `Add` produces an
   uncatchable Rust error. `(apply + (list 1 "a"))` compiled as `Call` produces
   a catchable exception. Same operation, different catchability.

2. **Translation seam.** A mapping from every `ErrorKind` variant to exception
   fields must be maintained at the `Call` boundary. This seam rots.

3. **Inconsistent state window.** After conversion, `Call` pushes NIL and
   returns `Ok(())`. One instruction of limbo where the operation failed but
   execution continues with a garbage value on the stack.

## Chosen Design

### `NativeFn`: `Result<Value, Exception>`

Primitives return `Exception` directly as their error type. The `Call` and
`TailCall` handlers receive it and set `current_exception`. No translation, no
inconsistency, no gap.

```rust
// Call handler for NativeFn
Err(exc) => {
    self.current_exception = Some(Rc::new(exc));
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
  handler constructs an `Exception`, sets `vm.current_exception`, pushes
  `Value::NIL` to keep the stack consistent, and returns `Ok(())`. The
  interrupt mechanism at the bottom of the instruction loop handles it.

This is the existing `handle_div_int` / `handle_load_global` pattern, extended
to all instruction-level data errors (`Add`, `Sub`, `Mul`, `Car`, `Cdr`,
`VectorRef`, `VectorSet`).

### VM bugs remain as `Err(String)`

Stack underflow, instruction pointer out of bounds, "cannot call non-function",
bytecode corruption — these stay in the Rust error channel. They are not
`Exception` objects. They cannot be caught by `try`/`catch` blocks.

## Exception Struct

### Message is mandatory

Every `Exception` carries a human-readable message. An error without a message
is a defect in the error producer.

```rust
pub struct Exception {
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

`Exception::new(id)` is private. All construction goes through named
constructors that enforce the message requirement:

```rust
Exception::type_error(msg)          // ID 3
Exception::division_by_zero(msg)    // ID 4
Exception::undefined_variable(msg)  // ID 5
Exception::arity_error(msg)         // ID 6
Exception::generic(msg)             // ID 0
Exception::error(msg)               // ID 2
```

Primitives never see a numeric ID. If the hierarchy changes, the constructors
change in one place.

### Exception hierarchy is data

The parent-child relationships live in `exception.rs` as a data table, not a
match statement scattered across modules. Both `Exception::is_instance_of` and
the VM's `MatchException` instruction use the same source of truth.

```
exception (1)
├── error (2)
│   ├── type-error (3)
│   ├── division-by-zero (4)
│   ├── undefined-variable (5)
│   └── arity-error (6)
├── warning (7)
│   └── style-warning (8)
└── generic (0) — legacy, treated as child of exception
```

### Structured fields

Named constructors can attach structured data via builder methods:

```rust
Exception::type_error("car: expected pair, got integer")
    .with_field(FIELD_EXPECTED, Value::string("pair"))
    .with_field(FIELD_GOT, Value::string("integer"))

Exception::division_by_zero("division by zero")
    .with_field(FIELD_DIVIDEND, Value::int(42))
    .with_field(FIELD_DIVISOR, Value::int(0))
```

Field IDs are constants on `Exception`. Structured fields are optional — the
message is always present and sufficient for display.

## Implementation Plan

### Phase 1: Exception struct rework (current)

1. Add `message: String` as a struct field on `Exception`.
2. Make `Exception::new` private (`pub(crate)` during transition).
3. Add named constructors: `type_error`, `division_by_zero`,
   `undefined_variable`, `arity_error`, `generic`, `error`.
4. Move exception hierarchy (`exception_parent`, `is_exception_subclass`) from
   `vm/core.rs` to `value/exception.rs`. Single source of truth.
5. Remove `FIELD_MESSAGE` constant — message is no longer in the fields map.
6. Update all `Exception::new(N)` call sites to use named constructors.
7. Update tests.

### Phase 2: NativeFn signature change

1. Change `NativeFn` from `fn(&[Value]) -> Result<Value, LError>` to
   `fn(&[Value]) -> Result<Value, Exception>`.
2. Migrate primitives mechanically: `LError::type_mismatch(...)` →
   `Exception::type_error(...)`. The compiler catches every site.
3. In the `Call` handler: `Err(exc)` → set `current_exception`, push NIL.
   Remove the `ErrorKind` → exception ID translation.
4. In the `TailCall` handler: same treatment. `Err(exc)` → set
   `current_exception`, return `Ok(VmResult::Done(Value::NIL))`. **TailCall
   must not convert to `Err(String)` — that bypasses all handlers.**

### Phase 3: Instruction handler migration

1. In instruction handlers (`Add`, `Sub`, `Mul`, `Car`, `Cdr`, `VectorRef`,
   `VectorSet`): on data error, construct `Exception` via named constructor,
   set `current_exception`, push NIL, return `Ok(())`.
2. Stack underflow and bytecode errors remain as `Err(String)`.
3. Top-level `execute()` checks `current_exception` after execution. If set,
   formats using the exception's message and returns `Err(String)`.

### Phase 4: try/catch/finally implementation

1. Implement `try`/`catch`/`finally` as the primary exception handling syntax
   (replacing `handler-case`/`handler-bind`/`unwind-protect`).
2. `try` evaluates a body and catches exceptions via `catch` clauses.
3. `catch` clauses match exception types and bind the exception value.
4. `finally` blocks execute regardless of success or exception.
5. Non-unwinding recovery via fiber suspension: a `catch` clause can call
   `signal` to suspend the current fiber, allowing the caller to inspect the
   exception and decide whether to `resume` with a recovery value or let the
   exception propagate.

### Phase 5: Fiber-backed exception handling

1. Integrate exceptions with the fiber system: exceptions become first-class
   values that can be caught, inspected, and resumed.
2. Replace restart machinery with fiber suspension/resumption patterns.
3. `signal` suspends the current fiber with an exception, returning control to
   the caller.
4. `resume` resumes a suspended fiber with a recovery value.
5. This enables non-unwinding recovery without the complexity of restart objects.

### Phase 6: Cleanup

1. Remove `ErrorKind` → exception ID translation from `Call` handler.
2. `LError` remains for VM-internal use. It is never exposed to Elle code.
3. Update tests that checked for specific `Err(String)` messages.
4. Deprecate and eventually remove `handler-case`/`handler-bind` syntax.

## What This Does NOT Change (in current implementation)

- The interrupt mechanism (check `current_exception` at bottom of instruction
  loop, jump to handler offset).
- `PushHandler`/`PopHandler`/`CheckException`/`MatchException`/`ClearException`/
  `ReraiseException` bytecode instructions (will be replaced by fiber-aware
  equivalents).
- Handler isolation (save/restore `exception_handlers` across call boundaries).
- The exception hierarchy (exception → error → type-error, etc.).
- `NativeFn` remains a bare function pointer (`fn`), not a trait object.

## Surface Syntax: try/catch/finally

The target syntax for exception handling is:

```lisp
(try
  (risky-operation)
  (catch type-error (e)
    (handle-type-error e))
  (catch error (e)
    (handle-any-error e))
  (finally
    (cleanup)))
```

- **`try`** evaluates its body. If an exception is raised, control jumps to the
  first matching `catch` clause.
- **`catch`** clauses match exception types. The exception is bound to the
  variable and the clause body is evaluated.
- **`finally`** blocks execute regardless of success or exception. They are
  useful for cleanup (closing files, releasing locks, etc.).
- Multiple `catch` clauses are tried in order; the first match wins.
- If no `catch` clause matches, the exception propagates to the enclosing
  `try` or to the top level.

### Non-unwinding recovery via signal/resume

For cases where you want to recover without unwinding the stack, use fiber
suspension:

```lisp
(try
  (risky-operation)
  (catch error (e)
    (signal e)))  ; Suspend and return control to caller
```

The caller can then:

```lisp
(let ((fiber (spawn-fiber (lambda () ...))))
  (match (resume fiber)
    ((exception? e)
     (let ((recovery-value (ask-user-for-recovery e)))
       (resume fiber recovery-value)))
    (result result)))
```

This pattern replaces the old `restart-case`/`invoke-restart` machinery with
a simpler fiber-based model.

## JIT Exception Handling

JIT-compiled functions handle exceptions by checking for pending exceptions
after every function call (`Call` instruction). If `current_exception` is set
after a call returns, the JIT immediately returns NIL, bailing out to the
interpreter's exception handling machinery.

This means:
- Exceptions thrown by callees propagate correctly through JIT frames
- `try`/`catch` handlers in the calling interpreter frame will fire
- The JIT does NOT implement `PushHandler`/`PopHandler`/`MatchException` — 
  functions containing these instructions are not JIT-compiled

The effect system ensures pure functions (the only JIT candidates) don't
contain exception handling instructions directly. But pure functions can
call other functions that throw, so the bail-out mechanism is essential
for correctness.

Tail calls from JIT code (`TailCall` instruction) use `elle_jit_tail_call`
which sets `vm.pending_tail_call` for closure targets. Native functions and
VM-aware functions are called directly (no TCO needed). After JIT code returns,
the VM detects `pending_tail_call` and hands off to the interpreter's trampoline
loop, which handles chains of tail calls with O(1) stack usage.

Future work: inline exception checks in JIT code (check a flag instead of
calling a runtime helper), JIT-native try/catch (would require Cranelift
landing pads or setjmp/longjmp).

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
  convert `Err(Exception)` to `current_exception`, not to `Err(String)`.

- **try/catch replaces handler-case.** The new syntax is simpler and more
  familiar to users coming from other languages. It integrates naturally with
  the fiber model for non-unwinding recovery.

- **finally replaces unwind-protect.** Cleanup blocks are now a first-class
  part of the exception handling syntax, not a separate mechanism.

- **signal/resume replaces restart machinery.** Fiber suspension is simpler
  and more composable than restart objects. A suspended fiber is just a value
  that can be inspected and resumed.
