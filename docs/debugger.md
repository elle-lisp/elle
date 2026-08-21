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
(`src/vm/fiber/catch.rs`). The catch test is bit *overlap*, not
subset, with two carve-outs (`covers`,
`src/value/fiber/signalbits.rs`): a compound signal that carries
`:io` is caught only by a mask that names `:io`, and a signal
carrying the VM-internal `SIG_TERMINAL` bit is never absorbed by any
mask. An I/O request is emitted as the
compound `:io|:yield` — the primitive's *static* signature adds
`:error`, but an error is an alternative return, not a co-emitted
bit, and subprocess completions add `:exec`. So the debuggee's
`:error` bit does not trap a request. The request passes through the
suspending debugger to the scheduler, whose mask on the debugger
names `:io`. The existing `FiberResume` chain
(`src/vm/fiber/resume.rs`) re-delivers the completion to the
debuggee. Signal routing needs no new rules;
`tests/elle/carveout.lisp` pins the pass-through end to end.

A paused debuggee is a resumable snapshot: `fiber.suspended` holds the
parked frame chain (`src/value/fiber/frame.rs`). The debugger reads it
through the inspection primitives below and never mutates it.

## The `:debug` signal

`:debug` is signal bit 2. It is already registered, classified
`SignalAction::Suspend`, and routed by fiber masks
(`src/signals/mod.rs`, `src/signals/registry.rs`). The LIR lowerer
already treats it as suspending — a call that may emit `:debug`
compiles to `SuspendingCall`, which gets a continuation frame
(`src/lir/lower/control/call.rs`; the trigger set is
`:yield|:debug|:io|:wait`) — and at every suspend site the runtime
moves the activation's owner node into the parked frame, so parked
state stays owned across the pause. Region inference is *more*
conservative, not less: a fiber-body lambda whose signal carries a
suspending bit is disqualified from the transferred-return ownership
cut, so a thunk that calls `debug/break` gives up that optimization
and nothing else. What is missing is producers. The debugger adds
two, plus the rules below.

**Producers.** The `debug/break` primitive, registered beside the
existing `debug/*` primitives in `src/primitives/debug.rs` with
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
by excluding the VM-internal bits (`src/signals/mod.rs`), and bit 2
is not excluded. There is no `fiber/deny` primitive: denial is the
`:deny` argument to `fiber/new`, inherited by descendants, and every
primitive call checks `signal.bits ∩ withheld ∩ CAP_MASK` at four
sites (interpreter call and tail call, JIT call and array call;
`tests/elle/caps.lisp` pins the payload). So
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
  `signals::squelched_bits` (`src/signals/mod.rs`), at all ten sites:
  the interpreter's `enforce_squelch` (`src/vm/core.rs`) serves six,
  and four more are inlined in the JIT paths — two in
  `src/vm/run_on/jit.rs` and two in `src/jit/calls/callops.rs`. One
  predicate is what keeps the exemptions from drifting apart between
  tiers. It exempts `:error` and `:halt` by intersection, `:switch` by
  exact equality, and subtracts the pause bits (`SIG_PAUSE`). A
  boundary that instead converts a tooling pause into a
  signal-violation discards the suspended frames and leaves the fiber
  paused-but-resumable over the wreckage — resume re-executes the
  interrupted instruction against a torn-down stack.
  `tests/elle/squelch-fuel.lisp` pins the rule for `:fuel` on both
  tiers, one case per charge-site shape. `:debug` joins `SIG_PAUSE`
  with the debug pause itself; the constant is the single place to add
  it. `(squelch f :fuel)` is inert — metering is the parent's action,
  not the closure's behavior, so the boundary has nothing to enforce.
- The silence enforcement (`src/vm/call/inner.rs`) kills the process
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
  (`src/vm/dispatch/interp/signals.rs`; the JIT mirrors it; the WASM
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
`BytecodeFrame` (`src/jit/suspend.rs`). A compiled-in breakpoint
therefore pauses with inspectable frames even in JIT'd code, and
resumes interpreted. Batch (SCC) compilation rejects suspending
members outright (`src/jit/compiler.rs`), so a function containing
`debug/break` is never in a batch group, and the direct SCC peer
calls that bypass the VM never contain a breakpoint. Stepping and
dynamic breakpoints exist only in the interpreter and need the debug
flag (see [Tier interactions](#tier-interactions)).

## Debug information

The runtime carries part of what inspection needs:

| Table | Where | Maps |
|-------|-------|------|
| `location_map` | `Code`, `CallFrame` | bytecode offset → file, line, column |
| `syntax` | `ClosureTemplate` | the lambda's full surface AST |
| `lir_function` | `ClosureTemplate` | SSA, CFG, yield points, call sites |

Two facts qualify the table. First, `location_map` is a sparse,
unordered `HashMap`: one entry per LIR instruction and one per block
terminator, not one per bytecode offset (`src/lir/emit/mod.rs`), and
all-zero synthetic spans are dropped (`record_location`,
`src/compiler/bytecode.rs`), so macro-generated code is unmappable.
Every consumer today does a point lookup; inspection adds the sorted
index that resolves an ip to the nearest preceding entry. Second, `Code` holds no back-pointer to its template, so
`syntax` and `lir_function` are reachable from a closure *value* but
not from a parked frame. The frame's parked closure register is a
possibly-dead borrow that inspection must never dereference (see the
register invariants in [impl/vm.md](impl/vm.md)). Everything frames
expose must ride `Code`; `BytecodeFrame.code` is always live.

Three additions, all on `Code` (`src/value/code.rs`), flowing the same
path as `location_map`:

- **`name: Option<Rc<str>>`** — copied from `ClosureTemplate.name`.
  That field exists but is `None` for all compiled code: nothing
  assigns `LirFunction.name`, which is why stack traces print
  `<anonymous>` today. The real work is plumbing the enclosing
  `define`/`letrec` binding name from HIR through `lower_lambda` into
  `LirFunction.name`; the emitter already copies it from there
  (`src/lir/emit/instr.rs`). Landing this names stack traces too.
- **`local_names`** — `(name, place, index)` entries for everything a
  frame binds. The lowerer's `binding_to_slot` map (declared in
  `src/lir/lower/mod.rs`, filled in `emitops.rs`) has the data and
  dies before emit; it moves onto `LirFunction`. The place is
  required because bindings live in two address spaces whose indices
  overlap, and in three shapes. A plain local lives in its stack
  slot. An in-lambda mutated-or-captured local is env-celled: its
  reserved stack slot stays `nil` and the value lives in an env cell
  (`allocate_slot_routed` mints the env index). A `letrec` binding
  with a compiled cell — and any captured binding outside a lambda —
  keeps a cell *in* its stack slot, one dereference behind the slot.
  The entry records which shape holds the value, so inspection reads
  the right address space and unwraps only real cells. Parameters
  ride the same table with an env place — they live after the
  captures in the env, not in stack slots, and without entries for
  them a frame's `:locals` would omit every argument. The
  per-function scratch slot (`discard_slot`) holds garbage between
  uses and is excluded; compiler temporaries get a `nil` name. Within
  each address space slots are allocated monotonically and never
  recycled, so one name per index is exact — no ip-ranged table is
  needed. Bindings the lowerer constant-folds away have no slot and
  do not appear.
- **the `Bytecode` → `Code` repair** — `reserved_locals` is copied
  from `ClosureTemplate.num_locals`, but the top-level, `eval`, and
  module-import paths build `Code` straight from `Bytecode`, which
  does not carry the count. Those code objects claim zero locals
  while their prologue reserves slots. `Bytecode` gains `num_locals`
  so every `Code` is correct; without it, top-level locals render as
  operand-stack junk. The same paths are uneven about locations:
  `VM::execute_bytecode` (`src/vm/mod.rs`) attaches an *empty*
  `location_map` although `Bytecode` carries a real one, so top-level
  frames resolve to no source at all today. The repair copies both
  fields. Copying `num_locals` also arms the debug-build
  locals-integrity assertion for these frames — it is vacuous while
  `reserved_locals` is 0 — which may surface latent violations; the
  phase's tests cover top-level pauses for exactly this reason.

## Inspection primitives

Inspection operates on a fiber whose status is `:new`, `:paused`,
`:dead`, or `:error`. The keyword is `:paused` — `tests/elle/fuel.lisp`
pins it. Inspecting an `:alive` fiber is a *checked* error, not a
convention: a running fiber's handle slot is empty and an unchecked
borrow panics (`src/value/fiber/handle.rs`). The checked forms
(`try_with`/`try_with_mut`) exist there; inspection uses them. `:new`
and `:dead` fibers have no suspended chain; `fiber/frames` returns
`[]` for them.

| Primitive | Signature | Returns |
|-----------|-----------|---------|
| `fiber/frames` | `(fiber) → array` | frame structs, innermost first |
| `fiber/trace` | `(fiber) → array` | call-site records from `Fiber.call_stack` |
| `fiber/disasm` | `(fiber index) → array` | disassembly lines for frame `index` |

A frame struct:

```text
{:kind     :bytecode
 :name     "worker"          # Code.name, or "<toplevel>"
 :file     "src/thing.lisp"  # nearest location_map entry at or before :ip
 :line     42
 :col      7
 :ip       118               # bytecode offset
 :locals   [("acc" 10) ("xs" [1 2 3])]   # via local_names places; parameters included
 :stack    [...]}            # operand stack above the locals
```

The chain is the fiber's `suspended` vec, and not every entry is
bytecode. A `FiberResume` link — a child fiber suspended through
`defer` or `protect`, or any child whose uncaught non-terminal signal
passed through — renders as `{:kind :fiber :fiber f}`; callers
recurse with `fiber/frames` on `f`. Tail calls trampoline in place,
so a run of tail calls appears as one frame holding the last
callee's code. Frame count is not logical call depth.

Locals occupy stack slots `[0, reserved_locals)` of the parked frame's
stack — closure bodies execute on a fresh stack with base 0. A celled
local's value is read through its env cell, per its `local_names`
place; its stack slot is not the value.

**Errors keep one frame.** On `:error` the VM parks exactly one
resumable activation and drops the rest (`src/vm/call/inner.rs`
returns without parking the caller chain). Which frame survives is
path-dependent: a first resume parks the fiber body's root activation
(`src/vm/fiber/resume.rs`); a later resume parks the innermost
re-entered frame and drops the *outer* parked frames
(`src/vm/core/resume.rs`). `fiber/frames` on an errored fiber returns
that single resumable frame. `fiber/trace` compensates:
`Fiber.call_stack` survives exactly the error path — the suspend path
pops it — and holds one record per *interpreter* closure call site
(natives never push one; the native-tier dispatches pop theirs). Each
record carries a name and an ip resolved through the record's own
`location_map` — names and locations, no locals. Phase 3 upgrades this under the debug flag: the
error path parks frames exactly as suspension does, so error
snapshots carry every activation and resume delivers the recovery
value to the error point. Production error handling (restarts) is
unaffected — the flag is off.

**Region rule.** Inspection primitives declare `RegionEffect::Fresh`:
the result allocates in a region minted fresh for the call, and the
allocation scan increfs every cross-region reference the result
embeds (`src/value/fiberheap/regionstore/alloc.rs`), balanced by the
caller's normal release. That pins the parked values' regions for the
result's lifetime without touching the fiber. The terminal-result pin
from the swap protocol is the wrong model here: it is one-shot — its
releases are the free-time signal scan and, for a restarted `:error`
fiber, the displacement release (`release_displaced_terminal_signal`)
— and copying it would leak one incref per call in a stepping loop. Snapshot values are shared, not
copied — if the debuggee resumes and mutates an array, the snapshot
sees the mutation. Inspection allocates nothing into the debuggee's
regions; this also keeps observation invisible to replay (see
[Execution history](#execution-history)). Tests must cover
inspect-then-drop-fiber-then-use ordering.

`fiber/disasm` wires the disassembler
(`src/compiler/bytecode/disasm.rs`) to a frame's `Code`. The current
entry point takes raw bytes and bakes offsets into strings; it gains
a structured `(offset, text)` form so the `*` marker on the frame's
ip needs no string parsing. The disassembler hand-duplicates operand
widths with a silent fallthrough, so Phase 1 adds an exhaustiveness
test pairing every operand-bearing opcode with its width — a new
opcode must not silently desync every following offset.

## Breakpoints

### Compiled-in: `debug/break`

```text
(debug/break payload)   # suspend with SIG_DEBUG; parent sees payload
```

The parent reads `(fiber/value f)` to get
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
`location_map` entry names it — a scan, since the map is unordered.
Resolution covers only the given closure's own code; the driver walks
`child_protos` when the caller wants a whole definition. A line that
exists only in macro-generated code has no entries; the returned ip
array is empty and the driver reports it.

**The check.** When a fiber's debug flag is set, the dispatch loop
consults the table before executing each instruction, beside the
loop's existing unconditional per-instruction work — the fiber-signal
check and the allocation-error take (`src/vm/dispatch/interp.rs`).
The locals-integrity assertion nearby exists only in debug builds and
is not the anchor. On a hit, the VM emits `:debug|:fuel` with a `nil`
payload and exits the loop at the *unexecuted* opcode's ip.

**Pause protocol.** The composed bits are the protocol. The three
park sites that can park a re-execute pause compute
`push_resume_value = !bits.intersects(SIG_FUEL)`
(`src/vm/fiber/resume.rs`, `src/vm/core/resume.rs`,
`src/vm/call/inner.rs`); the other suspend sites park emit/yield
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
(`src/vm/call/inner.rs`) parks the interrupted callee only when it is
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
`src/vm/dispatch/interp/opcodes.rs`): at zero, the fiber suspends
with `:fuel`, the ip points at the unexecuted opcode, and resume
re-executes it exactly. The pause payload is `nil`; the driver
synthesizes the outcome.

Today fuel is charged at seven interpreter sites: backward `Jump`,
the four call opcodes, and the two array-call opcodes
(`src/vm/dispatch/interp/opcodes.rs`). Conditional jumps never
charge, and no native tier charges at all. When a fiber's debug flag
is set, the loop charges fuel on **every** instruction, at the same
loop-top site as the breakpoint check. Instruction-granular stepping
is then:

```text
(fiber/set-fuel f n)      # existing primitive
(fiber/resume f nil)      # runs exactly n instructions, pauses with :fuel
```

Fuel is not refilled on resume — a zero-fuel resume re-pauses at the
same ip — so the driver sets fuel before every step, as
`tests/elle/fuel.lisp`'s driver-loop scenario pins.

**Fuel ownership.** A fiber has one fuel register, and the parent
that meters the fiber owns it. `debug:launch` creates the debuggee,
so the driver owns its register by construction. A fiber some other
scheduler already meters — a process under the quantum in
`lib/process.lisp` — cannot also be stepped; debugging inside a
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

## Execution history

Recording and replay are specified now so the instrumentation points
are designed in; they ship as a later phase.

### What gets recorded

Fibers are single-threaded and cooperative, so there are no thread
interleavings. External nondeterminism enters through primitives
(with two machine-level exceptions, below), so the recorder lives at
the primitive layer: a VM record/replay mode flag consulted at
primitive dispatch. Both schedulers — the async scheduler in
`src/stdlib.lisp` and the process scheduler in `lib/process.lisp` —
are deterministic Elle code driven entirely by these primitives'
results, so both replay without instrumentation.

The seam is not a hand-audited list. `PrimitiveDef` gains a `replay`
class, declared in the `primitive!` tables like `signal` and
`effect`, and a registry test fails on any unclassified primitive —
adding a primitive without deciding its replay class breaks the
build. Three classes:

| Class | Meaning | Examples |
|-------|---------|----------|
| `pure` | deterministic given VM state; never recorded | arithmetic, collections, `string/*` |
| `record` | result logged in call order; replay feeds it back | `io/*`; `file/*`, `port/*`, `read`, `read-all`; `subprocess/*`; `clock/*`, `time/sleep`; `sys/env`, `sys/args`, `sys/argv`, `sys/pid`, `sys/resolve`, `sys/thread-id`, `sys/ip?`; `os/sig-*`; `ev/poll-fd`, `chan/wait-ready`; `ffi/*`, `ptr/to-int`; `debug/memory`, `debug/arena-*`, `vm/tier`, `jit/rejections` |
| `refuse` | no sound seam exists; recording raises an error | `sys/spawn`, `sys/spawn-vm`, `ffi/callback` |

FFI stubbing is sound because every foreign observation flows back
through an `ffi/*` seam: a recorded pointer is just a number until
`ffi/read` dereferences it, and that read replays its recorded value.
`ffi/callback` is refused under recording — a foreign-initiated entry
into Elle has no seam. Note `:exec` is a capability bit, not a
dispatch route; subprocess completion rides the `:io` seam.

Thread refusal is load-bearing, not conservatism: `sys/thread-state`,
`chan/send`, `chan/recv`, and `chan/try-select` are synchronous
timing reads with no signal, so another thread's timing leaks into a
"single-VM" run invisibly. v2 may admit threads by moving the `chan/*`
family and `sys/thread-state` to the `record` class.

The two machine-level exceptions do not flow through primitives.
First, tier choice: the JIT worker is an OS thread, and whether call
*N* runs interpreted or native depends on its scheduling. Results are
tier-invariant (the corpus enforces that), but allocation order is
not guaranteed to be, and the tier observations (`vm/tier`,
`jit/rejections`, `debug/arena-*`) are not. Recording therefore
disables the async worker, promotes tiers synchronously by call
count — deterministic, because call counts are deterministic — and
stores the tier policy in the log header; replay applies the same
policy. Second, heap addresses: see
[Identity order](#identity-order) below.

### The log and the clock

The log is a flat array of event structs:

```text
{:t [segment fuel] :ev :io     :value <recorded result>}
{:t [segment fuel] :ev :break  :file "x.lisp" :line 9 :ip 44}
```

The clock is a pair. `segment` counts recorded events — the coarse,
cheap coordinate recorded live. `fuel` addresses a point inside a
segment: replay to the segment start, set the debug flag and
`fiber/set-fuel k`, resume. Recording never meters instructions; only
a replay that needs an intra-segment stop pays for step mode.

### Replay

`debug:replay log &named until` re-runs the program feeding recorded
values back at each seam, in order. Execution between seams is
deterministic, so the replay is bit-for-bit the original run. Every
debugger feature works during replay — which yields **retroactive
breakpoints**: choose the predicate after the crash, replay with it
armed, and observation does not perturb the bug.

That last claim needs two guarantees, stated here because both are
easy to break silently.

First, observation must not allocate into the debuggee's regions.
The design already guarantees this: inspection results are
`Fresh`-allocated in the debugger's regions, and dynamic pauses carry
`nil` payloads.

### Identity order

Second — a prerequisite, not a discipline. Reference-identity values
(closures, parameters, syntax, managed pointers) order and hash by
heap address today (`src/value/repr/traits.rs`): `(hash f)` returns
an address-derived integer, and a set of closures iterates in address
order. Replay runs in a fresh process, and addresses are not
reproducible across processes, so any program that observes such an
order can diverge under replay before the first seam. Phase 5
therefore starts by making identity order deterministic:
reference-identity values order and hash by a per-VM allocation
sequence number, not by address. Fibers already do the equivalent —
they hash by handle identity, which survives relocation — and the
same move makes closure order a pure function of the execution
history. A test pins the result: a recorded run and its replay, in
separate processes, produce the same event log, the same `hash`
values for the same closures, and the same closure-set iteration
order, with breakpoints armed in only one of them.

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

A fiber whose debug flag is set never enters native code. `call_inner`
skips the WASM, MLIR, and JIT entries for a flagged fiber, and the
forced-tier entries (`compile/run-on`, `src/vm/run_on/`) fall back to
the interpreter for it. The other native dispatch paths — the
JIT-to-JIT fast path in `elle_jit_call` and direct calls between
batch-compiled SCC peers — run only *inside* native frames, and a
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
| 1 | name plumbing (HIR → `LirFunction.name` → `Code.name`), `Code.local_names` with three-shape places and parameter entries, the `Bytecode` → `Code` repair (`num_locals` + `location_map`), `fiber/frames`, `fiber/trace`, `fiber/disasm`, `Fresh` region rule, disasm exhaustiveness | not started |
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
   locals and resolves file and line (the `Bytecode` → `Code`
   repair); a tail-call run renders one frame; a `FiberResume` entry
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
   `tests/elle/squelch-fuel.lisp`); a `debug/break-at`
   line pause resolves to that line's first instruction; resume past
   a breakpoint does not re-trigger it (skip-once); a dynamic break
   inside an inferred-silent function pauses instead of aborting; the
   same closure called from a non-flagged fiber takes the JIT path;
   an error under the debug flag exposes every activation's frame.
4. End-to-end corpus tests: `debug:until` over a loop,
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
