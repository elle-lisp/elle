# Janet's Unified Fiber Architecture

A design reference for language designers. Janet unifies coroutines, error
handling, generators, dynamic scoping, and green threads into a single
primitive: the **fiber**. Every interesting control flow event is modeled as a
**signal** propagating up a chain of fibers.

This document describes the architecture as implemented, with commentary on
the design trade-offs.


## The Fiber

A fiber is a self-contained call stack. Its core fields:

| Field | Type | Purpose |
|-------|------|---------|
| `flags` | int32 | Signal masks, status, resume control (triple-duty bitmask) |
| `frame` | int32 | Index into `data` of current stack frame |
| `stacktop` | int32 | Top of stack (push/pop point) |
| `capacity` | int32 | Allocated size of `data` |
| `maxstack` | int32 | Configurable stack overflow limit |
| `env` | Table* | Dynamic bindings table |
| `data` | Value* | The stack memory (heap-allocated, resizable) |
| `child` | Fiber* | Child fiber currently being resumed |
| `last_value` | Value | Last returned/signaled value |

The stack is a flat array of values. Stack frames are interleaved within it,
each preceded by a fixed-size frame header containing the function, program
counter, closure environment, previous frame pointer, and flags. Frames form a
singly-linked list through `prevframe`.

**Design note**: A single contiguous array for the entire call stack (frames +
locals + temporaries) is cache-friendly and avoids per-frame allocation. The
trade-off is that the array may be reallocated on growth, invalidating any raw
pointers into it — the VM must reload its `stack` pointer after any operation
that might grow the stack.


## Signals

Signals are the fundamental communication mechanism. They are integers 0–13:

| Signal | Name | Purpose |
|--------|------|---------|
| 0 | `ok` | Normal return |
| 1 | `error` | Error / panic |
| 2 | `debug` | Debug breakpoint |
| 3 | `yield` | Cooperative yield |
| 4–11 | `user0`–`user7` | User-defined signals |
| 12 | `interrupt` | Interpreter interrupt (cross-thread safe) |
| 13 | `await` | Event loop suspension |

Every fiber operation that transfers control — return, error, yield, debug
break, user signal — goes through the same mechanism: the VM function returns
an integer signal, and the caller decides what to do with it.

**Design note**: Unifying all control flow into a single signal type means
there is exactly one dispatch point for "what happened when I resumed this
fiber." No separate exception type, no separate coroutine protocol, no
separate generator interface. One mechanism, different signal numbers.


## Fiber Status (State Machine)

A fiber's status is encoded in its `flags` field (bits 16–21). There are 16
statuses, directly corresponding to the 14 signals plus `new` and `alive`:

| Status | Keyword | Terminal? | Notes |
|--------|---------|-----------|-------|
| 0 | `:dead` | Yes | Ran to completion |
| 1 | `:error` | Yes | Terminated with error |
| 2 | `:debug` | No | Suspended at breakpoint |
| 3 | `:pending` | No | Yielded |
| 4–8 | `:user0`–`:user4` | Yes | Terminal user signals |
| 9–11 | `:user5`–`:user7` | No | Resumable user signals |
| 12 | `:interrupted` | No | Interrupted externally |
| 13 | `:suspended` | No | Awaiting event loop |
| 14 | `:new` | No | Never run |
| 15 | `:alive` | No | Currently executing |

A fiber can be resumed if and only if its status is not terminal (`dead`,
`error`, or `user0`–`user4`).

**Design note**: The asymmetry between user0–4 (terminal) and user5–9
(resumable) is deliberate. User code can define both "finished with a custom
status" and "paused with a custom status." This is a small, elegant extension
point that costs nothing in the runtime.


## Signal Masks

When a fiber is created, it receives a **signal mask** — a set of bits
indicating which signals it catches from its children. The mask is encoded in
the fiber's `flags` field (bits 0–13).

Janet uses a string-based DSL for specifying masks at creation time:

| Flag | Catches |
|------|---------|
| `a` | All signals |
| `e` | Error |
| `d` | Debug |
| `y` | Yield (this is the default if no mask is given) |
| `t` | Termination signals (error + user0–4) |
| `u` | All user signals (user0–9) |
| `r` | Interrupt (user8) |
| `w` | Await (user9) |
| `0`–`9` | Specific user signal N |
| `i` | Inherit environment (share parent's dynamic bindings table) |
| `p` | Prototype-inherit environment (new table, parent as prototype) |

The mask lives on the **child** fiber, not the parent. When the parent resumes
the child and the child signals, the parent checks the child's mask to decide
whether to catch or propagate. This is slightly counterintuitive — you might
expect the catcher to declare what it catches — but it works because creation
and catching are co-located: the code that creates the fiber with a mask is the
same code that will resume it and handle the result.

**Design note**: Putting the mask on the child means a fiber's error-handling
behavior is fixed at creation time and visible to anyone who inspects it. This
is a form of documentation-in-data. The alternative (mask on the parent, or
mask at resume time) would be more flexible but harder to reason about.


## Resume and Yield

### No Stack Switching

Janet does **not** use coroutine-style C stack switching. When fiber A resumes
fiber B, the VM function `run_vm` is called recursively on the C stack. This
means:

- Each fiber resume adds one C stack frame
- Fiber nesting depth is bounded by C stack depth (a recursion guard)
- The implementation is simple and portable — no platform-specific assembly
- No need for pre-allocated C stacks per fiber

**Design note**: This is a pragmatic trade-off. Stack switching (as in Go,
Lua 5.x with coco, etc.) allows yielding from arbitrary C call depth, but
requires platform-specific code and complicates debugging. Janet's approach
means you cannot yield through a C function boundary (C functions that call
back into Janet use `janet_call`, which coerces all non-OK signals to errors).
This is a hard constraint but keeps the implementation honest.

### The Resume Path

When bytecode `resume` executes:

1. Set `parent.child = child` (link parent to child)
2. Call `run_vm(child, value)` — enters the interpreter loop for the child
3. On return, check the signal against the child's mask:
   - If the mask bit is set → signal is **caught**: clear `parent.child`,
     deliver the value to the parent's destination register
   - If the mask bit is NOT set → signal **propagates**: `parent.child`
     remains set, the parent's `run_vm` also returns with the same signal

### The Yield Path

Bytecode `yield` (signal 3) stores the value in the return register, saves the
program counter, and returns from `run_vm`. The fiber is left in `:pending`
status.

When resumed again, the resume value is placed into the register that was
waiting for the yield's result, and the PC advances past the yield instruction.

### The Child Chain

The `fiber.child` pointer creates a linked list of pending fibers. When a
signal propagates through multiple levels, each fiber retains its `.child`
link, forming a chain from the outermost catching fiber down to the innermost
signaling fiber.

This chain serves two purposes:

1. **Stack traces**: The trace walker follows `.child` links to collect frames
   from all fibers in the chain, producing a complete cross-fiber trace.

2. **Resumption routing**: When a fiber with a pending child is resumed, the
   resume is routed to the deepest child first. The chain unwinds naturally.


## Error Handling

### Two Raising Mechanisms

Janet has two paths for raising errors, converging at the same point:

**Path 1 — Bytecode `error`**: The VM executes `JOP_ERROR`, stores the error
value in the return register, and returns `SIGNAL_ERROR` from `run_vm`. This
is a normal C return — no longjmp, no stack unwinding. Fast and clean.

**Path 2 — C panic (`janet_panic`)**: When C code needs to signal an error, it
calls `janet_panic` → `janet_signalv`, which performs a `longjmp` back to the
nearest `setjmp` point. A `DID_LONGJUMP` flag is set on the fiber so the VM
knows it needs special recovery (popping C function frames, handling
interrupted tail calls).

Both paths land in the same place: `janet_continue_no_check` after `run_vm`
returns, where the fiber's status is set and `last_value` is stored.

**Design note**: The dual-path approach is pragmatic. Bytecode errors avoid
longjmp overhead entirely. C panics get an escape hatch for situations where
returning an error code through multiple C call frames would be impractical.
The `DID_LONGJUMP` flag is the price of this convenience — the VM must handle
the possibility that its state was interrupted mid-operation.

### The Try State Stack

The `setjmp`/`longjmp` context is managed through a `TryState` structure that
lives on the C stack:

```
try_init(state):
  save current signal_buf, return_reg, fiber, stack_depth
  install state.buf as new signal_buf
  install &state.payload as new return_reg

restore(state):
  restore all saved fields
```

Each `continue_no_check` call pushes one `TryState`. A `longjmp` always hits
the innermost `setjmp`. Nesting is naturally managed by C function call
nesting. No heap allocation for try states.

**Design note**: This is an implicit stack threaded through C call frames.
It is elegant and zero-allocation, but it means the try-state lifetime is
tied to C stack frames. This is fine for Janet's execution model where fiber
resumes are always nested.

### Signal Propagation

When a child fiber signals and the parent does not have the corresponding mask
bit set:

1. The parent's `run_vm` returns with the same signal
2. `parent.child` remains pointing to the child (preserving the trace chain)
3. The parent's own parent checks its mask, and so on up the chain
4. Eventually either a fiber catches the signal, or it reaches the top level

When a signal is caught:

1. `parent.child` is set to NULL (chain is broken)
2. The signal value is delivered to the parent's destination register
3. The parent continues executing normally

### `propagate` — Preserving the Chain

The `propagate` operation re-raises a signal from a caught child while
preserving the child fiber link:

```
fiber.child = caught_child_fiber
return(child_status, value)
```

Without `propagate`, catching and re-raising with `error` would lose the
original fiber's stack frames. `propagate` is essential for patterns like
"catch, do cleanup, re-raise with full trace."

### `cancel` — Injecting Errors

The `cancel` operation is the inverse of `resume`: it delivers a value to a
suspended fiber as an **error signal** rather than a normal resumption. It
walks down the child chain to the deepest fiber and injects the error there,
causing the fiber to unwind through its error handlers.

Use cases: timeouts, task cancellation, cooperative shutdown.


## `try`, `protect`, and `defer` — Just Sugar

All error-handling forms are trivially built on fibers:

### `protect`

```
(protect & body)
→ (let [f (fiber/new (fn [] ,;body) :ie)
        r (resume f)]
    [(not= :error (fiber/status f)) r])
```

Creates a fiber with `:ie` (inherit environment + catch errors), resumes it,
returns `[success? value]`.

### `try`

```
(try body [err fib] catch-body)
→ (let [f (fiber/new (fn [] ,body) :ie)
        r (resume f)]
    (if (= (fiber/status f) :error)
      (do (def err r) (def fib f) ,;catch-body)
      r))
```

Same machinery, different result shape. The catch clause can bind both the
error value and the fiber (for stack trace inspection).

### `defer`

```
(defer cleanup-form & body)
→ (do
    (def f (fiber/new (fn [] ,;body) :ti))
    (def r (resume f))
    cleanup-form
    (if (= (fiber/status f) :dead)
      r
      (propagate r f)))
```

Uses `:ti` (catch termination signals + inherit environment). The cleanup form
runs unconditionally. If the body errored, `propagate` re-raises after cleanup.

**Design note**: There is no `finally` keyword, no special VM support for
cleanup. `defer` is a macro that creates a fiber, catches its termination,
runs cleanup, and re-raises. This is the payoff of the unified model — complex
control flow patterns emerge from composition of the single primitive.

### `with`

```
(with [binding ctor dtor] & body)
→ (do
    (def binding ctor)
    (defer [(or dtor :close) binding] ,;body))
```

Resource management is just `defer` with a destructor call as the cleanup form.


## Generators and Iteration

Fibers double as generators through the yield signal:

### `generate`

```
(generate head & body)
→ (fiber/new (fn [] (loop head (yield (do ,;body)))) :yi)
```

Creates a fiber with `:yi` (catch yield + inherit environment). The body loops,
yielding each value.

### The `next` Protocol

When `next` is called on a fiber:

1. Resume the fiber with nil
2. If the fiber yields → return a truthy sentinel (there is a value)
3. If the fiber completes or errors → return nil (end of sequence)

The yielded value is captured separately by the iteration machinery. This means
any fiber that yields values can be iterated with `each`, `map`, `filter`, etc.
No separate iterator interface needed.

**Design note**: Generators are not a separate concept — they are fibers that
happen to yield. A generator can also error (and the error propagates through
the iteration), or be cancelled, or be inspected for its status. The full
fiber API applies.


## Dynamic Bindings

Each fiber has an optional `env` table for dynamic bindings (thread-local-like
scoped state).

- `(dyn :key)` — looks up `:key` in the current fiber's env table
- `(setdyn :key value)` — sets `:key` in the current fiber's env table

### Environment Inheritance

When creating a child fiber:

- **`:i` flag** — the child **shares** the parent's env table. Mutations in
  either are visible to both. Use when the child is logically part of the
  parent's execution context.

- **`:p` flag** — the child gets a **new** table with the parent's table as
  its prototype. The child sees parent bindings via prototype lookup, but
  `setdyn` in the child only modifies the child's own table. Use when the
  child should see but not modify the parent's bindings.

- **No flag** — the child has no env table. `dyn` returns nil.

### `with-dyns`

```
(with-dyns [:key1 val1 :key2 val2] & body)
```

Creates a fiber with `:p`, sets the bindings in the child's env, runs the
body. Dynamic bindings are automatically scoped to the fiber's lifetime.

**Design note**: Dynamic bindings are fiber-scoped, not function-scoped or
block-scoped. This falls naturally out of the fiber model — no additional
mechanism needed. The prototype-chain inheritance via `:p` gives clean scoping
semantics without copying the entire environment.


## Event Loop Integration

When the event loop is enabled, fibers serve as green threads (tasks):

### Task Lifecycle

1. `ev/go` launches a fiber as a task, marking it as a root fiber
2. The event loop tick drains a spawn queue, calling `continue` on each task
3. If a task calls `janet_await`, it signals `await` (user9), suspending the
   fiber until I/O, a timer, or a channel operation wakes it
4. `janet_schedule(fiber, value)` enqueues the fiber for the next tick

### Stale Wakeup Prevention

Each fiber has a `sched_id` counter, incremented on every schedule. When a
task is dequeued, its expected ID is compared to the fiber's current ID. If
they differ, the task is stale and silently skipped. This handles cancellation
and rescheduling races without locks or explicit cancellation tokens.

### Supervisor Channels

When `ev/go` launches a fiber, it can attach a supervisor channel. When the
fiber completes or signals, events are pushed to this channel:

- `[:ok fiber task-id]` — normal completion
- `[:error fiber task-id]` — error (if error is in the fiber's mask)
- `[:user0 fiber task-id]` — user signal (if masked)

If a signal is NOT in the fiber's mask, the stack trace is printed to stderr
instead of being sent to the supervisor. This is the "fail loud by default"
philosophy.

**Design note**: Supervisor channels are similar to Erlang's process monitors.
The mask determines which signals are "expected" (sent to supervisor) vs.
"unexpected" (printed as errors). Task fibers created by `ev/go` default to
masking termination signals, so supervisors receive error events by default.

### The `janet_call` Boundary

C functions that call back into Janet use `janet_call`, which runs on the
current fiber's stack and sets a `coerce_error` flag. This flag converts all
non-OK signals (yield, await, user signals) into errors. You cannot yield or
await through a `janet_call` boundary.

This is a hard constraint: C callback functions are synchronous boundaries.
If a C function needs to participate in cooperative scheduling, it must use
the event loop API directly rather than calling Janet functions synchronously.


## The Interrupt Mechanism

Two distinct mechanisms for pausing execution:

### Breakpoints (Debug Signal)

A breakpoint is a bit (0x80) set on a bytecode instruction. When the VM
dispatches an instruction with this bit set:

1. The fiber is suspended with a `debug` signal
2. Resume-control flags are set so that on resume, the PC stays at the same
   instruction (the breakpointed instruction is re-executed)
3. A parent fiber with the `:d` mask catches this and can inspect the fiber

### Interpreter Interrupt (Interrupt Signal)

An atomic counter (`auto_suspend`) can be incremented from any thread. The VM
checks this counter on every function call, resume, and cancel. If non-zero:

1. The fiber is suspended with an `interrupt` signal (user8)
2. The counter is decremented
3. A parent fiber with the `:r` mask catches this

This is the mechanism for timeouts, SIGINT handling, and cross-thread
cancellation.

**Design note**: Breakpoints are per-instruction (fine-grained, for
debuggers). Interrupts are per-VM (coarse-grained, for external control).
Both produce signals that flow through the same fiber machinery. A debugger
is just a fiber that catches debug signals from its child.


## Protected Call vs. Direct Call (C API)

Two calling conventions for C embedders:

### `janet_call` (Direct)

- Runs on the **current** fiber's stack (pushes a frame)
- Coerces all non-OK signals to errors
- If the call errors, re-panics via longjmp — error propagates upward
- Cannot yield through this boundary
- Use when: calling Janet from C in a context where you want errors to
  propagate naturally

### `janet_pcall` (Protected)

- Creates a **new** fiber with its own stack
- Sets up its own try-state, catching all signals
- Returns the signal as a return value — never panics
- The caller inspects the signal and handles errors explicitly
- Use when: calling Janet from C in a context where you want to handle
  errors yourself (e.g., a REPL, a plugin host)


## Design Principles to Extract

For language designers considering a similar unified model:

### 1. Signals Over Exceptions

Model all non-local control flow as numbered signals on a single axis. Error
is just signal 1. Yield is just signal 3. User-defined signals extend the
space. The dispatch logic is a single bitmask check, not a type hierarchy.

### 2. Masks Over Handlers

Instead of registering exception handlers (which require runtime dispatch
through a handler chain), use bitmasks that determine catch-or-propagate at
a single check point. This is O(1) and branch-predictor-friendly.

### 3. Fibers Over Try/Catch

Error handling boundaries are fiber boundaries. `try` is "create a fiber that
catches errors, resume it, check the result." No special syntax, no special
VM support — just fiber creation with the right mask.

### 4. Composition Over Special Forms

`defer` is a macro. `with` is a macro over `defer`. `try` is a macro.
`protect` is a macro. `generate` is a macro. All built on `fiber/new` +
`resume` + `fiber/status` + `propagate`. The runtime provides one primitive;
the language provides sugar.

### 5. The Child Chain

Maintaining a `fiber.child` link during signal propagation gives you:
- Complete cross-fiber stack traces for free
- Automatic resume routing to the deepest pending fiber
- The ability to re-raise with full context (`propagate`)

This is a small bookkeeping cost with large debugging payoff.

### 6. Dynamic Bindings as Fiber-Scoped State

If fibers are your execution context, dynamic bindings (thread-locals,
context variables) naturally live on the fiber. Prototype-chain inheritance
gives clean scoping. No additional mechanism needed.

### 7. Terminal vs. Resumable Signals

Splitting the signal space into terminal (fiber is done) and resumable (fiber
can continue) gives user code a way to define both completion-with-status and
suspension-with-status. This is a cheap extension point.

### 8. The C Boundary Problem

If your VM uses recursive C calls for fiber resumption (not stack switching),
you cannot yield through C function boundaries. This is a real constraint.
Janet handles it by coercing non-OK signals to errors at C boundaries. This
is honest — it makes the limitation explicit rather than hiding it behind
undefined behavior.

### 9. Cancellation as Error Injection

`cancel` (resume a fiber with an error signal) is a clean cancellation
primitive. The cancelled fiber's error handlers run normally. No special
cancellation protocol needed — errors are the cancellation protocol.

### 10. Stale Wakeup Prevention

If fibers are used as event loop tasks, a monotonic schedule ID on each fiber
elegantly prevents stale wakeups without locks or explicit cancellation tokens.
When a fiber is rescheduled or cancelled, old pending tasks become stale and
are silently dropped.
