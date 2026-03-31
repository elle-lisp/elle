# VM

The VM is a stack-machine interpreter that executes bytecode.

## Architecture

```text
┌──────────────┐
│    Fiber     │ ← execution context
│  ┌────────┐  │
│  │ Stack  │  │ ← operand stack (Values)
│  │ Frames │  │ ← call frame stack
│  │ Locals │  │ ← register-allocated locals
│  └────────┘  │
└──────────────┘
```

## Key types

- **`VM`** — owns the current fiber, primitive table, compiler, and
  JIT compiler
- **`Fiber`** — execution context: operand stack, call frames, locals,
  signal state, arena
- **`CallFrame`** — return address, local variable base, function
  metadata
- **`BytecodeFrame`** — points into a `CompiledFunction`'s bytecode
  stream

## Dispatch loop

The main loop in `execute.rs`:

1. Read opcode byte
2. Decode operands
3. Pop operands from stack
4. Perform operation
5. Push result
6. Advance instruction pointer

Specialized fast paths exist for common sequences (e.g., `LoadLocal` +
`AddInt` + `StoreLocal` for counter increments).

## Fiber integration

- **Yield** — saves the current frame as a `SuspendedFrame`, returns
  control to the parent fiber or scheduler
- **Signal emission** — checks the fiber's signal mask to decide
  whether to propagate or catch
- **Fuel** — decrements a counter per instruction; when zero, emits
  `:fuel` signal

## Tail calls

`TailCall` reuses the current call frame rather than pushing a new one.
The VM validates tail position at compile time. This guarantees constant
stack space for tail-recursive functions.

## JIT fallback

When a function is JIT-compiled, `Call` dispatches to the native code
pointer instead of interpreting bytecode. If the JIT rejects a function
(e.g., due to yields), the VM falls back to bytecode interpretation.

## Files

```text
src/vm/core.rs        VM struct and initialization
src/vm/execute.rs     Main dispatch loop
src/vm/dispatch.rs    Opcode handlers
src/vm/call.rs        Call/return mechanics
src/vm/fiber.rs       Fiber state management
```

---

## See also

- [impl/bytecode.md](bytecode.md) — instruction set
- [impl/jit.md](jit.md) — JIT compilation
- [impl/values.md](values.md) — Value representation
