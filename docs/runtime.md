# Runtime Signals

The runtime uses fiber signals for internal coordination. These are
distinct from user-level error handling.

## Built-in signals

```text
Signal     Bit   Purpose
───────────────────────────────────────────
:error      0    Error propagation
:yield      1    Coroutine yield
:debug      2    Debugger breakpoints
:ffi        4    FFI callbacks
:halt       8    Fiber termination
:io         9    Async I/O completion
:exec      11    Subprocess completion
:fuel      12    Instruction budget exhaustion
:switch    13    Context switch
:wait      14    Blocking wait
```

User-defined signals (via `(signal :keyword)`) get bits 16–31.

## Fuel budgets

Fuel limits instruction execution on a fiber. When fuel runs out,
the fiber pauses with a `:fuel` signal.

```lisp
(def f (fiber/new |:fuel :yield| (fn [] (while true (yield :tick)))))
(fiber/set-fuel f 1000)    # instruction budget
(fiber/resume f nil)       # runs until fuel exhausted
(fiber/fuel f)             # => 0 (exhausted)
(fiber/set-fuel f 10000)   # refuel
(fiber/resume f nil)       # resume execution
(fiber/clear-fuel f)       # remove budget, unlimited execution
```

## SIG_QUERY

`SIG_QUERY` requests introspection from a fiber without disrupting
its execution. Used by `arena/count`, `arena/stats`, and other
introspection primitives.

## SIG_EXEC

Signals subprocess completion. Used by the async scheduler when a
`subprocess/exec` process finishes.

---

## See also

- [signals](signals/index.md) — signal system design
- [fibers](signals/fibers.md) — fiber architecture
- [scheduler.md](scheduler.md) — async event loop
- [processes.md](processes.md) — fuel-based preemptive scheduling of Erlang-style processes
