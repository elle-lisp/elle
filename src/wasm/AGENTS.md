# WASM Backend

<!-- audited: 2026-10-06 -->

LIR → WASM emission via `wasm-encoder`, execution via Wasmtime.

## Architecture

```text
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
Each LIR basic block, and each park site's resume entry, is a case. A
Jump/Branch to a later case is a plain `br`; one to an earlier case sets the
state and goes round the loop. Return uses WASM `return`. Single-block
functions skip the loop.

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

`store_result_with_signal` writes the signal to `SIGNAL_SLOT`, memory[0..8], before returning.
`handle_wasm_result` reads signal from memory after WASM call returns.
Signals propagate through WASM↔host boundaries.

In a function that may signal, every call and every instruction that may
raise is a park site (`suspend::ParkSite`). A call that parks — any signal
but a halt, an error included — makes the site check the suspended word
before the early-return path, spill the live slots, and return suspended. A
host operation that raises parks its caller the same way. Each site's resume
entry is a case of the dispatch loop (`layout::Layout`).

`rt_call` intercepts SIG_RESUME from fiber/resume and executes the fiber's
WASM closure host-side via `handle_fiber_resume` (in resume.rs).

## Files

| File | Purpose |
|------|---------|
| [emit.rs](emit.rs) | LIR → WASM emission. `emit_module()` is the entry point. |
| [emit/abi.rs](emit/abi.rs) | The numbers the module and the host agree on: imports, memory slots, data-op codes. |
| [emit/functions.rs](emit/functions.rs) | Module sections, and the entry / closure function bodies. |
| [instruction.rs](instruction.rs) | LIR instruction → WASM instruction translation. |
| [controlflow.rs](controlflow.rs) | CFG emission: the dispatch loop, the instructions of each case, the terminators. |
| [layout.rs](layout.rs) | The order of the dispatch loop's cases, and the branch that reaches each one. |
| [suspend.rs](suspend.rs) | Park sites, the park itself, spill and restore, and block splitting. |
| [liveness.rs](liveness.rs) | Which register and local slots are live at each park site. |
| [outcome.rs](outcome.rs) | `CallOutcome`: what a call reports back to emitted code. |
| [handle.rs](handle.rs) | `HandleTable`: maps u64 handles to `Rc<HeapObject>`. |
| [host.rs](host.rs) | `ElleHost` state (handle table + primitives) and primitive dispatch. |
| [host/frames.rs](host/frames.rs) | The frames a fiber parks on this tier, kept per fiber in the host: a compiled closure's saved state, and a bytecode callee's parked chain. |
| [host/io.rs](host/io.rs) | Top-level I/O with no scheduler to take it: the backend a request reaches, and the completion it reads its answer out of. |
| [linker.rs](linker.rs) | Host function registration (`create_linker`); `linker/` holds the registrations and the data-op dispatch. |
| [linker/bytecode.rs](linker/bytecode.rs) | The bridge to a closure the module never compiled: run it on the host VM, and park and resume the chain it parks. |
| [resume.rs](resume.rs) | Fiber resume chain (`drive_resume_chain`, `handle_fiber_resume`). |
| [resume/route.rs](resume/route.rs) | What a fiber's own mask makes of its body's outcome: caught, parked, or propagated. |
| [store.rs](store.rs) | Engine/Store creation, `call_wasm_closure`, `resume_wasm_closure`, `run_module`. |
| [lazy.rs](lazy.rs) | `WasmTier`: per-closure WASM compilation and tiered dispatch. |
| [regalloc.rs](regalloc.rs) | Register allocation for WASM locals. |
| [mod.rs](mod.rs) | `eval_wasm()` entry point. |
| [tests/](tests/) | The unit tests, one file per subject. |

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
| `rt_emit` | The capability gate on a literal emit: the value and signal it parks on, a denial's where the fiber withholds a bit |
| `rt_push_param` | Push dynamic parameter binding frame; raises on an operand that is not a parameter |
| `rt_pop_param` | Pop dynamic parameter binding frame |

## Linear memory layout

| Region | Offset | Purpose |
|--------|--------|---------|
| Signal word | 0..8 (SIGNAL_SLOT) | Signal bits from last host call |
| Args buffer | 256 (ARGS_BASE) | Call args, data op args, and a park's spill |
| Env stack | 4096+ (ENV_STACK_BASE), above the widest args region and the widest spill | Closure envs for `call_wasm_closure` |

The env region uses a **stack allocator** (`ElleHost::env_stack_ptr`). Each
`call_wasm_closure` bumps the pointer forward by the env size; on return it
restores. This prevents nested closure calls from overwriting each other's
environments. Memory is grown automatically if the stack exceeds one page.

## Suspension frames and resume chain

Per-fiber parked frames are stored in a `VecDeque` keyed by fiber ID. A frame
is a `ParkedFrame`: a compiled closure's `WasmSuspensionFrame`, or the
`BytecodePark` chain a bytecode callee left. Frames are pushed to the back
(innermost first during yield-through-call) and consumed from the front
(innermost first during resume).

**Resume protocol (`resume_wasm_closure`):**
1. Peek the front frame (innermost) — `rt_load_saved_reg` reads from it
2. Restore env to linear memory from the frame's snapshot
3. Call WASM with `ctx = resume_state`
4. Pop the front frame after the call completes
5. If re-yielded: `back_suspension_frame_mut()` updates the new frame
6. New frames pushed by `rt_yield` go to the back, so they don't interfere

**`drive_resume_chain`:** Loops `resume_wasm_closure` until all frames are
consumed (Dead) or a frame parks again (Yielded), an error included.

**`handle_fiber_resume`:** Dispatches New (call_wasm_closure) vs Paused
(drive_resume_chain). Sets fiber status and signal on completion.

**Uncaught-suspend propagation + re-drive.** A `(fiber/resume child)` whose
`child` suspends on a scheduler wait/io its mask does not cover must PROPAGATE
that suspension to the resumer (so the scheduler drives it), then RE-DRIVE
`child` when the resumer is itself resumed — the WASM analogue of the VM's
`SuspendedFrame::FiberResume`. `route_emit` parks `child`, records it under the
parent in `pending_redrive`, and returns `SIG_YIELD | bits`; `rt_yield` stamps
the parent's continuation frame with `redrive_child`; `drive_resume_chain`
honors that marker (via `redrive_child`) before resuming the frame. This is
what makes `protect`/`defer`/`with` around a suspending body work. Pinned by
[wasm-protect-suspend.lisp](../../tests/lang/wasm-protect-suspend.lisp).

**The capability gate.** Every host path that reaches a native — `rt_call`,
`rt_prepare_tail_call`, the `call_primitive` import, and the tiered linker's own
`rt_call` — calls `host::capability_denial` before it runs the primitive, so
this tier denies a withheld primitive exactly as the interpreter and the JIT do.
Each path then builds the native's ctx with `ElleHost::native_ctx`, which
carries the driven fiber's withheld set, so a fiber or thread the native starts
inherits it. `handle_fiber_resume` adds the resumer's set to the fiber it
drives. A literal emit reaches no host call path, so its `Emit` terminator
calls `rt_emit`, which asks the same question of the bits the emit names. See
[wasm.md](../../docs/impl/wasm.md).

**Signal handling in `rt_call`.** Every host path that runs a native first settles
its result (`ElleHost::settle_native_signal`): a `SIG_QUERY` (`vm/config`, the
`compile/*` primitives) is answered on the driving VM through
`VM::dispatch_query`, as the interpreter and the JIT answer it, and a top-level
`SIG_IO` runs inline. `rt_call` then intercepts two fiber signals: `SIG_RESUME`
(`fiber/resume` → `handle_fiber_resume`) and `SIG_PROPAGATE` (`fiber/propagate`
→ `handle_fiber_propagate`, which re-raises the named child's caught signal as
this call's own). The tail-call host (`rt_prepare_tail_call`) mirrors the RESUME
and PROPAGATE handoffs.

## CPS state-machine transform

Yielding functions become re-entrant via compile-time state machine:
- Closures return `(tag: i64, payload: i64, status: i32)`: status=0 normal, >0 suspended
- `ctx` parameter (param 3) carries resume state on re-entry (0 = initial)
- A park spills the registers and local slots live after its site to ARGS_BASE, calls `rt_yield`, returns suspended
- Resume prologue: if ctx!=0, load every saved slot once, then select the state's case through the dispatch loop
- After a call or host operation that may signal, check the suspended word, spill and return
- Each park site's resume entry delivers the restart value and falls through into the block after the site
- Host snapshots env, manages the parked-frame deque, drives resume chain

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
WASM bytes and reuses a pre-compiled module on a cache hit, skipping the
Wasmtime compile.

## Tiered compilation (lazy WASM)

`--wasm=N` enables tiered execution: the bytecode VM runs by default,
and hot closures are compiled to per-closure WASM modules on demand.

**Constraints on per-closure compilation** — enforced by the
`standalone_emittable` gate in `emit.rs` (`emit_single_closure` returns `None`;
the tiered/precache callers fall back to the VM / full-module dispatch). A
standalone module serves one closure through hosts whose suspension and
tail-call imports are panic stubs ([linker.rs](lazy/linker.rs)) and whose
funcref table has a single entry, so the gate refuses every shape whose
execution would reach one:
- No `TailCall`/`TailCallArrayMut` (`return_call_indirect` needs callee table
  indices + `rt_prepare_tail_call`)
- No function that may signal. An emit, a suspending call, and an error a
  call or host operation raises all park through `rt_yield`'s suspension-frame
  machinery, which a standalone host does not have, so the gate admits only a
  function whose inferred signal is silent
- No `MakeClosure` without module context (`ClosureId` resolution; nested
  closures stay on the bytecode VM)

Pinned by `wasm::tests::gate::standalone_emission_refuses_*`.

**Self-recursive call optimization:** When `rt_call` detects a call to the
same closure currently executing in WASM (same bytecode pointer), it dispatches
directly through the instance's funcref table (`call_indirect` on index 0)
instead of creating a new Store. This makes recursive functions like fib
efficient within a single WASM instance.

## What this tier does not do

- `eval` — dynamic module compilation, which no WASM path has. The Makefile's
  `WASM_SKIP` keeps [eval.lisp](../../tests/lang/eval.lisp)/[eval-env.lisp](../../tests/lang/eval-env.lisp) out of the full-module pass of `make smoke-wasm`.
- Tiered mode creates a `Store` per cross-closure call
  (`call_precached_closure`), so such a call costs a store setup.
- `call_primitive` is imported and never called: the module declaration lists
  it, and every compiled call reaches a primitive through `rt_call`.
