# SIG_SWITCH trampoline — resume notes

## Branch

`feature/fiber-trampoline-switch-v2` based on `main` (c1604b0c).

## What this commit fixes

### 1. hfrs frame save (`fiber.rs`)

`handle_fiber_resume_signal` now saves a `SuspendedFrame::Bytecode` for the caller's
continuation. Previously it returned `Some(SIG_SWITCH)` without saving any frame.

**Why this is needed:** `call_inner` returns `handle_primitive_signal`'s result directly
for native functions (line 155 of call.rs). Its own frame-saving code (lines 316+, for
closures) is never reached for native fn calls like `coro/resume`. Without hfrs saving
a frame, `handle_sig_switch` gets empty `caller_frames` and can't resume execution after
the fiber completes.

### 2. JIT `exec_result_to_jit_value` (`jit/calls.rs`)

Changed from a `match` on `SIG_OK | SIG_HALT` and `SIG_YIELD` to a general check for
any suspending signal. Previously, SIG_SWITCH fell through to the catch-all `_ =>` branch,
which returned `JitValue::nil()` — silently dropping the signal and continuing execution
as if the call succeeded.

### 3. JIT `elle_jit_has_signal` (`jit/suspend.rs`)

Changed from checking `SIG_ERROR | SIG_HALT | SIG_YIELD` to checking any non-OK signal.
The JIT emits a `has_signal` check after Call instructions in yielding functions. Without
detecting SIG_SWITCH, the JIT code continues with the YIELD_SENTINEL return value
(0xDEAD_CAFE_DEAD_CAFE) as if it were a regular value, corrupting data structures.

### 4. JIT `elle_jit_call` signal check (`jit/calls.rs`)

The JIT-to-JIT fast path's signal check after calling a callee was hardcoded to
`matches!(vm.fiber.signal, Some((SIG_YIELD, _)))`. Changed to check any non-OK,
non-error, non-halt signal. Same issue as #3 but for the JIT-to-JIT dispatch path.

## Current state

**Passing (with JIT):** `contracts.lisp`, `errors.lisp`, `http.lisp`, all stream
operations except `stream/zip`.

**Failing:** `stream/zip` in `streams.lisp` — panics with "Upvalue index out of bounds"
when sources have 3+ elements. Only fails with JIT enabled. Root cause: JIT frame
chain gap (see below).

## BLOCKER: JIT frame chain gap

When a JIT-compiled function (e.g. stdlib `map`) calls a closure via `elle_jit_call`'s
interpreter fallback, and that closure triggers SIG_SWITCH (via `coro/resume` → hfrs),
the JIT function side-exits correctly. But the **JIT function's own continuation frame
is never saved**.

The frame chain ends up as `[callback_frame, zip_body_frame]` — missing the map frame
in between. When the fiber is later resumed, map's continuation is lost. The callback's
return value flows directly to the zip body frame, skipping map's list-building logic.

### Why this happens

The interpreter path for closures: call_inner calls `execute_bytecode_saving_stack`,
which saves/restores the caller's stack. When the callee signals SIG_SWITCH, call_inner
saves a caller frame (with the CALLER's bytecode/env/ip). Each nesting level saves its
own frame.

The JIT path: `elle_jit_call` calls the callback via `execute_bytecode_saving_stack` (interpreter
fallback). The callback signals SIG_SWITCH. `elle_jit_call` returns YIELD_SENTINEL to the JIT
code. The JIT code side-exits. `run_jit` returns `Some(SIG_SWITCH)` to call_inner. call_inner's
JIT suspend path saves the OUTER caller frame (zip body). But nobody saves a frame for the
JIT-compiled map's continuation — the JIT function's state is lost on side-exit.

### Fix approach

Option A: Have `elle_jit_call`'s interpreter fallback save a caller frame (for the JIT
function's continuation) before returning YIELD_SENTINEL. This mirrors what call_inner
does for closures in the interpreter path.

Option B: On JIT side-exit for SIG_SWITCH, fall back to the interpreter to re-execute
the JIT function from its saved bytecode. This would require saving the JIT function's
ip and stack state on side-exit.

Option A is simpler. The frame would use the JIT function's bytecode/constants/env
(available from the closure passed to `elle_jit_call`) and an ip derived from the
current execution point.

## Test commands

```bash
cargo run -- tests/elle/contracts.lisp
cargo run -- tests/elle/streams.lisp
cargo run -- examples/errors.lisp
cargo run -- examples/http.lisp
make smoke
ELLE_DEBUG_RESUME=1 cargo run -- <file>   # fiber resume tracing
```

## Files changed

- `src/vm/fiber.rs` — hfrs frame save; handle_fiber_resume_signal_jit inline resume (unchanged)
- `src/vm/call.rs` — debug instrumentation removed
- `src/vm/execute.rs` — execute_bytecode_from_ip inner stack capture (unchanged from trampoline PR)
- `src/vm/jit_entry.rs` — JIT tail-call SIG_SWITCH handling (unchanged from trampoline PR)
- `src/vm/mod.rs` — handle_sig_switch (unchanged from trampoline PR)
- `src/jit/calls.rs` — exec_result_to_jit_value + elle_jit_call signal checks
- `src/jit/suspend.rs` — elle_jit_has_signal broadened
