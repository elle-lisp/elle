# Runtime Signals

<!-- audited: 2026-10-22 -->

The runtime uses fiber signals for internal coordination. These are
distinct from user-level error handling.

## Runtime signals and capability bits

[protocol.md](signals/protocol.md) owns bits 0 to 10 and the user-defined
range. This document owns bits 11 to 19.

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
:ambient   19    Ambient-state capability
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

### The expansion budget

An expansion is a macro transformer call, or the body of a `begin-for-syntax`
definition. It runs under a budget of 16,777,216 units, which the VM counts
apart from the fuel of every fiber. Each charge site that runs during the
expansion spends one unit of it. That includes a charge site in a silent
function, on a child fiber the body resumes, and in an expansion the body
starts, such as a macro call that `eval` expands. Such a nested expansion
spends the budget of the expansion that started it.

The fiber the expansion runs on spends none of its own fuel until the
expansion ends, and no fuel stop pauses it. A child fiber with a budget of its
own spends both, and pauses as usual when its own budget runs out.

Every closure call during an expansion runs on the interpreter, because
compiled code charges the fuel of a fiber and cannot spend the expansion's.
`compile/run-on` refuses every tier but `:bytecode` there.

A charge site that finds the budget spent halts the expansion. The halt is a
resource fault, like the depth cap's: it passes every mask and the boundary of
every silent function, so a `protect` inside the body does not catch it. The
compile fails, and the error names the macro or the definition
([macros.md](macros.md)).

```lisp
(def [ok? err]
  (protect (eval '(begin (defmacro stubborn [] (begin (protect (forever nil)) :caught))
                         (stubborn)))))
(assert (not ok?) "the halt passes the protect inside the macro")
(assert (string/contains? (get err :message) "macro 'stubborn'"))
```

## SIG_QUERY

A primitive returns `SIG_QUERY` to ask the running VM a question that only
the VM can answer, such as `arena/stats`, `arena/allocs` or `vm/config`.
[query.rs](../src/vm/signal/query.rs) lists every operation.

## SIG_EXEC

`:exec` is a capability bit. A subprocess request carries it beside `:io`:
`:io` routes the request to the scheduler, and `:exec` is the bit a fiber's
mask or `:deny` names to allow or refuse spawning.

## SIG_AMBIENT

`:ambient` is a capability bit. A primitive carries it when its body reads or
changes state that no argument carries and no other bit names: the
environment, the clocks, the VM's configuration and gauges, the standard
ports. A fiber denied it computes only from what it was handed, and every macro
expansion withholds it ([capabilities.md](signals/capabilities.md)).

---

## See also

- [signals](signals/index.md) — signal system design
- [fibers](signals/fibers.md) — fiber architecture
- [scheduler.md](scheduler.md) — async event loop
- [processes.md](processes.md) — fuel-based preemptive scheduling of Erlang-style processes
