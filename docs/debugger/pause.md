# Debugger: how a fiber comes to pause

<!-- audited: 2026-09-28 -->

Breakpoints compiled into a program or armed in a running one, and stepping by
fuel.

This document is part of the [debugger design](../debugger.md), and every name
it introduces is proposed unless it says the tree has it today.

## Breakpoints

### Compiled-in: `debug/break`

`(debug/break payload)` suspends the fiber with `SIG_DEBUG`, and the
parent sees the payload. The parent reads `(fiber/value f)` to get
`{:kind :break :value payload}`. The value passed to the resuming
`fiber/resume` becomes `debug/break`'s return value — the standard
resume-value flow. With no debugger attached, `debug/break` returns
`nil` immediately. A fiber denied `:debug` gets the standard
capability denial instead.

### Dynamic: the breakpoint table

Dynamic breakpoints pause code that was compiled without any
instrumentation. The VM holds a table keyed by bytecode identity
whose entry *owns* an `Rc` clone of the bytecode plus the set of
armed offsets. The owning clone pins the allocation, so the key
cannot be freed and reissued to unrelated code while a breakpoint is
armed. The JIT cache keys the same pointer without an owner and never
evicts; the table must not copy that shape.

| Primitive | Signature | Purpose |
|-----------|-----------|---------|
| `debug/break-at` | `(closure line) → array` | set breakpoints; returns the ips armed |
| `debug/clear` | `(closure line?) → nil` | clear one line, or all for the closure |

`debug/break-at` resolves a line to the lowest bytecode offset whose
`location_map` entry names it — a scan, since the table is sorted by offset
and not by line.
Resolution covers only the given closure's own code; the driver walks
`child_protos` when the caller wants a whole definition. A line that
exists only in macro-generated code has no entries; the returned ip
array is empty and the driver reports it.

**The check.** When a fiber's debug flag is set, the dispatch loop
consults the table before executing each instruction, beside the
loop's existing unconditional per-instruction work — the fiber-signal
check and the allocation-error take ([src/vm/dispatch/interp.rs](../../src/vm/dispatch/interp.rs)).
The locals-integrity assertion nearby exists only in debug builds and
is not the anchor. On a hit, the VM emits `:debug|:fuel` with a `nil`
payload and exits the loop at the *unexecuted* opcode's ip.

**Pause protocol.** The composed bits are the protocol. The three
park sites that can park a re-execute pause compute
`push_resume_value = !bits.intersects(SIG_FUEL)`
([src/vm/fiber/resume.rs](../../src/vm/fiber/resume.rs), [src/vm/core/resume.rs](../../src/vm/core/resume.rs),
[src/vm/call/inner/park.rs](../../src/vm/call/inner/park.rs)); the other suspend sites park emit/yield
frames and hardcode the push. The `:fuel` bit therefore selects
re-execute semantics — the paused instruction has not run and runs on resume —
with no edits to those sites. The `:debug` bit routes the pause to
the debugger and distinguishes a breakpoint from a plain step pause
(`:fuel` alone). Compiled-in `debug/break` emits plain `:debug`
through the `Emit` instruction, whose frames push the resume value;
the two producers never share a park decision. The payload is `nil`
because building a payload struct at the pause site would allocate
into the debuggee's regions; the driver synthesizes
`{:kind :breakpoint :file f :line l :ip i :fn name}` from the parked
frame instead.

**Empty-stack parks.** `park_suspended_callee_frame`
([src/vm/call/inner/park.rs](../../src/vm/call/inner/park.rs)) parks the interrupted callee only when it is
the innermost pause (no deeper frame already parked) *and* its
operand stack is non-empty; a re-execute pause that loses its frame
resumes by injecting `nil` into the caller. Fuel's seven charge sites
make that near-unreachable today; per-instruction charging makes it
routine. The guard changes: a re-execute pause always parks its
frame, empty stack or not. The innermost-pause conjunct stays.

**Skip-once.** Re-executing the paused instruction would hit the same
breakpoint forever. On resume, the fiber holds the breakpoint ip it
paused at; the check skips exactly that ip once, then clears the
record. The record is a second new `Fiber` field beside the debug
flag, so it rides the fiber through swaps and parks.

**Cost when detached.** The debug flag is per fiber and defaults off.
The loop already reads per-fiber state every iteration; the flag is
one more predictable branch on the same state. If measurement shows
the branch matters, the fallback is a second loop variant selected at
frame entry; the specification does not require either
implementation.

## Stepping

Fuel is the step engine. It already provides exact, resumable,
per-fiber pausing (`charge_fuel`,
[src/vm/dispatch/interp/opcodes.rs](../../src/vm/dispatch/interp/opcodes.rs)): at zero, the fiber suspends
with `:fuel`, the ip points at the unexecuted opcode, and resume
re-executes it exactly. The pause payload is `nil`; the driver
synthesizes the outcome.

Today fuel is charged at seven interpreter sites: backward `Jump`,
the four call opcodes, and the two array-call opcodes
([src/vm/dispatch/interp/opcodes.rs](../../src/vm/dispatch/interp/opcodes.rs)). Conditional jumps never
charge, and no native tier charges at all. When a fiber's debug flag
is set, the loop charges fuel on **every** instruction, at the same
loop-top site as the breakpoint check. Instruction-granular stepping
is then `(fiber/set-fuel f n)`, an existing primitive, followed by
`(fiber/resume f nil)`, which runs exactly `n` instructions and pauses
with `:fuel`.

Fuel is not refilled on resume — a zero-fuel resume re-pauses at the
same ip — so the driver sets fuel before every step, as
[tests/lang/fuel.lisp](../../tests/lang/fuel.lisp)'s driver-loop scenario pins.

**Fuel ownership.** A fiber has one fuel register, and the parent
that meters the fiber owns it. `debug:launch` creates the debuggee,
so the driver owns its register by construction. A fiber some other
scheduler already meters — a process under the quantum in
[lib/process.lisp](../../lib/process.lisp) — cannot also be stepped; debugging inside a
process host is future work. `:fuel` is VM-internal to signal
inference: no function is ever marked as emitting it, so step mode
changes no function's inferred signal or tier eligibility.

**Pause provenance is advisory.** `emit` bakes its bits into the
instruction and is not capability-checked, so a program can emit
`:fuel` or `:debug` itself. A forged pause differs from a meter pause
in two observable ways: the fiber's fuel register is nonzero
(`fiber/fuel`), and an emit park pushes the resume value where a
meter park re-executes. The driver classifies pauses with these
signals but does not treat the classification as a security
boundary; the debugger's authority is being the parent.

The debuggee's mask names `:fuel`, so the pause lands in the
debugger. Line stepping is a driver loop: step instructions until the
frame's resolved line — the nearest preceding `location_map` entry —
changes. Step-over and step-out compare `fiber/frames` depth between
pauses; a tail call keeps depth constant, so stepping stays "in"
through tail calls, consistent with their constant-stack semantics.
None of these need Rust support.

