# WIP: WASM Backend I/O + Scheduler Integration

## What's Done

### Code changes (all committed on `wasm-backend` branch)

1. **`src/wasm/host.rs` — `maybe_execute_io`**: Hybrid approach. Inside fibers (`fiber_id_stack` non-empty), propagates SIG_IO so the scheduler can drive it. At top level, executes inline via bound backend or SyncBackend fallback.

2. **`src/wasm/host.rs` — per-fiber suspension frames**: `suspension_frames` is now `HashMap<usize, Vec<WasmSuspensionFrame>>` keyed by `FiberHandle::id()`. `fiber_id_stack: Vec<usize>` tracks the active fiber. Fixes nested coroutine frame corruption.

3. **`src/wasm/host.rs` — `WasmSuspensionFrame::signal_bits`**: New `u32` field carrying the full signal bits at the yield point (SIG_IO, SIG_YIELD, etc.).

4. **`src/wasm/emit.rs` — `rt_yield` 7th parameter**: `signal_bits: i32` passed from WASM to host. Direct Yield terminators pass `2` (SIG_YIELD). Yield-through-call passes `signal_local` (full bits from memory[0..4]).

5. **`src/wasm/emit.rs` — `store_result_with_signal`**: Always writes signal to memory[0..4], even when 0. Prevents stale SIG_YIELD from inner closure yields leaking to callers.

6. **`src/wasm/emit.rs` — entry function**: Type changed to `(ctx: i32) -> (i64, i64, i32)` for CPS re-entry. `local_offset = 1`, `ctx_local = 0`. `may_suspend = false` (scheduler handles I/O internally). Resume prologue condition changed from `is_closure && may_suspend` to just `may_suspend`.

7. **`src/wasm/emit.rs` — `ctx_local` field**: Configurable ctx parameter index (0 for entry, 3 for closures). Used by `emit_resume_prologue` instead of hardcoded `LocalGet(3)`.

8. **`src/wasm/emit.rs` — dual compilation**: Each nested closure is also compiled to bytecode via `lir::Emitter`. Stored in `EmitResult::closure_bytecodes`, passed through to `ElleHost`, used by `rt_make_closure` to populate `ClosureTemplate.bytecode` so `spawn` can execute WASM closures in new threads.

9. **`src/wasm/store.rs` — `handle_fiber_resume`**: Pushes/pops `fiber_id_stack` around call_wasm_closure/resume_wasm_closure. Reads `frame_sig_bits` from `frames.first()` (innermost frame has the original I/O signal; outer frames only have SIG_YIELD because call_wasm_closure strips SIG_IO).

10. **`src/wasm/store.rs` — `call_wasm_closure`**: Clears memory[0..4] on suspension. Returns `SIG_YIELD` (not full signal_bits) as the third return value.

11. **`src/wasm/store.rs` — `run_module`**: Entry function signature updated to `(i32,) -> (i64, i64, i32)`. Has a while loop for re-entry on suspension (currently unused since `may_suspend=false` for entry).

12. **`src/wasm/mod.rs` — ev/run wrapping**: User source wrapped in `(ev/run (fn []\n...\n))`. Epoch directives hoisted before stdlib. `stdlib_form_count` passed to `compile_file_to_lir` for scoped epoch migration.

13. **`src/pipeline/compile.rs` — `epoch_skip` parameter**: `compile_file_to_lir` takes `epoch_skip: usize`. When > 0, epoch migration only applies to forms after that index.

14. **`src/value/fiber.rs` — `FiberHandle::id()`**: Returns `Rc::as_ptr` as usize for stable fiber identity.

15. **`Cargo.toml` + `patches/wasmparser/`**: Local patch raising `MAX_WASM_FUNCTION_SIZE` from 7.6MB to 128MB.

16. **`Makefile`**: smoke-wasm timeout raised from 60s to 180s.

## What Works

- `examples/hello.lisp` — prints "Hello, World!" (I/O goes through scheduler)
- `examples/coroutines.lisp` — all coroutine tests pass (per-fiber frames)
- `examples/portrait.lisp` — parses correctly (newline-separated ev/run wrapping)
- `tests/elle/print-epoch.lisp` — epoch migration scoped to user forms
- `tests/elle/concurrency.lisp` — spawn works via dual-compiled bytecode
- `tests/elle/coroutines.lisp` — wasmparser limit patched (was 7.6MB)
- All 25 examples pass (in the last full smoke-wasm run before the sig_bits fix)
- Most elle scripts pass

## Current Bug: ev/run Hangs After I/O Completes

### Symptom
`hello.lisp` prints "Hello, World!" but the process hangs (never exits). The ev/run scheduler detects SIG_IO, submits I/O, completes it, but the pump loop never terminates.

### Root Cause (hypothesis)
After I/O completes and the fiber is resumed, the scheduler should detect the fiber is `:dead` and exit the pump loop. But the fiber may not reach `:dead` because:

1. **Stale suspension frames**: The yield-through-call chain creates multiple frames per fiber. When the fiber is resumed with the I/O result, `resume_wasm_closure` pops the LAST frame (outermost). But the INNER frames (from the yield-through-call chain) remain. The resume chain loop should consume them, but it may not handle the case where the innermost frame's resume produces another I/O yield.

2. **Resume value delivery**: When the scheduler calls `fiber/resume` with the I/O result, `handle_fiber_resume` (Paused branch) calls `resume_wasm_closure` with that value. But the resume value needs to reach the INNERMOST frame (the one that originally did port/write). If the outermost frame is resumed first and it re-executes the call that yielded, the call_wasm_closure for the inner closure would start fresh instead of resuming.

3. **Infinite I/O loop**: The fiber might re-execute println on resume (instead of continuing past it), triggering another I/O yield, creating an infinite cycle. This would explain the hang after successful output.

### Debugging approach
Trace what happens after the scheduler resumes the fiber with the I/O result:
- Does `resume_wasm_closure` consume all frames?
- Does the fiber return normally (signal=0, status=0)?
- Does handle-fiber-after-resume see `:dead`?

### Key question
The yield-through-call resume model: when the outermost frame is resumed, does the CPS code re-invoke the inner closure call (which would create new frames), or does it treat the resume value as the call's result (consuming the inner frames)?

In the bytecode VM, yield-through-call saves the ENTIRE call stack (frames + operand stack) in `SuspendedFrame`. On resume, the VM restores the full stack and continues from the exact instruction. In WASM CPS, each function is independent — the resume block re-enters the function from the resume state, but inner functions aren't automatically resumed.

This is the fundamental gap: the WASM CPS yield-through-call creates per-function suspension points, but resumption only resumes one function at a time. The inner functions need to be resumed too, which is what the resume chain loop in `handle_fiber_resume` does. But the loop has bugs.

## Files Modified

```
src/wasm/host.rs      — maybe_execute_io, per-fiber frames, signal_bits, ClosureBytecode
src/wasm/store.rs     — rt_yield 7-param, handle_fiber_resume sig_bits, run_module entry sig
src/wasm/emit.rs      — signal_bits param, store_result_with_signal, ctx_local, dual compile
src/wasm/mod.rs       — ev/run wrapping, epoch hoisting, closure_bytecodes passthrough
src/pipeline/compile.rs — epoch_skip parameter
src/value/fiber.rs    — FiberHandle::id()
src/primitives/concurrency.rs — (unchanged, uses dual-compiled bytecode)
Cargo.toml            — wasmparser patch
Makefile              — 180s timeout
patches/wasmparser/   — MAX_WASM_FUNCTION_SIZE = 128MB
tests/wasm_smoke.rs   — epoch_skip parameter
tests/wasm_stdlib.rs  — epoch_skip parameter
```
