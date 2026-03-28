# WASM Backend

LIR → WASM emission via `wasm-encoder`, execution via Wasmtime.

## Architecture

```
LIR → WasmEmitter (emit.rs) → .wasm bytes + const_pool
                                    ↓
                              Wasmtime Engine/Store/Linker (store.rs)
                                    ↓
                              Host functions (store.rs, host.rs)
                                    ↓
                              HandleTable (handle.rs) ← heap objects live here
```

### Key design: host-side handles

WASM code sees Values as `(tag: i64, payload: i64)`. Immediates (int, float,
nil, bool, symbol, keyword) are constructed in WASM. Heap values (strings,
arrays, closures, etc.) have payload = opaque handle index into `HandleTable`
on the host. All heap operations cross the WASM-host boundary via host
function calls.

### Control flow: loop + br_table

Multi-block functions use a state machine: `loop { block*N { br_table } }`.
Each LIR basic block is a case. Jump/Branch set state and `br` to loop.
Return uses WASM `return`. Single-block functions skip the loop.

### Closures and tail calls

Each `MakeClosure` in LIR produces a separate WASM function in a `funcref`
table. `rt_make_closure` creates a `Closure` value with `wasm_func_idx`.
`rt_call` dispatches closure calls by building env in linear memory
(captures + params + local slots) and invoking via table lookup.

Tail calls use `return_call_indirect` (WASM tail call proposal). The host
function `rt_prepare_tail_call` sets up the env for the callee; the WASM
function then tail-calls through the funcref table. Entry functions that
lack the right signature fall back to `call` + `return`.

### Three address spaces in emit.rs

- **Env** (linear memory): captures, params, LBox locals → LoadCapture/StoreCapture
- **Local slots** (dedicated WASM locals): non-LBox let-bound vars → LoadLocal/StoreLocal
- **Registers** (separate WASM locals): computation intermediates → Reg(N)

`local_slot_tag(slot)` / `local_slot_pay(slot)` map slots to WASM local indices.
No collisions between register bank and local variable storage.

### Float arithmetic

Tag-check dispatch: if either operand is TAG_FLOAT, use f64 instructions.
Int-to-float promotion for mixed operands. Bitwise ops remain integer-only.

### Signal propagation

`store_result_with_signal` writes signal to memory[0..4] before returning.
`call_wasm_closure` reads signal from memory after WASM call returns.
Signals propagate through WASM↔host boundaries.

For SIG_YIELD in suspending functions, `emit_call_suspending` checks the
signal BEFORE the general early-return path, spills caller state, and
returns suspended. `rt_call` intercepts SIG_RESUME from fiber/resume and
executes the fiber's WASM closure host-side via `handle_fiber_resume`.

## Files

| File | Purpose |
|------|---------|
| `emit.rs` | LIR → WASM emission. `emit_module()` is the entry point. |
| `handle.rs` | `HandleTable`: maps u64 handles to `Rc<HeapObject>`. |
| `host.rs` | `ElleHost` state (handle table + primitive dispatch + const pool). |
| `store.rs` | Wasmtime setup, host function registration, `call_wasm_closure`, `dispatch_data_op`. |
| `mod.rs` | `eval_wasm()` entry point. |

## Host functions (WASM imports)

| Import | Purpose |
|--------|---------|
| `call_primitive` | Dispatch by prim_id (unused currently, rt_call covers this) |
| `rt_call` | Dynamic function call: NativeFn or WASM closure dispatch |
| `rt_load_const` | Load heap constant from const_pool by index |
| `rt_data_op` | Data operations (cons, car, cdr, arrays, lbox, etc.) by opcode |
| `rt_make_closure` | Create Closure value with wasm_func_idx + captures |
| `rt_prepare_tail_call` | Build env for tail callee, return func_idx for `return_call_indirect` |
| `rt_yield` | Save yielded value + live regs to WasmSuspensionFrame |
| `rt_get_resume_value` | Return the resume value passed by scheduler |
| `rt_load_saved_reg` | Load saved register by index from suspension frame |

## Linear memory layout

| Region | Offset | Purpose |
|--------|--------|---------|
| Signal word | 0..4 | Signal bits from last host call |
| Args buffer | 256 (ARGS_BASE) | Call args + data op args |
| Env stack | 4096+ (ENV_STACK_BASE) | Closure envs for `call_wasm_closure` |

The env region uses a **stack allocator** (`ElleHost::env_stack_ptr`). Each
`call_wasm_closure` bumps the pointer forward by the env size; on return it
restores. This prevents nested closure calls from overwriting each other's
environments. Memory is grown automatically if the stack exceeds one page.

## WASM local layout

Entry function: `[tags: i64 * N] [payloads: i64 * N] [env_ptr: i32] [signal/state: i32]`

Closure function (params 0-3): `[tags: i64 * N] [payloads: i64 * N] [signal/state: i32]`
- Param 0 = env_ptr, Param 1 = args_ptr (unused), Param 2 = nargs (unused), Param 3 = ctx

Register mapping: `tag_local(Reg(i)) = offset + i`, `pay_local(Reg(i)) = offset + N + i`
where offset = 0 for entry, 4 for closures.

## What works (58 tests, ELLE_WASM=1 make smoke passes)

- All LIR instructions except Eval (emit Unreachable)
- Constants (int, float, bool, nil, empty_list, symbol, keyword, string)
- Arithmetic (int and float with tag dispatch), comparisons, bitwise, unary
- Control flow: if/else (nested, cond), let*, defn, letrec, block/break
- All 331 primitives via rt_call
- Closures: creation, calling, capture, higher-order, recursion, mutual recursion
- Tail calls via `return_call_indirect` (100K deep recursion verified)
- Nested closure calls with captures (env stack allocator)
- Data: cons, car, cdr, arrays, structs, destructuring, lbox
- Strings via constant pool
- Signal propagation (error early-return after host calls)
- stdlib.lisp compiles to WASM and runs; all smoke examples pass
- Yield/resume: basic yield, resume with value, multiple sequential yields
- Yield-through-call: callee yields, caller suspends, resume chain
- Fiber primitives: fiber/new, fiber/resume work with WASM closures

### CPS state-machine transform (Phase 2)

Yielding functions become re-entrant via compile-time state machine:
- Closures return `(tag: i64, payload: i64, status: i32)`: status=0 normal, >0 suspended
- `ctx` parameter (param 3) carries resume state on re-entry (0 = initial)
- Yield spills all registers + local slots to ARGS_BASE, calls `rt_yield`, returns suspended
- Resume prologue: if ctx!=0, dispatch via br_table to restore block, load saved regs
- Yield-through-call: after Call in suspending functions, check SIG_YIELD, spill+return
- Virtual resume blocks handle mid-block resume (instructions after the call + terminator)
- Host snapshots env, manages `WasmSuspensionFrame` stack, drives resume chain

Host functions: `rt_yield`, `rt_get_resume_value`, `rt_load_saved_reg`.

## Known issues / Phase 3 scope

- `Eval` — needs dynamic module compilation (Phase 3)
- Recursive yield-through-call with multiple suspension frames — incorrect frame ordering
- Port-edge-cases example slow under WASM — needs investigation
