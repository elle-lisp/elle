# Async-First Runtime: Session Notes

## What was done

Rewrote `execute_scheduled` to call `ev/run(thunk)` instead of handling
SIG_IO inline in Rust. User code now runs in a fiber under the async
scheduler from the start.

### Rust changes

**`src/vm/mod.rs`** — `execute_scheduled` generates synthetic bytecode:
```
LoadConst 0 (thunk)    # arg
LoadConst 1 (ev/run)   # func
Call 1
Return
```
This uses the normal `Call` instruction path, which correctly builds
the closure env via `populate_env` (captures first, then params, then
locals) and handles SIG_SWITCH trampolining. Earlier attempt using
`build_closure_call_env` failed because it has the wrong layout
(params first, captures last) — that function only works for zero-arg
closures with no locals (like the stdlib exports closure).

The thunk is a `Value::closure` wrapping user bytecode with
`num_locals: 0, num_captures: 0`. This works because user bytecode
uses the stack for locals (via LoadLocal/StoreLocal), not the closure
env (LoadUpvalue). The thunk's env is empty.

**`src/pipeline/cache.rs`** — Added `lookup_stdlib_value(SymbolId)`
to look up `ev/run` from the compilation cache at runtime.

**`src/primitives/module_init.rs`** — Made `build_closure_call_env`
public (needed for thunk construction, though ultimately not used in
the final approach).

### Elle changes

**`stdlib.lisp`**:
- `make-async-scheduler` returns `{:spawn fn :pump fn :shutdown fn}`
  instead of `(list scheduler-fn pump-fn shutdown-fn)`
- `ev/run` uses `(get sched :spawn)` etc. to destructure
- `ev/spawn` uses `|:error :io :exec|` set literal for fiber mask
- `process-completions` uses `fiber/abort` for I/O errors instead of
  `(error ...)` — fixes `protect` catching I/O errors correctly

### Test changes

- `tests/elle/file_stat.lisp` — Removed `ev/spawn` around
  `subprocess/system` (sequential, no concurrency needed)
- `tests/elle/io.lisp` — Updated `*scheduler*` assertion (now fn?,
  not sync-scheduler); `make-async-scheduler` returns struct; ev/sleep
  error tests use `protect` directly (no nested ev/run)
- `tests/elle/subprocess.lisp` — Removed `ev/spawn` wrappers (I/O
  works directly in async context); concurrent test uses `ev/run`
- `tests/elle/ports.lisp` — Removed `ev/spawn` from protect test
- `tests/elle/http.lisp` — Removed `ev/spawn` from protect test
- `lib/http.lisp` — Removed `ev/spawn` wrappers in test function
  (read-headers, read-body, etc. work directly)

## What's broken

### 1. `ev/spawn` returns fiber, not result

In the async context (which is now always), `ev/spawn` calls
`((*scheduler*) fiber)` — the async scheduler's spawn function, which
pushes the fiber to the runnable queue and returns the fiber handle.
The old sync-scheduler ran the fiber to completion and returned the
result.

Tests and lib/http.lisp still have `ev/spawn` wrappers that expect
return values (streams.lisp wasn't updated, subprocess.lisp partially
updated). These need one of:
- Remove `ev/spawn` — call I/O directly (works for sequential code)
- Use `ev/run` for concurrent cases

### 2. Sequential SIG_IO from coroutines breaks

`port/lines`, `port/chunks` use coroutines internally. The coroutine
yields values, and when the stream consumer calls `stream/read-line`
(which yields SIG_IO), there's a coroutine yield nested inside an
I/O yield. After ~10 such operations, the signal propagation breaks
with "stream/flush: expected port, got integer".

The simplified print functions (directly calling stream/write +
stream/flush without sync-scheduler wrapping) trigger the same bug
after ~10 calls. Print functions are currently kept with
sync-scheduler wrapping as a workaround.

### 3. Nested ev/run state leak

After ~10 sequential `ev/run` calls (each creating and discarding an
async scheduler + io_uring backend), `protect` around a nested
`ev/run` stops catching errors correctly. Independent io_uring rings
shouldn't interfere — the leak is likely in fiber/signal state, not
kernel resources.

## Root cause analysis

All three issues stem from one problem: **SIG_IO propagation through
multi-level fiber suspension chains**.

When code inside a fiber yields SIG_IO:
1. The fiber's mask may or may not catch it
2. If caught → scheduler dispatches I/O, resumes fiber with result
3. If NOT caught → SIG_IO propagates to parent fiber, which catches it
4. Parent suspends, scheduler dispatches I/O
5. On completion, scheduler resumes parent
6. Parent's `resume_suspended` replays FiberResume frames to deliver
   the result back down to the original yielding fiber

Step 6 is where things go wrong. The `resume_suspended` →
`FiberResume` → `do_fiber_resume_single` chain doesn't correctly
deliver I/O results through deep fiber nesting in all cases. The
sync-scheduler avoided this entirely by running I/O inline within
the `fiber/resume` call — no suspension chain, no replay.

## What to investigate next

The signal flow through `handle_sig_switch` → `do_fiber_resume` →
unwind loop → `resume_suspended` needs tracing for the specific case
where:
- User fiber calls `fiber/resume child` (SIG_SWITCH)
- Child does I/O (SIG_IO)
- Child's mask catches SIG_IO
- `do_fiber_resume` returns caught result to caller
- Caller (scheduler pump) dispatches I/O, resumes child
- This happens N times in a loop

After ~10 iterations, the resume value delivered to the child is wrong
(integer instead of expected value). Something in the suspension state
accumulates — likely extra SuspendedFrame entries or stale
FiberResume frames that shift the resume value to the wrong recipient.

## Key architectural insight

The thunk wrapping approach (synthetic bytecode that calls ev/run)
is correct and clean. The Call instruction path handles everything —
env construction, arity checking, SIG_SWITCH trampolining. No special
Rust code needed for closure calling conventions.

The deeper issue is that the sync-scheduler's inline I/O model was
load-bearing in ways the codebase doesn't acknowledge. Moving to
async-first requires the signal machinery to correctly handle I/O
dispatch + resume through arbitrary fiber nesting depths, which it
currently doesn't after sustained use.
