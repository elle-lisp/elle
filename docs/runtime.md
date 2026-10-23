# Runtime Signals

<!-- audited: 2026-10-22 -->

The runtime uses fiber signals for internal coordination. These are
distinct from user-level error handling.

## Runtime signals and capability bits

[protocol.md](signals/protocol.md) owns bits 0 to 10 and the user-defined
range. This document owns bits 11 to 18.

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
:net       18    Network access capability
```

## Fuel budgets

Fuel limits instruction execution on a fiber. When fuel runs out,
the fiber pauses with a `:fuel` signal. Each call and each backward jump costs
one unit, in the interpreter and in JIT code alike, so a loop pauses in
compiled code as it does in the interpreter. A silent function spends fuel but
never pauses, because it is a proof that nothing stops the fiber. The pause
comes at the next charge site outside it
([signals/inference.md](signals/inference.md)). The MLIR and WASM tiers charge
nothing.

```lisp
(def f (fiber/new (fn [] (def @n 0) (while (< n 100000) (assign n (+ n 1))) n) |:fuel|))
(fiber/set-fuel f 1000)    # instruction budget
(fiber/resume f nil)       # runs until the fuel runs out
(assert (= :paused (fiber/status f)))
(assert (= 0 (fiber/fuel f)))
(fiber/set-fuel f 10000)   # refuel
(fiber/resume f nil)       # runs on, and pauses again
(fiber/clear-fuel f)       # remove the budget
(assert (= 100000 (fiber/resume f nil)) "with no budget the loop finishes")
```

A budget can be any non-negative integer:

```lisp
(def g (fiber/new (fn [] 1) |:fuel|))
(fiber/set-fuel g 9223372036854775807)   # the largest integer
(assert (= (fiber/fuel g) 9223372036854775807))
```

### The kill word

Every charge site reads one more word before it charges: the VM's kill word.
A budget below the word ends the fiber with the terminal error
`{:error :killed}` in place of the charge. The word starts at zero, which no
budget is below. `sys/kill` sets a worker's word to the largest 64-bit value,
which every budget is below ([threads](threads.md)). An unmetered fiber holds
a word above the largest budget and below that value, so a kill reaches it as
it reaches a metered one. The check runs inside a silent function as outside
one, because the error carries `:terminal`, and a terminal signal passes the
boundary of a silent function ([inference](signals/inference.md)). The MLIR,
SPIR-V and WASM tiers carry no check, so a kill lands when control returns to
a tier that does.

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
