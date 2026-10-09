# Debugger

<!-- audited: 2026-10-06 -->

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
([src/vm/fiber/catch.rs](../src/vm/fiber/catch.rs)). The catch test is bit *overlap*, not
subset, with one carve-out in `mask_catches` there: a signal carrying
the VM-internal `SIG_TERMINAL` bit is never absorbed by any mask. An I/O request is
emitted as `:io` alone — the primitive's *static* signature adds
`:error`, but an error is an alternative return, not a co-emitted
bit, and a subprocess request adds `:exec`. So the debuggee's
`:error` bit does not trap a request. The request passes through the
suspending debugger to the scheduler, whose mask on the debugger
names `:io`. The existing `FiberResume` chain
([src/vm/fiber/resume.rs](../src/vm/fiber/resume.rs)) re-delivers the completion to the
debuggee. Signal routing needs no new rules;
[tests/lang/io-mask.lisp](../tests/lang/io-mask.lisp) pins the pass-through end to end.

A paused debuggee is a resumable snapshot: `fiber.suspended` holds the
parked frame chain ([src/value/fiber/frame.rs](../src/value/fiber/frame.rs)). The debugger reads it
through the inspection primitives below and never mutates it.

## The `:debug` signal

`:debug` is signal bit 2. It is already registered
([src/signals/mod.rs](../src/signals/mod.rs), [src/signals/registry.rs](../src/signals/registry.rs)),
classified `SignalAction::Suspend` ([src/signals/dispatch.rs](../src/signals/dispatch.rs)),
and routed by fiber masks. The LIR lowerer
already treats it as suspending — a call that may emit `:debug`
compiles to `SuspendingCall`, which gets a continuation frame
([src/lir/lower/control/call.rs](../src/lir/lower/control/call.rs); the trigger set is
`:yield|:debug|:io|:wait`) — and at every suspend site the runtime
moves the activation's owner node into the parked frame, so parked
state stays owned across the pause. Region inference is *more*
conservative, not less: a fiber-body lambda whose signal carries a
suspending bit is disqualified from the transferred-return ownership
cut, so a thunk that calls `debug/break` gives up that optimization
and nothing else. What is missing is producers. The debugger adds
two, plus the rules below.

**Producers.** The `debug/break` primitive, registered beside the
existing `debug/*` primitives in [src/primitives/debug.rs](../src/primitives/debug.rs) with
`Signal::of(SIG_DEBUG.union(SIG_ERROR))` (the `primitive!` tables
are `const` items; `.union` is the `const` form of `|`), and the
dynamic breakpoint check in the dispatch loop.

**Two flags, not one.** "Attached" is a VM-level flag the driver sets
for the session; it makes `debug/break` pause instead of returning
`nil`. The *debug flag* is a per-fiber field (new — `Fiber` has no
flags word today); it turns on per-instruction fuel, the breakpoint
check, and the interpreter-only tier gate. `debug:launch` sets both.

**No catcher, no pause.** When the VM is not attached, `debug/break`
returns `nil` without suspending. A breakpoint in production code is
inert.

**Deniable.** `:debug` sits inside `CAP_MASK` — the mask is defined
by excluding the VM-internal bits ([src/signals/mod.rs](../src/signals/mod.rs)), and bit 2
is not excluded. There is no `fiber/deny` primitive: denial is the
`:deny` argument to `fiber/new`, inherited by descendants, and every
primitive call checks `signal.bits ∩ withheld ∩ CAP_MASK` at every native
dispatch (interpreter call and tail call, JIT call and array call, the WASM host;
[tests/lang/caps.lisp](../tests/lang/caps.lisp) pins the payload). So
`(fiber/new f mask :deny |:debug|)` denies `debug/break` through the
standard check, and a supervisor can forbid debugging of untrusted
code. This is the third behavior mode, beside pause (attached) and
inert `nil` (detached). Denial gates the primitive only: a bare
`(emit :debug v)` still raises the bit, because `emit` is not
capability-checked. Denial is a policy statement about `debug/break`,
not an information barrier.

**Transparent to signal hygiene.** Three enforcement points must
exempt tooling pauses:

- Squelch/attune enforcement asks one predicate,
  `signals::squelched_bits` ([src/signals/mod.rs](../src/signals/mod.rs)), at all ten sites:
  the interpreter's `enforce_squelch` ([src/vm/core/discard.rs](../src/vm/core/discard.rs)) serves six,
  and four more are inlined in the JIT paths — two in
  [src/vm/run_on/jit.rs](../src/vm/run_on/jit.rs) and two in [src/jit/calls/callops.rs](../src/jit/calls/callops.rs). One
  predicate is what keeps the exemptions from drifting apart between
  tiers. It exempts `:error` and `:halt` by intersection, `:switch` by
  exact equality, and subtracts the pause bits (`SIG_PAUSE`). A
  boundary that instead converts a tooling pause into a
  signal-violation discards the suspended frames and leaves the fiber
  paused-but-resumable over the wreckage — resume re-executes the
  interrupted instruction against a torn-down stack.
  [tests/lang/squelch-fuel.lisp](../tests/lang/squelch-fuel.lisp) pins
  the rule for `:fuel`, one case per charge-site shape. `:debug` joins `SIG_PAUSE`
  with the debug pause itself; the constant is the single place to add
  it. `(squelch f :fuel)` is inert — metering is the parent's action,
  not the closure's behavior, so the boundary has nothing to enforce.
- The silence enforcement ([src/vm/execute/nested.rs](../src/vm/execute/nested.rs)) kills the process
  with `std::process::abort` when a statically-silent closure
  produces any signal. It has no exemption list today; it gains one:
  pause bits within `:debug|:fuel` pass. It is the interpreter's
  check alone — no JIT, WASM, or MLIR equivalent exists — and one
  site suffices: a flagged fiber runs interpreted, so a dynamic pause
  can only surface there. Compiled-in `debug/break` does not need the
  exemption — inference marks its callers non-silent.
- `CheckSignalBound` — the `(silence param)` parameter bound — is a
  bind-time check on closure values, not a runtime signal filter.
  Binding a closure to a bounded parameter compares the closure's
  static `signal.bits` against the bound and errors on any excess
  ([src/vm/dispatch/interp/signals.rs](../src/vm/dispatch/interp/signals.rs); the JIT mirrors it; the WASM
  tier currently drops the instruction). The check subtracts `:debug`
  from the excess, so a closure that contains `debug/break` still
  binds to a silence-bounded parameter. Without that, adding a
  breakpoint to a function passed as a silenced argument turns a
  running program into a bind-time signal violation.

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
the VM-side pause payload is `nil`. `:error` snapshots include
`:trace` (and full frames once Phase 3 error parking lands).

`debug:until` is the batch verb from principle 1: one call subsumes
an arbitrary number of pauses, and only the terminating snapshot
returns to the caller. `debug:eval` is read-only in v1 — it binds the
snapshot's local values (reading celled locals through their env
cells), it does not write into the parked frame.

## MCP surface

The Elle MCP server ([mcp.md](mcp.md)) gains session tools backed by
`lib/debug` and the persistent image: `debug_launch`, `debug_until`,
`debug_snapshot`, `debug_eval`, `debug_replay`. A session is a UUID
handle like `eval`'s value handles. The implementation lives in the
`mcp` submodule and is out of scope for this repository; the tool
schemas mirror the driver functions one to one.

## Tier interactions

A fiber whose debug flag is set never enters native code. `call_inner`
skips the WASM, MLIR, and JIT entries for a flagged fiber, and the
forced-tier entries (`compile/run-on`, [src/vm/run_on/](../src/vm/run_on)) fall back to
the interpreter for it. The other native dispatch path, the JIT-to-JIT
fast path in `elle_jit_call`, runs only *inside* native frames, and a
flagged fiber acquires none: the flag can change only while the fiber
is not running (a running fiber's handle slot is empty, and fibers
are single-threaded), and a suspended native frame has already
deoptimized into a bytecode frame, so resumption is interpreted. This
closes the bypass set by construction rather than by auditing each
dispatch path.

Compiled-in `debug/break` does not need the flag to *pause*: the JIT
compiles suspending functions and side-exits into interpreter-shaped
frames at the suspension, so the pause is inspectable and resume runs
interpreted. It does need the flag for stepping and dynamic
breakpoints, which native code never checks. MLIR refuses functions
with non-error signals, so a `debug/break` caller is MLIR-ineligible
already. Fibers without the flag are unaffected; their JIT'd frames
remain opaque to inspection while running, as today.

## Implementation status

Documentation, then tests, then code — each phase lands its tests
before its implementation.

| Phase | Contents | Status |
|-------|----------|--------|
| 1 | name plumbing (HIR → `LirHead::name` → the template name), `local_names` with three-shape places and parameter entries, the `Bytecode` local count, `fiber/frames`, `fiber/trace`, `fiber/disasm`, `Fresh` region rule, disasm exhaustiveness | name plumbing built; the rest not started |
| 2 | `debug/break`, attached flag, hygiene exemptions (`:debug` joins `SIG_PAUSE`; silence; silence bounds), denial semantics, JIT side-exit inspectability | not started |
| 3 | fiber debug + skip-once fields, owning-key breakpoint table, `debug/break-at`, composed-bit pauses, per-instruction fuel, always-park re-execute frames, tier gate, error-path frame preservation | not started |
| 4 | `lib/debug.lisp` driver, snapshot/outcome schemas | not started |
| 5 | identity order (allocation-sequence ordering for reference-identity values), `PrimitiveDef.replay` classes + registry exhaustiveness test, deterministic tier promotion under recording, record/replay mode flag, thread/callback refusal, log schema, `debug:replay`, `debug:bisect` | not started |
| 6 | MCP session tools (out of tree) | not started |

### Test obligations per phase

1. Frames of a yielded fiber carry correct names, locations, locals;
   a local's name matches its `let` binding; a celled local reads its
   value through its env cell; a compiled-cell binding reads through
   the cell in its slot; a parameter appears in `:locals`; the
   scratch slot never appears; a top-level pause shows top-level
   locals and resolves file and line (the `Bytecode` local
   count); a tail-call run renders one frame; a `FiberResume` entry
   renders and recurses; inspect → drop fiber → use value does not
   crash; `:alive` inspection errors without panicking; every
   operand-bearing opcode round-trips through the disassembler at the
   correct width.
2. Parent with `|:debug|` catches a break and reads the payload;
   resume value becomes `debug/break`'s result; no debugger → `nil`,
   no suspension; `:deny |:debug|` produces a capability denial;
   break inside `squelch` preserves the continuation; a
   `(silence param)` bind accepts a closure containing `debug/break`;
   a function containing `debug/break` reports `(silent? f)` false; a
   break inside a JIT-compiled function pauses with inspectable
   frames.
3. `fiber/set-fuel 1` under the debug flag advances exactly one
   instruction; a pause at an instruction with an empty operand stack
   parks and resumes correctly; a step pause under the debug flag
   inside a `squelch` boundary passes through (extends the pinned
   [tests/lang/squelch-fuel.lisp](../tests/lang/squelch-fuel.lisp)); a `debug/break-at`
   line pause resolves to that line's first instruction; resume past
   a breakpoint does not re-trigger it (skip-once); a dynamic break
   inside an inferred-silent function pauses instead of aborting; the
   same closure called from a non-flagged fiber takes the JIT path;
   an error under the debug flag exposes every activation's frame.
4. End-to-end Elle tests: `debug:until` over a loop,
   `debug:step-line` across calls, `debug:eval` reads a plain local
   and a celled local.
5. Every registered primitive carries a replay class (registry
   exhaustiveness); a recorded run with I/O replays to an identical
   event log; a replay in a separate process reproduces `hash` values
   and closure-set iteration order; a retroactive breakpoint during
   replay observes the same values and the same order;
   `debug:bisect` finds a planted invariant violation; recording
   refuses `sys/spawn` and `ffi/callback`.

---

## See also

- [signals/fibers.md](signals/fibers.md) — fiber architecture, suspension frames
- [signals/primitives.md](signals/primitives.md) — resume semantics, swap protocol
- [runtime.md](runtime.md) — fuel budgets, signal bits
- [impl/vm.md](impl/vm.md) — dispatch loop, executing-closure register
- [analysis/debugging.md](analysis/debugging.md) — existing introspection toolkit
- [mcp.md](mcp.md) — the Elle MCP server
