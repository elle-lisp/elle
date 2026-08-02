# Debugger

The debugger pauses a program, exposes its state as structured values,
and resumes it. It is designed for AI agents first: every operation is
one call that returns data, never a prompt that waits for a keystroke.

This document is the specification. The [status table](#implementation-status)
at the end records which phases exist.

## Design principles

1. **Batch over interactive.** The primary verb is "run until a
   condition holds, then return everything" — frames, locals, source
   lines, recent events — as one structured value. Stepping exists,
   but as a loop the driver runs internally, not as a conversation.
2. **The debugger is Elle code.** The debuggee runs as a child fiber.
   The debugger is an ordinary parent fiber that catches `:debug`
   signals, inspects the frozen continuation, and resumes it. This is
   the same shape as the scheduler (`src/stdlib.lisp`), and like the
   scheduler it needs only a small set of Rust primitives.
3. **Structured output.** Every result is an Elle value with a fixed
   schema. Nothing is formatted for a terminal.
4. **Query the past.** Recording captures the program's nondeterministic
   inputs. Replay recomputes any past state on demand. An agent asks
   questions about history instead of guessing breakpoint locations.

## Architecture

```text
scheduler fiber                 (mask of debugger: |:error :io :exec :wait|)
  └─ debugger fiber             runs lib/debug driver loop
       └─ debuggee fiber        (mask: |:debug :fuel :error|)
```

The debuggee's mask routes `:debug`, `:fuel`, and `:error` to the
debugger. The mask does not name `:io`, so an I/O request passes
through the debugger to the scheduler untouched — the debugger fiber
suspends with it and the existing `FiberResume` chain re-delivers the
completion to the debuggee. Signal routing needs no new rules; the one
rule in `src/vm/fiber/catch.rs` already covers this topology.

A paused debuggee is a complete, resumable snapshot: its
`fiber.suspended` chain holds one `BytecodeFrame` per activation, each
with code, environment, instruction pointer, and operand stack
(`src/value/fiber/frame.rs`). The debugger reads these frames through
the inspection primitives below and never mutates them.

## The `:debug` signal

`:debug` is signal bit 2 (`src/signals/mod.rs`). It is registered,
classified as `SignalAction::Suspend`, and routed by fiber masks like
any other signal. The debugger adds producers and three semantic rules.

**Producers.** Two: the `debug/break` primitive, and the dynamic
breakpoint check in the dispatch loop.

**No catcher, no pause.** When no debugger is attached, `debug/break`
returns `nil` without suspending. "Attached" is a VM flag that the
driver sets for the session. A breakpoint in production code is inert.

**Transparent to signal hygiene.** `:debug` joins `:error` and `:halt`
in the exemption lists of both `enforce_squelch` and the silence
enforcement (`src/vm/core.rs`, `src/vm/call/inner.rs`). Rationale:
`:debug` is a tooling channel, not program behavior. Without the
squelch exemption, a breakpoint inside a `squelch` boundary would
discard the suspended frames and destroy the continuation. Mask
routing still applies — only a fiber whose mask names `:debug` catches it.

**Forces the interpreter tier.** `debug/break` registers as a
primitive that may emit `:debug`, so signal inference marks every
caller `may_suspend()`. The JIT already rejects suspending functions,
so a function with a compiled-in breakpoint runs interpreted — the
only tier where frames are inspectable. Dynamic breakpoints get the
same guarantee from the per-fiber debug flag (see
[Tier interactions](#tier-interactions)).

## Debug information

The runtime already carries most of what inspection needs:

| Table | Where | Maps |
|-------|-------|------|
| `location_map` | `Code`, `CallFrame` | bytecode offset → file, line, column |
| `syntax` | `ClosureTemplate` | the lambda's full surface AST |
| `lir_function` | `ClosureTemplate` | SSA, CFG, yield points, call sites |

Two additions, both on `Code` (`src/value/code.rs`), flowing the same
path as `location_map`:

- **`name: Option<Rc<str>>`** — the function name, copied from
  `ClosureTemplate.name` when the `Code` is built.
- **`local_names: Rc<Vec<Option<Rc<str>>>>`** — local slot index →
  binding name. The emitter populates it from HIR binding data, which
  currently dies before emit. When a slot is reused by shadowing or
  scope collapse, the table holds the last binding's name; an
  ip-ranged table is future work.

These ride `Code` rather than the parked executing-closure register
because a parked register is a possibly-dead borrow — inspection must
never dereference it (see the register invariants in
[impl/vm.md](impl/vm.md)). `BytecodeFrame.code` is always live, so
everything inspection needs hangs off it.

## Inspection primitives

Inspection operates on a fiber value whose status is `:new`,
`:suspended`, `:dead`, or `:error`. Inspecting an `:alive` fiber is an
error; use `fiber/self` and a `:debug` pause to inspect the current one.

| Primitive | Signature | Returns |
|-----------|-----------|---------|
| `fiber/frames` | `(fiber) → array` | frame structs, innermost first |
| `fiber/disasm` | `(fiber index) → array` | disassembly lines for frame `index` |

A frame struct:

```text
{:name     "worker"          # Code.name, or "<toplevel>"
 :file     "src/thing.lisp"  # from location_map at :ip
 :line     42
 :col      7
 :ip       118               # bytecode offset
 :locals   [("acc" 10) ("xs" [1 2 3])]   # (name . value) pairs, slot order
 :stack    [...]}            # operand stack above the locals
```

Locals occupy stack slots `[0, reserved_locals)` of the parked frame's
stack — closure bodies execute on a fresh stack with base 0, and
`Code::reserved_locals` records the count. Unnamed slots (compiler
temporaries) render with a `nil` name.

**Region rule.** Every value an inspection primitive hands out is
incref'd in its region, exactly like the terminal-result pin in the
fiber swap protocol ([signals/primitives.md](signals/primitives.md)).
Without the pin, the caller's release of the returned value could free
a region the paused fiber still owns. Tests must cover
inspect-then-drop-fiber-then-use ordering.

`fiber/disasm` wires the existing disassembler
(`src/compiler/bytecode/disasm.rs`) to a frame's `Code`, one string
per instruction, with a `*` marker on the frame's current `ip`.

## Breakpoints

### Compiled-in: `debug/break`

```text
(debug/break payload)   # suspend with SIG_DEBUG; parent sees payload
```

The parent reads `(fiber/value f)` to get
`{:kind :break :value payload}`. The value passed to the resuming
`fiber/resume` becomes `debug/break`'s return value — the standard
resume-value flow. With no debugger attached, `debug/break` returns
`nil` immediately.

### Dynamic: the breakpoint table

Dynamic breakpoints pause code that was compiled without any
instrumentation. The VM holds a breakpoint table keyed by bytecode
identity (`*const u8`, the same key the JIT cache uses) mapping to a
set of instruction offsets.

| Primitive | Signature | Purpose |
|-----------|-----------|---------|
| `debug/break-at` | `(closure line) → array` | set breakpoints; returns the ips armed |
| `debug/clear` | `(closure line?) → nil` | clear one line, or all for the closure |

`debug/break-at` resolves a line through the closure's `location_map`
in reverse: the lowest bytecode offset whose entry names that line.
Resolution covers only the given closure's own code; the driver
library walks child protos when the caller wants a whole definition.

**The check.** When a fiber's debug flag is set, the dispatch loop
consults the table before executing each instruction
(`src/vm/dispatch/interp.rs`, beside the existing per-instruction
debug assertion). On a hit, the VM emits `:debug` with payload
`{:kind :breakpoint :file f :line l :ip i :fn name}` and suspends
with `push_resume_value = false` — the paused instruction has not
executed and re-executes on resume, the same protocol fuel uses.

**Skip-once.** Re-executing the paused instruction would hit the same
breakpoint forever. On resume, the fiber records the breakpoint ip it
paused at; the check skips exactly that ip once, then clears the record.

**Cost when detached.** The debug flag is per fiber and defaults off.
The check is one predictable branch on a flag the loop already has in
cache. If measurement shows the branch matters, the fallback is a
second loop variant selected at frame entry; the specification does
not require either implementation.

## Stepping

Fuel is the step engine. It already provides exact, resumable,
per-fiber pausing (`charge_fuel`,
`src/vm/dispatch/interp/opcodes.rs`): at zero, the fiber suspends with
`:fuel`, the ip points at the unexecuted opcode, and resume re-executes
it exactly.

Today fuel is charged only on backward jumps and calls. When a fiber's
debug flag is set, the loop charges fuel on **every** instruction.
Instruction-granular stepping is then:

```text
(fiber/set-fuel f n)      # existing primitive
(fiber/resume f nil)      # runs exactly n instructions, pauses with :fuel
```

The debuggee's mask names `:fuel`, so the pause lands in the debugger.
Line stepping is a driver loop: step instructions until the frame's
resolved line changes. Step-over and step-out compare `fiber/frames`
depth between pauses. None of these need Rust support.

## The driver library: `lib/debug.lisp`

The driver owns the session: it spawns the debuggee fiber with the
right mask, arms the VM's attached flag, catches `:debug`/`:fuel`/
`:error`, and packages every pause as one snapshot value.

| Function | Purpose |
|----------|---------|
| `debug:launch thunk &named breakpoints step-mode` | start a session; returns it paused at entry |
| `debug:continue session &opt value` | resume; returns the next outcome |
| `debug:step session n` | run `n` instructions |
| `debug:step-line session` | run to the next source line |
| `debug:until session pred` | auto-continue past pauses until `(pred snapshot)` is true |
| `debug:snapshot session` | frames, status, signal, payload — the full state |
| `debug:eval session frame-index f` | apply `f` to the frame's locals struct |
| `debug:break-at session closure line` | arm a dynamic breakpoint |
| `debug:finish session` | run to completion; returns the final outcome |

An outcome struct:

```text
{:kind    :break | :fuel | :done | :error
 :payload <breakpoint payload, error value, or return value>
 :snapshot {...}}    # present for pauses; the same value debug:snapshot returns
```

`debug:until` is the batch verb from principle 1: one call subsumes an
arbitrary number of pauses, and only the terminating snapshot returns
to the caller. `debug:eval` is read-only in v1 — it binds the
snapshot's local values, it does not write into the parked frame.

## Execution history

Recording and replay are specified now so the instrumentation points
are designed in; they ship as a later phase.

### What gets recorded

Fibers are single-threaded and cooperative, so there are no thread
interleavings. All external nondeterminism enters through an
enumerable set of seams:

| Class | Seam | Recorded value |
|-------|------|----------------|
| I/O, timers, subprocess, futex | scheduler resume | (fiber id, signal bits, resume value), in delivery order |
| Clocks | `clock/monotonic`, `clock/realtime`, `clock/cpu` | the float returned |
| Environment | `os/*` reads | the value returned |
| FFI | `ffi/*` calls | the result, replayed as a stub — foreign code is never re-called |
| OS threads | `sys/spawn` channels | out of scope for v1; recording refuses or scopes to one VM |

The scheduler is Elle code, so the primary recorder is an Elle wrapper
around its resume sites. Primitive interception (clocks, FFI) is a
record/replay mode flag those primitives consult.

### The log and the clock

The log is a flat array of event structs:

```text
{:t [segment fuel] :ev :resume :fiber 3 :bits 512 :value <io result>}
{:t [segment fuel] :ev :break  :file "x.lisp" :line 9 :ip 44}
```

The clock is a pair. `segment` counts scheduler deliveries — the
coarse, cheap coordinate recorded live. `fuel` addresses a point
inside a segment: replay to the segment start, set the debug flag and
`fiber/set-fuel k`, resume. Recording never meters instructions; only
a replay that needs an intra-segment stop pays for step mode.

### Replay

`debug:replay log &named until` re-runs the program feeding recorded
values back at each seam, in order. Execution between seams is
deterministic, so the replay is bit-for-bit the original run. Every
debugger feature works during replay — which yields **retroactive
breakpoints**: choose the predicate after the crash, replay with it
armed, and observation cannot perturb the bug.

`debug:bisect log invariant` binary-searches the clock for the first
point where `invariant` fails on the snapshot: log₂(N) replays, each
one batch call. This is the primary agent workflow the design serves.

## MCP surface

The Elle MCP server ([mcp.md](mcp.md)) gains session tools backed by
`lib/debug` and the persistent image: `debug_launch`, `debug_until`,
`debug_snapshot`, `debug_eval`, `debug_replay`. A session is a UUID
handle like `eval`'s value handles. The implementation lives in the
`mcp` submodule and is out of scope for this repository; the tool
schemas mirror the driver functions one to one.

## Tier interactions

A fiber whose debug flag is set dispatches every call to the
interpreter: `call_inner` skips the WASM, MLIR, and JIT entries for
that fiber. This is the whole story — native code checks neither fuel
nor breakpoints, so debugging never enters it. Compiled-in
`debug/break` needs no flag: signal inference already forces its
callers off the JIT. Fibers without the flag are unaffected; their
JIT'd frames remain opaque to inspection, as today.

## Implementation status

Documentation, then tests, then code — each phase lands its tests
before its implementation.

| Phase | Contents | Status |
|-------|----------|--------|
| 1 | `Code.name`, `Code.local_names`, `fiber/frames`, `fiber/disasm`, region pin | not started |
| 2 | `debug/break`, attached flag, hygiene exemptions, signal registration | not started |
| 3 | fiber debug flag, breakpoint table, `debug/break-at`, per-instruction fuel, tier bypass | not started |
| 4 | `lib/debug.lisp` driver, snapshot/outcome schemas | not started |
| 5 | recording seams, log schema, `debug:replay`, `debug:bisect` | not started |
| 6 | MCP session tools (out of tree) | not started |

### Test obligations per phase

1. Frames of a yielded fiber carry correct names, locations, locals;
   a local's name matches its `let` binding; inspect → drop fiber →
   use value does not crash (region pin); `:alive` inspection errors.
2. Parent with `|:debug|` catches a break and reads the payload;
   resume value becomes `debug/break`'s result; no debugger → `nil`,
   no suspension; break inside `squelch` preserves the continuation;
   a function containing `debug/break` reports `(jit? f)` false.
3. `fiber/set-fuel 1` under the debug flag advances exactly one
   instruction; a `debug/break-at` line pause resolves to that line's
   first instruction; resume past a breakpoint does not re-trigger it
   (skip-once); the same closure called from a non-debug fiber takes
   the JIT path.
4. End-to-end corpus tests: `debug:until` over a loop, `debug:step-line`
   across calls, `debug:eval` reads a local.
5. A recorded run with I/O replays to an identical event log; a
   retroactive breakpoint during replay observes the same values;
   `debug:bisect` finds a planted invariant violation.

---

## See also

- [signals/fibers.md](signals/fibers.md) — fiber architecture, suspension frames
- [signals/primitives.md](signals/primitives.md) — resume semantics, swap protocol
- [runtime.md](runtime.md) — fuel budgets, signal bits
- [impl/vm.md](impl/vm.md) — dispatch loop, executing-closure register
- [analysis/debugging.md](analysis/debugging.md) — existing introspection toolkit
- [mcp.md](mcp.md) — the Elle MCP server
