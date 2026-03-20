# Signal propagation height limit

## Summary

Sequential yielding function calls overflow the runtime's signal
propagation chain after a small number of iterations (~8–10 Redis
round trips). The error is:

```
fiber/resume: cannot propagate signal (no parent fiber to catch it)
```

This is not specific to Redis. Any code that makes many sequential TCP
round trips through a function call chain hits it. Raw `stream/write`
+ `stream/read-line` in a `while` loop works for 50+ iterations; the
same operations wrapped in a function (`redis:command`) overflow at
~10.

## Reproduction

Minimal repro — fails at 10 iterations, succeeds at 9:

```lisp
(def redis ((import-file "lib/redis.lisp")))

(ev/run (fn []
  (let [[conn (redis:connect "127.0.0.1" 6379)]]
    (var i 0)
    (while (< i 10)
      (redis:ping conn)
      (assign i (+ i 1)))
    (redis:close conn))))
```

Same operations inlined — works at 50+:

```lisp
(ev/spawn (fn []
  (let [[conn (redis:connect "127.0.0.1" 6379)]]
    (var i 0)
    (while (< i 50)
      (stream/write conn:port "*1\r\n$4\r\nPING\r\n")
      (stream/flush conn:port)
      (stream/read-line conn:port)
      (assign i (+ i 1)))
    (port/close conn:port))))
```

The difference: the inline version has 3 yields per iteration directly
in the loop body. The `redis:ping` version calls through `redis-ping`
→ `redis-command` → `assert-conn` + `resp-encode` + `stream/write` +
`stream/flush` + `resp-read` (which calls `stream/read-line` + `case`).
Each function body is an implicit `begin` with multiple expressions.

## What does NOT cause the overflow

- **Bytecode `Begin` depth.** `lower_begin` in `src/lir/lower/expr.rs`
  compiles `Begin(exprs)` as a flat sequence: lower each expr, discard
  intermediate results. The bytecode is flat, not nested.

- **The scheduler event loop.** Both the sync scheduler
  (`stdlib.lisp:945–960`) and the async scheduler (`make-async-scheduler`,
  `stdlib.lisp:972–1069`) are iterative: `forever { drain-runnable;
  process-completions }`. They do not recurse.

- **`while` compilation.** `analyze_while` (`src/hir/analyze/forms.rs:561`)
  wraps the loop in a `Block` with a `While` node. Multi-expression
  bodies get an implicit `Begin`, but this compiles to flat bytecode.

## What DOES cause the overflow

### Observation 1: top-level accumulation

The budget is shared across the entire file's top-level body. Adding
more `ev/run` calls or `ev/spawn` calls at the top level eats into
it, even if each individual call is small.

With 2 `import-file` calls at the top level, the budget is roughly:
- 8 `ev/run` blocks × 3 redis commands each = passes
- 8 `ev/run` blocks × 5 redis commands each = fails

The total is ~40 signal propagation frames across the whole file.

### Observation 2: function calls do not reset depth

Wrapping `ev/run` calls in `defn` and calling those functions does
not reduce the accumulated depth. The signal propagation height grows
through function call boundaries.

### Observation 3: `while` loops accumulate too

A `while` loop calling `redis:ping conn` overflows at 10 iterations.
The same `while` loop calling raw `stream/write` + `stream/flush` +
`stream/read-line` works for 50+. So the depth is per-yield-through-
function-chain, not per-yield.

### Observation 4: the stack trace is in the scheduler

The repeating stack frames are at `stdlib:1060` (`drain-runnable`) and
`stdlib:1068` (`process-completions`). These are inside `pump-fn`, the
async scheduler's event loop. They alternate:

```
stdlib:1060  drain-runnable
stdlib:1068  process-completions
stdlib:1060  drain-runnable
stdlib:1068  process-completions
... (82+ more frames)
```

This is the `forever` loop body in `pump-fn`:

```lisp
(forever
  (drain-runnable)              # line 1060
  (when (= (length pending) 0)
    (break :loop nil))
  ...
  (process-completions))        # line 1068
```

The loop is syntactically iterative. But the stack trace shows these
calls NESTED, not flat. This means the Rust VM is not actually
returning between iterations — it's accumulating Rust call stack
frames across loop iterations.

### Observation 5: the trigger is at root fiber boundary

The error at `src/vm/fiber.rs:208–214`:

```rust
if self.current_fiber_handle.is_none() && !result_bits.contains(SIG_ERROR) {
    set_error(&mut self.fiber, "state-error",
        "fiber/resume: cannot propagate signal (no parent fiber to catch it)");
```

A non-error signal (SIG_IO) reaches the root fiber with no parent.
This happens when the signal propagation chain exceeds the available
depth, and the scheduler's fiber can no longer catch the signal.

## Hypothesis

The Rust VM's `fiber_resume` method does not return between iterations
of the `forever`/`while` loop in the scheduler. Each `fiber/resume`
call within the loop body suspends the current Rust call frame (for the
scheduler fiber) and enters the child fiber. When the child yields
SIG_IO, the Rust stack unwinds back to the scheduler. But the scheduler
is itself a fiber being resumed by the top-level execution context.

The `pump-fn` closure runs inside a fiber (created by `ev/run` →
`ev/spawn`). That fiber is resumed by the sync scheduler at the top
level. Each `forever` iteration doesn't truly "loop" at the Rust level
— the bytecode `While` instruction re-enters the Rust `execute` loop,
but each `fiber/resume` within that execution adds a Rust stack frame
that persists for the duration of the child fiber's execution.

Concretely: `pump-fn` runs → calls `drain-runnable` → calls
`fiber/resume user-fiber` → user fiber calls `redis:command` →
`redis:command` calls `stream/write` → `stream/write` yields SIG_IO
→ signal propagates back to pump-fn's fiber → pump-fn's fiber
propagates to its parent (the sync scheduler) → sync scheduler does
`io/submit` → resumes pump-fn fiber → pump-fn continues `forever`
loop → calls `process-completions` → reads completion → calls
`fiber/resume user-fiber` → ...

Each cycle through this chain adds Rust stack depth because the
`fiber/resume` calls are nested within the Rust execution of the
outer fiber. The scheduler's `forever` loop appears iterative in Elle
bytecode but is recursive in the Rust call stack because `fiber/resume`
is a recursive Rust function call.

## Impact

- Any library wrapping TCP I/O in functions is limited to ~8–9 round
  trips per `ev/run` block.
- Tests must be structured with very few operations per block.
- Real applications doing sequential I/O in a loop (e.g., processing
  a queue, polling) will hit the limit quickly.
- The limit does NOT affect raw I/O (stream/* calls directly in a
  loop), only I/O through function call chains.

## Cost per operation (approximate)

Each `redis:command` call consumes ~4 signal propagation frames:
- `redis-command` function body (3 exprs: assert-conn, write, read)
- `resp-encode` function body
- `resp-read` function body (case dispatch + parsing)
- Plus the `redis:set`/`redis:ping` wrapper itself

Total file budget: ~40 frames. With connect + flushdb consuming ~8
frames, that leaves ~32 frames ÷ 4 per command ≈ 8 commands.

## Relevant source locations

| Location | What |
|----------|------|
| `src/vm/fiber.rs:208–214` | Error site: root fiber with uncaught non-error signal |
| `src/vm/fiber.rs:190–220` | `fiber_resume` signal propagation logic |
| `src/lir/lower/expr.rs:199–253` | `lower_begin` — flat bytecode (not the cause) |
| `src/hir/analyze/forms.rs:396–441` | `analyze_begin` — HIR construction |
| `src/hir/analyze/forms.rs:561–612` | `analyze_while` — implicit begin for multi-expr body |
| `stdlib.lisp:945–960` | Sync scheduler (iterative, used by `ev/spawn`) |
| `stdlib.lisp:972–1092` | Async scheduler + `ev/run` |
| `stdlib.lisp:996–1001` | `drain-runnable` (line 1060 in stack traces) |
| `stdlib.lisp:1003–1015` | `process-completions` (line 1068 in stack traces) |
| `stdlib.lisp:1057–1068` | `pump-fn` forever loop |

## Possible fix directions

1. **Trampoline fiber/resume.** Instead of recursive Rust calls for
   `fiber/resume` within the VM, use a trampoline pattern: return a
   "resume this fiber" action to the caller, letting the Rust call
   stack unwind between fiber transitions.

2. **Increase Rust stack size.** If the default thread stack is the
   bottleneck, increasing it would raise the limit but not eliminate it.

3. **Flatten the scheduler.** If `pump-fn` could avoid calling
   `fiber/resume` from within a fiber (i.e., run the event loop at
   the Rust level rather than in Elle code), the Rust stack would
   be flat.

4. **Detect and report clearly.** At minimum, detect the approaching
   limit and produce a clear "signal propagation depth exceeded" error
   instead of the confusing "no parent fiber to catch it" message.
