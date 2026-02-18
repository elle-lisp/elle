# vm

Bytecode execution. Stack-based operand handling with register-addressed locals.

## Responsibility

Execute bytecode instructions. Manage:
- Operand stack
- Global bindings
- Call frames and stack traces
- Closure environments
- Exception handlers
- Coroutine state

Does NOT:
- Compile code (that's `compiler/`, `hir/`, `lir/`)
- Parse source (that's `reader/`)
- Define primitives (that's `primitives/`)

## Interface

| Type | Purpose |
|------|---------|
| `VM` | Execution state: stack, globals, call frames |
| `VmResult` | `Done(Value)` or `Yielded(Value)` |
| `CallFrame` | Function name, IP, frame base |
| `ExceptionHandler` | Handler offset, finally offset, stack depth |

## Data flow

```
Bytecode + Constants
    │
    ▼
execute_bytecode()
    │
    ├─► fetch instruction
    ├─► dispatch by opcode
    ├─► modify stack/locals/globals
    ├─► check for exceptions
    └─► loop until Return/Yield
    │
    ▼
VmResult
```

## Dependents

- `primitives/` - `VmAwareFn` primitives call back into VM
- `repl.rs` - runs compiled code
- `main.rs` - file execution

## Invariants

1. **Stack underflow is an error.** Every pop must have a preceding push.
   If you see "Stack underflow," the bytecode or emitter is broken.

2. **Closure environments are immutable Rc<Vec>.** The vec is created at
   closure call time; mutations go through cells, not env modification.

3. **`LocalCell` auto-unwraps on `LoadUpvalue`.** `Cell` (user's `box`) does
   NOT auto-unwrap. This distinction matters.

4. **Tail calls don't grow call_depth.** `TailCall` stores pending call info
   and returns; the outer loop executes it. Stack overflow = tail call bug.

5. **Exception handlers are a stack.** `PushHandler` adds, `PopHandler` removes.
   On exception, unwind to handler's stack_depth and jump to handler_offset.

6. **Coroutines use first-class continuations.** On yield, a `ContinuationFrame`
   captures bytecode, constants, env, IP, stack, and exception handler state.
   When yield propagates through Call instructions, each caller's frame is
   appended to form a chain. `resume_continuation` replays frames from
   innermost to outermost, restoring handler state for each frame.

7. **Instruction handlers have two error channels.** `Err(String)` is for VM
   bugs (stack underflow, bad bytecode). `Ok(())` with `current_exception`
   set is for runtime errors on bad data (type mismatch, division by zero).
   The handler pushes `Value::NIL` to keep the stack consistent and returns
   `Ok(())`. The interrupt mechanism at the bottom of the instruction loop
   handles the exception. See `handle_div_int` and `handle_load_global` for
   the canonical pattern.

## Key VM fields

| Field | Type | Purpose |
|-------|-------|---------|
| `stack` | `SmallVec<[Value; 256]>` | Operand stack |
| `globals` | `HashMap<u32, Value>` | Global bindings by SymbolId |
| `call_stack` | `Vec<CallFrame>` | For stack traces |
| `exception_handlers` | `Vec<ExceptionHandler>` | Active handlers |
| `current_exception` | `Option<Rc<Condition>>` | Exception being handled |
| `coroutine_stack` | `Vec<Rc<RefCell<Coroutine>>>` | Active coroutines |
| `pending_tail_call` | `Option<(bytecode, constants, env)>` | Deferred tail call |

## Exception hierarchy

```
condition (1)
├── error (2)
│   ├── type-error (3)
│   ├── division-by-zero (4)
│   ├── undefined-variable (5)
│   └── arity-error (6)
└── warning (7)
    └── style-warning (8)
```

Hierarchy data and `is_exception_subclass(child, parent)` live in
`value/condition.rs` — the single source of truth. Re-exported from `vm/mod.rs`.

## Continuation mechanism

When a coroutine yields:

1. **Yield instruction** captures innermost frame: bytecode, constants, env,
   IP (after yield), stack, exception_handlers, handling_exception
2. **Call handler** (if yield propagates through a call) appends caller's
   frame to the continuation chain
3. **Frame ordering**: innermost (yielder) first, outermost (caller) last
4. **Resume** iterates frames forward, calling `execute_bytecode_from_ip_with_state`
   for each, which restores exception handler state before execution

Key methods:
- `execute_bytecode_from_ip_with_state`: Executes with pre-set handler state
- `resume_continuation`: Replays frame chain, handles re-yields and exceptions

Exception handling across resume:
- Exception check at START of instruction loop catches exceptions from inner frames
- Each frame's handlers are restored before execution
- Exceptions propagate to outer frames if no local handler

## Files

| File | Lines | Content |
|------|-------|---------|
| `mod.rs` | ~1200 | Main execution loop, instruction dispatch |
| `core.rs` | ~600 | `VM` struct, `VmResult`, `resume_continuation` |
| `stack.rs` | ~100 | Stack operations: LoadConst, Pop, Dup |
| `variables.rs` | ~150 | LoadGlobal, StoreGlobal, LoadUpvalue, etc. |
| `control.rs` | ~100 | Jump, JumpIfFalse, Return |
| `closure.rs` | ~100 | MakeClosure |
| `arithmetic.rs` | ~150 | Add, Sub, Mul, Div |
| `comparison.rs` | ~100 | Eq, Lt, Gt, Le, Ge |
| `types.rs` | ~50 | IsNil, IsEmptyList, IsPair, Not |
| `data.rs` | ~100 | Cons, Car, Cdr, MakeVector |
| `scope/` | ~200 | Runtime scope stack (legacy) |

## Truthiness

The VM evaluates truthiness via `Value::is_truthy()`:
- `Value::NIL` → falsy
- `Value::FALSE` → falsy  
- Everything else (including `Value::EMPTY_LIST`, `Value::int(0)`) → truthy

The `Instruction::Nil` pushes `Value::NIL` (falsy).
The `Instruction::EmptyList` pushes `Value::EMPTY_LIST` (truthy).
```
