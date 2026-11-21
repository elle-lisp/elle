# Debugger

<!-- audited: 2026-11-21 -->

A design for a debugger that pauses a program, shows its state as structured
values, and resumes it.

No phase of the design is complete. It is designed for AI agents first: every
operation is one call that returns data, never a prompt that waits for a
keystroke.

This document is the specification, with three companions: [what a paused
fiber shows](debugger/inspect.md), [how a fiber comes to pause](debugger/pause.md),
and [recording and replay](debugger/replay.md). The status table at the end
records which phases exist. Every primitive, flag and library function these
documents introduce is proposed; a name they mark as existing is one the tree
has today.

## Design principles

1. **Batch over interactive.** The primary verb is "run until a
   condition holds, then return everything" — frames, locals, source
   lines, recent events — as one structured value. Stepping exists,
   but as a loop the driver runs internally, not as a conversation.
2. **The debugger is Elle code.** The debuggee runs as a child fiber.
   The debugger is an ordinary parent fiber that catches `:debug`
   signals, inspects the frozen continuation, and resumes it. This is
   the same shape as the scheduler ([src/stdlib.lisp](../src/stdlib.lisp)), and like the
   scheduler it needs only a small set of Rust primitives.
3. **Structured output.** Every result is an Elle value with a fixed
   schema. Nothing is formatted for a terminal.
4. **Query the past.** Recording captures the program's nondeterministic
   inputs at the primitive layer. Replay recomputes any past state on
   demand. An agent asks questions about history instead of guessing
   breakpoint locations.

## Architecture

```text
scheduler fiber
  └─ debugger fiber        mask |:error :io :exec :wait| — the scheduler catches these
       └─ debuggee fiber   mask |:debug :fuel :error|    — the debugger catches these
```

A mask lives on the child and names the signals its parent catches
([src/vm/fiber/catch.rs](../src/vm/fiber/catch.rs)). The catch test is bit
*overlap*, not subset, with one carve-out in `mask_catches` there: no mask
absorbs a signal that carries the VM-internal `SIG_TERMINAL` bit. An I/O
request raises `:io` alone, and a subprocess request adds `:exec`. The
primitive's *static* signature adds `:error`, but an error is an alternative
return, not a bit the request carries.

So the debuggee's `:error` bit does not trap a request. The request passes
through the suspending debugger to the scheduler, whose mask on the debugger
names `:io`. The existing `FiberResume` chain
([src/vm/fiber/resume.rs](../src/vm/fiber/resume.rs)) re-delivers the
completion to the debuggee. Signal routing needs no new rules, and
[tests/lang/io-mask.lisp](../tests/lang/io-mask.lisp) pins the pass-through
end to end.

A paused debuggee is a resumable snapshot: `fiber.suspended` holds the
parked frame chain ([src/value/fiber/frame.rs](../src/value/fiber/frame.rs)). The debugger reads it
through the inspection primitives below and never mutates it.

## The `:debug` signal

`:debug` is signal bit 2. It is already registered
([src/signals/mod.rs](../src/signals/mod.rs), [src/signals/registry.rs](../src/signals/registry.rs)),
classified `SignalAction::Suspend` ([src/signals/dispatch.rs](../src/signals/dispatch.rs)),
and routed by fiber masks.

The LIR lowerer already treats `:debug` as suspending. A call that may emit it
compiles to `SuspendingCall`, which gets a continuation frame
([src/lir/lower/control/call.rs](../src/lir/lower/control/call.rs); the
trigger set is `:yield|:debug|:io|:wait`). At every suspend site the runtime
moves the activation's owner node into the parked frame, so parked state stays
owned across the pause.

Region inference is *more* conservative, not less. A fiber-body lambda whose
signal carries a suspending bit cannot take the transferred-return ownership
cut. So a thunk that calls `debug/break` gives up that optimization and
nothing else. What is missing is producers. The debugger adds two, plus the
rules below.

**Producers.** The first is the `debug/break` primitive. It registers beside
the existing `debug/*` primitives in
[src/primitives/debug.rs](../src/primitives/debug.rs), with
`Signal::of(SIG_DEBUG.union(SIG_ERROR))`; the `primitive!` tables are `const`
items, and `.union` is the `const` form of `|`. The second is the dynamic
breakpoint check in the dispatch loop.

**Two flags, not one.** "Attached" is a VM-level flag the driver sets
for the session; it makes `debug/break` pause instead of returning
`nil`. The *debug flag* is a per-fiber field (new — `Fiber` has no
flags word today); it turns on per-instruction fuel, the breakpoint
check, and the interpreter-only tier gate. `debug:launch` sets both.

**No catcher, no pause.** When the VM is not attached, `debug/break`
returns `nil` without suspending. A breakpoint in production code is
inert.

**Deniable.** `:debug` sits inside `CAP_MASK`, which is defined by excluding
the VM-internal bits ([src/signals/mod.rs](../src/signals/mod.rs)), and bit 2
is not excluded. There is no `fiber/deny` primitive. Denial is the `:deny`
argument to `fiber/new`, and a fiber's descendants inherit it. Every native
dispatch checks a primitive's declared bits against the fiber's withheld set:
the interpreter's call and tail call, the JIT's call and array call, and the
WASM host. [tests/lang/caps.lisp](../tests/lang/caps.lisp) pins the payload.

So `(fiber/new f mask :deny |:debug|)` denies `debug/break` through the
standard check, and a supervisor can forbid debugging of untrusted code. This
is the third behavior mode, beside pause (attached) and inert `nil`
(detached). The denial reaches a bare `(emit :debug v)` as well, because an
`emit` needs the bits it names ([emit](signals/emit.md)):

```lisp
(let [f (fiber/new (fn [] (emit :debug 1)) |:debug :error| :deny |:debug|)]
  (assert (= (get (fiber/resume f) :denied) |:debug|) "a denied fiber emits no :debug"))
```

**Transparent to signal hygiene.** Two enforcement points must let a
tooling pause through:

- Squelch/attune enforcement asks one predicate, `signals::squelched_bits`
  ([src/signals/squelch.rs](../src/signals/squelch.rs)), from one method.
  Every tier's call, tail-call and sentinel site reaches it through
  `VM::boundary_violation` ([src/vm/core/discard.rs](../src/vm/core/discard.rs)),
  directly or through `VM::enforce_boundary`. One predicate keeps the
  exemptions from drifting apart between tiers. It exempts `:error` and
  `:halt` by intersection, `:switch` by exact equality, and any signal that
  carries the pause bit `:fuel` (`SIG_PAUSE`) whole.

  A dynamic breakpoint's `:debug|:fuel` therefore passes every boundary, a
  squelch of `:debug` included. A compiled-in `debug/break` raises plain
  `:debug`, which is the code's own emit and stays enforceable. A boundary
  that instead converts a tooling pause into a signal-violation discards the
  suspended frames. It leaves the fiber paused and resumable over the
  wreckage, and a resume re-executes the interrupted instruction against a
  torn-down stack.

  [tests/lang/squelch-fuel.lisp](../tests/lang/squelch-fuel.lisp) pins the
  rule for `:fuel`, one case per charge-site shape, and for a pause that
  carries a second bit. `(squelch :fuel f)` is inert, because metering is the
  parent's action, not the closure's behavior, so the boundary has nothing to
  enforce.
- `CheckSignalBound`, the `(bound param spec)` parameter bound, is a bind-time
  check on closure values, not a runtime signal filter. Binding a closure to a
  bounded parameter compares the closure's effective signal against the bound,
  and errors on any excess
  ([src/vm/dispatch/interp/signals.rs](../src/vm/dispatch/interp/signals.rs)).
  The JIT asks the same question, and the WASM tier drops the instruction.
  Phase 2 makes the check subtract `:debug` from the excess, so a closure that
  contains `debug/break` still binds to a bounded parameter. Without that,
  adding a breakpoint to a function passed as a bounded argument turns a
  running program into a bind-time signal violation.

No tooling pause arises inside an inferred-silent function. A silent function
is a proof that nothing stops the fiber, and the native tiers run one with no
way to park it. So a silent function spends fuel but never pauses, the
interpreter checks no breakpoint while one runs, and a signal that leaves one
is a broken proof ([signals/inference.md](signals/inference.md)). A step passes over a silent
call whole, and a breakpoint armed inside one never fires.

Rationale: `:debug` is a tooling channel, not program behavior. Mask
routing still applies — only a fiber whose mask names `:debug`
catches it.

**Tiers.** Signal inference does *not* keep `debug/break` off the
JIT. Single-function JIT compiles suspending functions and side-exits
at the suspension, deoptimizing the native frame into a
`BytecodeFrame` ([src/jit/suspend.rs](../src/jit/suspend.rs)). A compiled-in breakpoint
therefore pauses with inspectable frames even in JIT'd code, and
resumes interpreted. Stepping and dynamic breakpoints exist only in the
interpreter and need the debug flag (see Tier interactions below).

## The driver library: `lib/debug.lisp`

The driver owns the session: it spawns the debuggee fiber with the
right mask and debug flag, arms the VM's attached flag, catches
`:debug`/`:fuel`/`:error`, and packages every pause as one snapshot
value.

| Function | Purpose |
|----------|---------|
| `debug:launch thunk &named breakpoints step-mode` | start a session; returns it paused at entry |
| `debug:continue session &opt value` | resume; returns the next outcome |
| `debug:step session n` | run `n` instructions |
| `debug:step-line session` | run to the next source line |
| `debug:until session pred` | auto-continue past pauses until `(pred snapshot)` is true |
| `debug:snapshot session` | frames, trace, status, signal, payload — the full state |
| `debug:eval session frame-index f` | apply `f` to the frame's locals struct |
| `debug:break-at session closure line` | arm a dynamic breakpoint |
| `debug:finish session` | run to completion; returns the final outcome |

An outcome struct:

```text
{:kind    :break | :breakpoint | :step | :done | :error
 :payload <break payload, error value, or return value>
 :snapshot {...}}    # present for pauses; the same value debug:snapshot returns
```

`:break` is a `debug/break` pause and carries the user's payload.
`:breakpoint` and `:step` carry driver-synthesized payloads, since
the VM-side pause payload is `nil`. An `:error` snapshot carries every
activation's frame, because an error parks the whole chain
([what a paused fiber shows](debugger/inspect.md)), and `:trace` beside them.

`debug:until` is the batch verb from principle 1: one call subsumes
an arbitrary number of pauses, and only the terminating snapshot
returns to the caller. `debug:eval` is read-only in v1. It binds the
snapshot's local values, reading celled locals through their env cells, and
it does not write into the parked frame.

## MCP surface

The Elle MCP server ([mcp.md](mcp.md)) gains session tools backed by
`lib/debug` and the persistent image: `debug_launch`, `debug_until`,
`debug_snapshot`, `debug_eval`, `debug_replay`. A session is a UUID
handle like `eval`'s value handles. The implementation lives in the
`mcp` submodule, which is a repository of its own. The tool schemas mirror the driver
functions one to one.

## Tier interactions

A fiber whose debug flag is set never enters native code. `call_inner`
skips the WASM, MLIR, and JIT entries for a flagged fiber, and the
forced-tier entries (`compile/run-on`, [src/vm/run_on/](../src/vm/run_on)) fall back to
the interpreter for it.

The other native dispatch path, the JIT-to-JIT fast path in `elle_jit_call`,
runs only *inside* native frames, and a flagged fiber acquires none. The flag
can change only while the fiber is not running, because a running fiber's
handle slot is empty and fibers are single-threaded. A suspended native frame
has already deoptimized into a bytecode frame, so resumption is interpreted.
This closes the bypass set by construction rather than by auditing each
dispatch path.

Compiled-in `debug/break` does not need the flag to *pause*: the JIT
compiles suspending functions and side-exits into interpreter-shaped
frames at the suspension, so the pause is inspectable and resume runs
interpreted. It does need the flag for stepping and dynamic
breakpoints, which native code never checks. MLIR refuses a function
that calls anything, so a `debug/break` caller is MLIR-ineligible
already. Fibers without the flag are unaffected; their JIT'd frames
remain opaque to inspection while running, as today.

## Implementation status

Documentation, then tests, then code — each phase lands its tests
before its implementation.

| Phase | Contents | Status |
|-------|----------|--------|
| 1 | name plumbing (HIR → `LirHead::name` → the template name), `local_names` with three-shape places and parameter entries, the `Bytecode` local count, `fiber/frames`, `fiber/trace`, `fiber/disasm`, `Fresh` region rule, disasm exhaustiveness | name plumbing built; the rest not started |
| 2 | `debug/break`, attached flag, hygiene exemptions (parameter bounds), denial semantics, JIT side-exit inspectability | not started |
| 3 | fiber debug + skip-once fields, owning-key breakpoint table, `debug/break-at`, composed-bit pauses, per-instruction fuel, always-park re-execute frames, tier gate | not started |
| 4 | `lib/debug.lisp` driver, snapshot/outcome schemas | not started |
| 5 | identity order (allocation-sequence ordering for reference-identity values), `PrimitiveDef.replay` classes + registry exhaustiveness test, deterministic tier promotion under recording, record/replay mode flag, thread/callback refusal, log schema, `debug:replay`, `debug:bisect` | not started |
| 6 | MCP session tools (out of tree) | not started |

### Test obligations per phase

1. Phase 1:
   - The frames of a yielded fiber carry correct names, locations and
     locals, and a local's name matches its `let` binding.
   - A celled local reads its value through its env cell, and a
     compiled-cell binding reads through the cell in its slot.
   - A parameter appears in `:locals`, and the scratch slot never appears.
   - A top-level pause shows top-level locals and resolves file and line
     (the `Bytecode` local count).
   - A tail-call run renders one frame, and a `FiberResume` entry renders
     and recurses.
   - An errored fiber's frames hold every activation.
   - Inspect, drop the fiber, then use the value: nothing crashes.
   - Inspecting an `:alive` fiber errors without panicking.
   - A fiber denied `:inspect` cannot call `fiber/frames`.
   - Every opcode that carries an operand round-trips through the
     disassembler at the correct width.
2. Phase 2:
   - A parent masked `|:debug|` catches a break and reads the payload, and
     the resume value becomes `debug/break`'s result.
   - With no debugger, `debug/break` answers `nil` and does not suspend.
   - `:deny |:debug|` produces a capability denial.
   - A break inside `squelch` preserves the continuation.
   - A `(bound param ||)` bind accepts a closure containing `debug/break`.
   - A function containing `debug/break` reports `(silent? f)` false.
   - A break inside a JIT-compiled function pauses with inspectable frames.
3. Phase 3:
   - `fiber/set-fuel 1` under the debug flag advances exactly one
     instruction.
   - A pause at an instruction with an empty operand stack parks and
     resumes correctly.
   - A step pause under the debug flag inside a `squelch` boundary passes
     through, which extends
     [tests/lang/squelch-fuel.lisp](../tests/lang/squelch-fuel.lisp).
   - A `debug/break-at` line pause resolves to that line's first
     instruction, and a resume past a breakpoint does not re-trigger it
     (skip-once).
   - A dynamic breakpoint armed inside an inferred-silent function never
     fires, and a step passes over a silent call whole.
   - The same closure called from a fiber without the flag takes the JIT
     path.
4. Phase 4, end-to-end Elle tests:
   - `debug:until` runs over a loop, and `debug:step-line` steps across
     calls.
   - `debug:eval` reads a plain local and a celled local.
5. Phase 5:
   - Every registered primitive carries a replay class (registry
     exhaustiveness).
   - A recorded run with I/O replays to an identical event log.
   - A replay in a separate process reproduces `hash` values and
     closure-set iteration order.
   - A retroactive breakpoint during replay observes the same values and
     the same order.
   - `debug:bisect` finds a planted invariant violation.
   - Recording refuses `sys/spawn` and `ffi/callback`.

---

## See also

- [signals/fibers.md](signals/fibers.md) — fiber architecture, suspension frames
- [signals/primitives.md](signals/primitives.md) — resume semantics, swap protocol
- [runtime.md](runtime.md) — fuel budgets, signal bits
- [impl/vm.md](impl/vm.md) — dispatch loop, executing-closure register
- [analysis/debugging.md](analysis/debugging.md) — existing introspection toolkit
- [mcp.md](mcp.md) — the Elle MCP server
