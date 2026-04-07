# WASM Backend

LIR → WASM emission via `wasm-encoder`, execution via Wasmtime.

## Architecture

```
LIR → WasmEmitter (emit.rs) → .wasm bytes + const_pool
                                    ↓
                              Wasmtime Engine/Store (store.rs)
                                    ↓
                              Host functions (linker.rs)
                                    ↓
                              Fiber resume chain (resume.rs)
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
`handle_wasm_result` reads signal from memory after WASM call returns.
Signals propagate through WASM↔host boundaries.

For SIG_YIELD in suspending functions, `emit_call_suspending` checks the
signal from the return value BEFORE the general early-return path, spills
caller state, and returns suspended. Virtual resume blocks (CPS continuations
for mid-block call sites) use the same suspending-aware emission so that
yield-through-call works correctly after resume.

`rt_call` intercepts SIG_RESUME from fiber/resume and executes the fiber's
WASM closure host-side via `handle_fiber_resume` (in resume.rs).

## Files

| File | Purpose |
|------|---------|
| `emit.rs` | LIR → WASM emission. `emit_module()` is the entry point. |
| `handle.rs` | `HandleTable`: maps u64 handles to `Rc<HeapObject>`. |
| `host.rs` | `ElleHost` state (handle table + primitives + suspension frames). |
| `linker.rs` | Host function registration (`create_linker`), data op dispatch. |
| `resume.rs` | Fiber resume chain (`drive_resume_chain`, `handle_fiber_resume`). |
| `store.rs` | Engine/Store creation, `call_wasm_closure`, `resume_wasm_closure`, `run_module`. |
| `lazy.rs` | `WasmTier`: per-closure WASM compilation and tiered dispatch. |
| `regalloc.rs` | Register allocation for WASM locals. |
| `mod.rs` | `eval_wasm()` entry point. |

## Host functions (WASM imports)

| Import | Purpose |
|--------|---------|
| `call_primitive` | Dispatch by prim_id (registered but unused; rt_call covers this) |
| `rt_call` | Dynamic function call: NativeFn or WASM closure dispatch |
| `rt_load_const` | Load heap constant from const_pool by index |
| `rt_data_op` | Data operations (cons, car, cdr, arrays, lbox, etc.) by opcode |
| `rt_make_closure` | Create Closure value with wasm_func_idx + captures |
| `rt_prepare_tail_call` | Build env for tail callee, return func_idx for `return_call_indirect` |
| `rt_yield` | Save yielded value + live regs to WasmSuspensionFrame |
| `rt_get_resume_value` | Return the resume value passed by scheduler |
| `rt_load_saved_reg` | Load saved register by index from suspension frame |
| `rt_push_param` | Push dynamic parameter binding frame |
| `rt_pop_param` | Pop dynamic parameter binding frame |

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

## Suspension frames and resume chain

Per-fiber suspension frames are stored in a `VecDeque` keyed by fiber ID.
Frames are pushed to the back (innermost first during yield-through-call)
and consumed from the front (innermost first during resume).

**Resume protocol (`resume_wasm_closure`):**
1. Peek the front frame (innermost) — `rt_load_saved_reg` reads from it
2. Restore env to linear memory from the frame's snapshot
3. Call WASM with `ctx = resume_state`
4. Pop the front frame after the call completes
5. If re-yielded: `back_suspension_frame_mut()` updates the new frame
6. New frames pushed by `rt_yield` go to the back, so they don't interfere

**`drive_resume_chain`:** Loops `resume_wasm_closure` until all frames are
consumed (Dead), a frame yields (Yielded), or a frame errors (Error).

**`handle_fiber_resume`:** Dispatches New (call_wasm_closure) vs Paused
(drive_resume_chain). Sets fiber status and signal on completion.

## CPS state-machine transform

Yielding functions become re-entrant via compile-time state machine:
- Closures return `(tag: i64, payload: i64, status: i32)`: status=0 normal, >0 suspended
- `ctx` parameter (param 3) carries resume state on re-entry (0 = initial)
- Yield spills all registers + local slots to ARGS_BASE, calls `rt_yield`, returns suspended
- Resume prologue: if ctx!=0, dispatch via br_table to restore block, load saved regs
- Yield-through-call: after Call in suspending functions, check SIG_YIELD, spill+return
- Virtual resume blocks handle mid-block resume (instructions after the call + terminator)
- Both real blocks and virtual resume blocks use `emit_call_suspending` for SuspendingCall
- Host snapshots env, manages `WasmSuspensionFrame` deque, drives resume chain

## WASM local layout

Entry function: `[tags: i64 * N] [payloads: i64 * N] [env_ptr: i32] [signal/state: i32]`

Closure function (params 0-3): `[tags: i64 * N] [payloads: i64 * N] [signal/state: i32]`
- Param 0 = env_ptr, Param 1 = args_ptr (unused), Param 2 = nargs (unused), Param 3 = ctx

Register mapping: `tag_local(Reg(i)) = offset + i`, `pay_local(Reg(i)) = offset + N + i`
where offset = 0 for entry, 4 for closures.

## Deterministic WASM output

WASM module bytes are fully deterministic across runs of the same source.
Non-deterministic runtime values (symbol IDs, keyword hashes) are routed
through the constant pool (`rt_load_const`) instead of inlined as `i64.const`.
Register allocation uses deterministic slot freeing order.

This enables reliable module caching: `--cache=/path` hashes the
WASM bytes and reuses pre-compiled modules on cache hit (~3ms vs ~400ms).

## Tiered compilation (lazy WASM)

`--wasm=N` enables tiered execution: the bytecode VM runs by default,
and hot closures are compiled to per-closure WASM modules on demand.

**Constraints on per-closure compilation:**
- No `MakeClosure` instructions (nested closures stay on bytecode VM)
- No `TailCall`/`TailCallArrayMut` (uses `return_call_indirect` with callee table indices)
- No `Yield` terminators (suspension frame management)

**Self-recursive call optimization:** When `rt_call` detects a call to the
same closure currently executing in WASM (same bytecode pointer), it dispatches
directly through the instance's funcref table (`call_indirect` on index 0)
instead of creating a new Store. This makes recursive functions like fib
efficient within a single WASM instance.

## Known issues

- `Eval` — needs dynamic module compilation (post-merge)
- Port-edge-cases example slow under WASM — needs investigation
- Tiered mode: per-call Store creation for cross-closure WASM calls is slow
- `call_primitive` host function registered but unused (import required by module declaration)
