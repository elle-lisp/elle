# Runtime Signals

<!-- audited: 2026-09-28 -->

The runtime uses fiber signals for internal coordination. These are
distinct from user-level error handling.

## Runtime signals and capability bits

[protocol.md](signals/protocol.md) owns bits 0 to 10 and the user-defined
range. This document owns bits 11 to 17.

```text
Signal     Bit   Purpose
───────────────────────────────────────────
:exec      11    Subprocess capability
:fuel      12    Instruction budget exhaustion
switch     13    VM-internal fiber switch trampoline; no keyword
:wait      14    Structured-concurrency wait request
:gpu       15    GPU hardware dispatch capability
:os-signal 16    POSIX signal send/raise capability
:fs        17    Filesystem access capability
```

## Fuel budgets

Fuel limits instruction execution on a fiber. When fuel runs out,
the fiber pauses with a `:fuel` signal.

```lisp
(def f (fiber/new (fn [] (while true (yield :tick))) |:fuel :yield|))
(fiber/set-fuel f 1000)    # instruction budget
(fiber/resume f nil)       # runs until fuel exhausted
(fiber/fuel f)             # => 0 (exhausted)
(fiber/set-fuel f 10000)   # refuel
(fiber/resume f nil)       # resume execution
(fiber/clear-fuel f)       # remove budget, unlimited execution
```

## SIG_QUERY

A primitive returns `SIG_QUERY` to ask the running VM a question that only
the VM can answer, such as `arena/stats`, `arena/allocs` or `vm/config`.
[query.rs](../src/vm/signal/query.rs) lists every operation.

## SIG_EXEC

`:exec` is a capability bit. A subprocess request carries it beside `:io`:
`:io` routes the request to the scheduler, and `:exec` is the bit a fiber's
mask or `:deny` names to allow or refuse spawning.

---

## See also

- [signals](signals/index.md) — signal system design
- [fibers](signals/fibers.md) — fiber architecture
- [scheduler.md](scheduler.md) — async event loop
- [processes.md](processes.md) — fuel-based preemptive scheduling of Erlang-style processes
