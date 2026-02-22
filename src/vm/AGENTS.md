# vm

Bytecode execution. Stack-based operand handling with register-addressed locals.

## Responsibility

Execute bytecode instructions. Manage:
- Operand stack
- Global bindings
- Call frames and stack traces
- Closure environments
- Fiber state and signals

Does NOT:
- Compile code (that's `compiler/`, `hir/`, `lir/`)
- Parse source (that's `reader/`)
- Define primitives (that's `primitives/`)

## Interface

| Type | Purpose |
|------|---------|
| `VM` | Global state + root Fiber. Per-execution state lives on `vm.fiber` |
| `SignalBits` | Internal return type: `SIG_OK`, `SIG_ERROR`, `SIG_YIELD`, `SIG_DEBUG`, `SIG_RESUME`, `SIG_FFI`, `SIG_PROPAGATE`, `SIG_CANCEL` |
| `CallFrame` | Function name, IP, frame base |

## Data flow

```
Bytecode + Constants (as Rc<Vec<u8>>, Rc<Vec<Value>>)
    │
    ▼
execute_bytecode()  ← public API, wraps slices in Rc once, returns Result<Value, String>
    │
    ├─► execute_bytecode_inner_impl() → (SignalBits, usize)
    │       │
    │       ├─► fetch instruction
    │       ├─► dispatch by opcode
    │       ├─► modify stack/locals/globals
    │       ├─► check for errors
    │       └─► loop until Return/Yield/Error
    │       │
    │       ▼
    │   (SignalBits, ip) — signal + IP at exit
    │
    ▼
Result<Value, String>  ← translation boundary
```

## Signal-based returns

Internal VM methods return `SignalBits` (or `(SignalBits, usize)` for the
inner dispatch loop):
- `SIG_OK` (0): Normal completion. Value in `fiber.signal`.
- `SIG_ERROR` (1): Error. Error tuple in `fiber.signal` as `[:keyword "message"]`.
- `SIG_YIELD` (2): Fiber yield. Value in `fiber.signal`, suspended frames in `fiber.suspended`.
- `SIG_RESUME` (8): VM-internal. Fiber primitive requests VM-side execution.
- `SIG_PROPAGATE` (32): VM-internal. `fiber/propagate` re-raises caught signal.
- `SIG_CANCEL` (64): VM-internal. `fiber/cancel` injects error into fiber.

The public `execute_bytecode` method is the translation boundary — it converts
`SignalBits` to `Result<Value, String>` for external callers. On `SIG_ERROR`,
it extracts the error tuple from `fiber.signal` and formats the error message.

Instruction handlers no longer return `Result<(), String>`. VM bugs panic
immediately. User errors set `fiber.signal` to `(SIG_ERROR, error_val(kind, msg))`
and push `Value::NIL` to keep the stack consistent.

## Rc threading

Bytecode and constants are threaded through the dispatch loop as `&Rc<Vec<u8>>`
and `&Rc<Vec<Value>>`. Individual instruction handlers dereference to slices
(`&[u8]`, `&[Value]`). Only the dispatch loop and its direct callees
(`handle_yield`, `handle_call`) need the `Rc` — they clone it cheaply when
creating `SuspendedFrame`s or `TailCallInfo`.

- `execute_bytecode` wraps raw slices in `Rc` once at the public boundary
- `execute_bytecode_from_ip` / `execute_bytecode_saving_stack` take `&Rc`
- `TailCallInfo` is `(Rc<Vec<u8>>, Rc<Vec<Value>>, Rc<Vec<Value>>)` — tail
  calls clone the Rc (cheap), not the Vec (expensive)
- `closure_env` parameter is `&Rc<Vec<Value>>` (non-optional; empty Rc for no env)

## Primitive dispatch (NativeFn)

All primitives are `NativeFn`: `fn(&[Value]) -> (SignalBits, Value)`. The VM
dispatches the return signal in `handle_primitive_signal()` (`signal.rs`):
- `SIG_OK` → push value to stack
- `SIG_ERROR` → store `(SIG_ERROR, value)` in `fiber.signal`, push NIL
- `SIG_YIELD` → store in `fiber.signal`, return yield
- `SIG_RESUME` → dispatch to fiber handler
- `SIG_PROPAGATE` → re-raise child fiber's signal, preserve child chain
- `SIG_CANCEL` → inject error into target fiber

All SIG_RESUME primitives (including coroutine wrappers) return
`(SIG_RESUME, fiber_value)`. The VM uses `FiberHandle::take()`/`put()` to swap
the child fiber into `vm.fiber`, executes the child, then swaps back.

On resume, the VM wires up the parent/child chain (Janet semantics):
- `parent.child = child_handle` before executing child
- On signal caught (SIG_OK or mask match): clear `parent.child = None`
- On signal NOT caught (propagates): leave `parent.child` set (trace chain)

## Dependents

- `primitives/` - NativeFn primitives; SIG_RESUME signals trigger VM-side execution
- `repl.rs` - runs compiled code
- `main.rs` - file execution

## Invariants

1. **Stack underflow is a VM bug.** Every pop must have a preceding push.
   If you see "Stack underflow," the bytecode or emitter is broken. Handlers
   panic on stack underflow.

2. **Closure environments are immutable Rc<Vec>.** The vec is created at
   closure call time; mutations go through cells, not env modification.

3. **`LocalCell` auto-unwraps on `LoadUpvalue`.** `Cell` (user's `box`) does
   NOT auto-unwrap. This distinction matters.

4. **Tail calls don't grow call_depth.** `TailCall` stores pending call info
   and returns; the outer loop executes it. Stack overflow = tail call bug.

5. **Yield uses `SuspendedFrame` chains.** On yield, a `SuspendedFrame`
   captures bytecode (`Rc`), constants (`Rc`), env (`Rc`), IP, and operand
   stack. When yield propagates through Call instructions, each caller's frame
   is appended to `fiber.suspended`. `resume_suspended` replays frames from
   innermost (index 0) to outermost (last index).

6. **VM bugs panic, user errors set `fiber.signal`.** Instruction handlers
   return `()` (not `Result`). VM bugs (stack underflow, bad bytecode) panic
   immediately. User errors (type mismatch, division by zero) set
   `fiber.signal` to `(SIG_ERROR, error_val(kind, msg))`, push `Value::NIL`
   to keep the stack consistent, and return normally. The dispatch loop checks
   `fiber.signal` for `SIG_ERROR` after each instruction and returns
   immediately. See `set_error()` in `call.rs` and `fiber.rs` for the helper.

## Key VM fields

| Field | Type | Purpose |
|-------|-------|---------|
| `fiber` | `Fiber` | Root fiber: stack, call frames, signal state |
| `current_fiber_handle` | `Option<FiberHandle>` | Handle for current fiber (`None` for root) |
| `current_fiber_value` | `Option<Value>` | Cached NaN-boxed Value for current fiber (`None` for root) |
| `globals` | `Vec<Value>` | Global bindings by SymbolId |
| `jit_cache` | `HashMap<*const u8, Rc<JitCode>>` | JIT code cache |
| `scope_stack` | `ScopeStack` | Runtime scope stack |
| `pending_tail_call` | `Option<TailCallInfo>` | Rc-based tail call info (transient) |

### Key Fiber fields (on `vm.fiber`)

| Field | Type | Purpose |
|-------|-------|---------|
| `stack` | `SmallVec<[Value; 256]>` | Operand stack |
| `call_stack` | `Vec<CallFrame>` | For stack traces |
| `call_depth` | `usize` | Stack overflow detection |
| `signal` | `Option<(SignalBits, Value)>` | Signal from execution (errors, yields) |
| `suspended` | `Option<Vec<SuspendedFrame>>` | Suspended execution frames (for yield/signal resumption) |
| `signal_mask` | `SignalBits` | Which signals this fiber catches |
| `parent` | `Option<WeakFiberHandle>` | Weak back-pointer to parent fiber |
| `parent_value` | `Option<Value>` | Cached NaN-boxed Value for parent (identity-preserving) |
| `child` | `Option<FiberHandle>` | Strong pointer to child fiber |
| `child_value` | `Option<Value>` | Cached NaN-boxed Value for child (identity-preserving) |

## Suspension mechanism

When a fiber suspends (via yield instruction or `fiber/signal`):

1. **Yield instruction** (`handle_yield`): captures innermost frame as a
   `SuspendedFrame` with bytecode (Rc clone), constants (Rc clone), env
   (Rc clone), IP (after yield), and operand stack. Stored in `fiber.suspended`.
2. **Call handler** (if yield propagates through a call): appends caller's
   frame to `fiber.suspended` vec.
3. **Signal suspension** (`fiber/signal`): single `SuspendedFrame` with empty
   stack, stored in `fiber.suspended` by the resume handler.
4. **Frame ordering**: innermost (yielder/signaler) at index 0, outermost
   (caller) at last index.
5. **Resume** (`resume_suspended`): iterates frames forward, calling
   `execute_bytecode_from_ip` for each. Handles re-yields and errors.

Key methods:
- `execute_bytecode_from_ip`: Executes from a given IP with Rc bytecode/constants
- `execute_bytecode_saving_stack`: Saves/restores caller's stack, handles tail calls
- `resume_suspended`: Replays `Vec<SuspendedFrame>`, handles re-yields and errors
- `with_child_fiber`: Shared swap protocol for fiber resume/cancel

## Files

| File | Lines | Content |
|------|-------|---------|
| `mod.rs` | ~100 | VM struct, VmResult, public interface |
| `dispatch.rs` | ~334 | Main execution loop, instruction dispatch, returns `(SignalBits, usize)` |
| `call.rs` | ~417 | Call, TailCall, JIT dispatch, environment building |
| `signal.rs` | ~93 | Primitive signal dispatch (`handle_primitive_signal`) |
| `fiber.rs` | ~388 | Fiber resume/propagate/cancel, shared swap protocol |
| `execute.rs` | ~94 | `execute_bytecode_from_ip`, `execute_bytecode_saving_stack` |
| `core.rs` | ~453 | VM struct, `resume_suspended`, stack trace helpers |
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
