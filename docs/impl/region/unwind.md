# An abandoned frame runs the releases it still owes

<!-- audited: 2026-09-05 -->

The two tables naming what an abandoned frame still owed, and the exits that walk
them. An error, a squelch boundary, a discard and a compiled unwind share one walk.

The section above places one release in the block a signal exit skips. That block is
not the only thing skipped. An **error** leaves through the signal machinery, so
*none* of the frame's remaining instructions run — and every release the frame still
owed is among them. The frame that called the raising native holds the arguments it
materialized for that call, and every binding whose last use lies past it; each of
those is one region nobody releases. The rate is per unwound frame and per pending
value, so a `try`/`protect` in a loop grows without bound — the shape a retry loop
and a server request loop both are.

The frame is gone, so the release cannot be reached by resuming it — the runtime
runs it at the exit instead. What that needs is the set the frame still owes,
and the emitter already names it. A **value-routed** release is three
instructions, `LoadLocal s; DecrefValueRegion; StoreLocal s nil` ([two
resolutions](mechanism.md)), and the slot `s` is its whole identity: the
instruction releases whatever `s` holds at the moment it runs, and the nil stamp
is what records that it ran. `Code::frame_release_slots` carries those slots per
function; an error exit walks them, and a slot still holding a heap value is a
release that did not run.

**The slot is the release, not the value.** Three facts make the walk *that* release
rather than a new one:

- **A slot belongs to one binding for the whole body.** `allocate_slot` counts
  `num_locals` up and never reuses, and stamps each new slot nil where the binding is
  introduced, so nothing but that binding's own value is ever what the walk finds.
- **The nil stamp is the receipt.** Every value route clears its slot as it releases,
  so a non-nil slot is an unrun release and a run one is invisible to the walk. An arm
  whose release the taken path would not have reached reads nil for the same reason a
  replicated release does ([the relocation point](replicate.md)).
- **A release the emitter declined is not in the table.** A slot is recorded where the
  plain value route is *emitted*, so a mutated route, a reassigned binding's slot, a
  cell release naming the box, and a transfer adopt each record nothing. The walk can
  only run a release the frame genuinely had.

**One other site records, on the same three facts.** A dynamic `emit` off tail
position takes a retain at its payload argument and releases it in the
continuation past the call ([owner.md](owner.md)). That release is a value route
in every respect the walk reads: a slot the site allocates for itself, writes
once before the call and reads once after it, and clears as it releases. What
records it is the **terminal** raise. There the catcher consumes the delivery
and this retain answers to the continuation alone — which a fiber nobody
restarts never runs, so the table is the only route to it. A suspending raise
records the slot too and is unaffected: its parked payload is what the walk
protects, so the walk passes the slot over and the discharge answers for the
retain instead.

**Two routes, two receipts.** The slot-resolved release (`DecrefRegion`) is named the
same way, by the static region slot it carries, and its receipt is the activation map
itself: the alloc mints the mapping and the release TAKES it
(`take_runtime_region_for_drop_slot`), so a slot still mapped is a release that did not
run. `Code::frame_release_regions` carries those. Naming only the slots the *executing*
function releases for is what keeps a caller's leftovers out: the map survives a
frame-replacing tail call, and the references still in it are the callee's own machinery
to answer for.

The tables are not everything the exit runs. A release the frame took over from
a frame-replacing tail call has no route and no receipt to record — the
instruction that would have run it is the *caller's*, dead past the `TailCall` —
so it is carried on the activation itself and discharged at the same exits, on
the same `walk_abandoned` question ([owner.md](owner.md)).

**What the signal carries is not abandoned — unless the raise minted its delivery.**
The error's payload leaves with the signal, and the catcher's read of it is funded by
exactly one **delivery** reference. Where that reference comes from decides what the
walk may release, and the two raise paths differ:

- A **native** raise installs the payload with no retain. A fresh error struct funds
  the delivery with its own birth reference; a payload the native read out of an
  argument funds it with the frame's reference to that argument — the very release the
  walk would otherwise run. So the walk skips a slot whose value lives in the
  payload's region: the skipped release *is* the delivery.
- An **`Emit`** raise (`(error v)`) mints the delivery itself — the `EmitEscape`
  retain `handle_emit` takes, consumed by the resumer's release of the resume result.
  The frame's own reference funds nothing, so the skip has nothing to stand in for and
  would strand one region per raised-and-caught error whose payload the raise chain
  owns. The raise records the mint in the ledger (`Fiber::delivery`, `record_mint`),
  and a walk whose live signal payload matches it (`mint_names`) skips nothing.
- A **dynamic `emit`** reads like the `Emit` case with the mint moved to the exit: the
  raise is an ordinary native call, so the signal exit mints the delivery of a payload
  the call received as an argument and records it there ([owner.md](owner.md)). What the walk then reclaims is the
  frame's own reference to the payload — in TAIL position wherever the body allocated it,
  and off tail position the site's own payload retain, which the table names for exactly
  this reason.
- An **injected** `fiber/abort` / `fiber/refuse` payload reads like the `Emit` case,
  for the same reason: the injection mints the delivery
  ([effects.md](effects.md)) and records it on every fiber whose frames
  the payload then travels through — the aborted one and, where the error escapes it,
  the aborting one. A frame holding the payload owes its release like any other.

The same reading governs the parked frame's discharge: `Fiber::take_parked_state`
withholds its payload protection exactly where the mint is recorded, so the free-path
discharge runs the parked body frame's owed release for an emitted payload and leaves a
native payload's standing. Nothing else survives the frame either way: a value the
frame stored elsewhere is held by a counted edge the store funnel recorded, which this
release cannot take below that holder's count.

**A frame the restarts system can replay is not abandoned.** A fiber body's first run
parks its own frame on an error exit (`do_fiber_first_resume`), so a restart replays
those instructions and the releases among them; running them here as well would release
twice. The parking caller says so with a one-shot (`VM::pending_error_park`), taken at
the activation's entry so the frames that body *calls* — which nothing parks — still
walk. What the parked frame owes runs where no resume can reach it either: the fiber's
own discharge reads the same two tables off each parked `BytecodeFrame`, its saved
locals and its saved activation map standing in for the live ones
([owner.md](owner.md)).

**A squelch boundary abandons frames the same way, so it runs the same walk.** A
`squelch`/`attune` boundary raises a `signal-violation` the activation it breaks out of
never catches (`VM::enforce_squelch`), so that activation reaches none of its remaining
instructions either. The exit **is** the error exit, and the trampoline writes it as
one: the squelch arm turns the bits into `SIG_ERROR` and falls through to the walk
below it. One arm is what keeps the two exits from drifting, and it hands the frame's
operand stack out exactly as an error's does — a fiber body's first run parks that
frame, and what a parked frame owes runs at its discharge.

The boundary abandons a whole chain rather than one frame. Every frame parked between
the emitting site and the boundary is discarded at one chokepoint,
`VM::discard_suspended_frames`, which each enforcement site reaches through
`squelch_violation`. So the discard reads each discarded `BytecodeFrame`'s two tables
off its own `Code` — the reading `Fiber::take_parked_state` already makes for a fiber
that can never run again, with the frame's saved locals and saved activation map
standing in for the live ones ([owner.md](owner.md)).

The fiber **survives** a squelch, and that is what the reading has to answer for: an
outer, non-discarded frame and the activation that catches the violation both run on.
The tables answer it, and the fiber's survival never enters the argument. A discarded
frame's saved stack and its map were taken and cloned at that frame's own park, and the
activation they came from returned before the boundary was reached, so no live frame
reads either. Within them, each route names only the executing function's own slots and
carries its own receipt, so a slot still holding a heap value, or still mapped, is a
release that frame genuinely owed and did not run. The rest of the parked map stays
untouched for the opposite reason: a blanket release of it has no receipt at all, and
the regions it names may be an outer frame's.

**What the boundary raises exempts nothing, and neither does what it displaces.** The
`signal-violation` is built by `escaping_error` in a fresh region of its own and records
no delivery mint, so no frame's reference funds its delivery and every release the
tables name runs. The signal the boundary displaces is not the exemption's subject
either: a squelched park's payload is delivered to nobody, and the park's own escape
retain holds it independently of any frame, so a frame's reference to it is that
frame's to release like any other. That retain is then the boundary's own to release,
along with whatever a displacing install would have owed — the park having ended with
neither a reader nor an install ([owner.md](owner.md)).

**The compiled tier runs the same walk.** A compiled frame leaves by an error at two
points, and each runs the walk before its activation's region-map pop: the check after
a call, which finds the callee's raise, and an `Emit` of `SIG_ERROR`, which parks no
frame to resume. The tables are compile-time constants, so the prologue materializes
them once — each in its own stack slot, at its own width — and every exit hands the
runtime both, with the frame's locals spilled in slot order. The value route resolves
`s` to the spilled `LoadLocal s`; the slot route reads the very activation map the
compiled prologue pushed. The two tiers share the walk itself; only where the slots
are read differs.

Nothing is spilled back. The compiled frame returns as the walk completes, so the nil
stamp that is the interpreter's receipt has no compiled counterpart and needs none: a
table names each slot once, and one error exit runs per unwind.

A compiled frame is never one a restart replays, so `VM::pending_error_park` has no
compiled reader. A fiber body's first run enters through
`execute_bytecode_saving_stack`, and compiled code is reached only from a call site
inside it, so the parked frame is always an interpreter frame.

Every exit — compiled or not — pops the map it pushed, and the walk depends on it:
`last()` must be the abandoned activation's own frame, not a callee's leftover.
`execute_bytecode_saving_stack` asserts the balance in debug builds, so an exit path
that returns without popping detonates at the first activation to return through it
rather than resolving some later release against the wrong frame.

The rule reaches the exits an error never takes. A compiled frame whose CALLEE
suspends parks itself at the post-call yield check and returns the yield sentinel,
and that exit pops too: the park reads the map first, so what the pop discards is a
frame nothing needs again. Left behind, it is what `last()` names for the interpreter
activation above — which then parks a map that was never its own — and the remap stack
never shrinks back, one frame per suspend through a compiled callee. That the exit is a
suspend rather than an unwind changes only what runs before the pop: nothing, because
the frame resumes and still owes its releases to the resumed body.
`jit::compiler::tests::every_compiled_exit_pops_the_region_map` pins it on the emitted
code, where a missing pop is visible without an activation having to return first.

Pinned by `tests/elle/region-error-unwind.lisp` (the leak gauge — the pending
release of a raising call's argument, of two of them, of a binding live across
the raising call, and of an enclosing frame, each bounded beside a control that
raises holding nothing), the `error-payload*` closed controls in
`tests/elle/oracle.lisp` (the emitted payload's own region, bounded per face —
raised in the parked body frame, in a walked non-tail callee, handed down as an
owned parameter, and as a two-region struct — beside the native-raise control
whose gap isolates the recorded mint from the walk and discharge) with
`tests/elle/region-error-payload-uaf.lisp` as their guardfree complement (the
payload a catcher stores outward, a borrowed module payload raised repeatedly, a
native raise's unrecorded install, and a restarted `:error` fiber's replay), the
`denied-discard` probe in `tests/elle/oracle.lisp` (the per-op rate of what the
tables cannot name), `tests/elle/region-jit-error-unwind.lisp` with
`tests/elle/region-jit-error-unwind-uaf.lisp` as its guardfree complement (the
compiled face — one subject per compiled error exit, and, on the soundness side,
the caller's binding live across a compiled callee's exit),
`vm::core::region::tests::a_compiled_frames_*` (the spilled locals stand in for
the frame stack, and the payload exemption reads the same) and
`jit::dispatch::tests::release_abandoned_frame_runs_both_routes_off_the_compiled_exits_buffers`
(the two tables reach the runtime as separate buffers of different widths),
`lir::lower::tests::release::emission::{frame_release_tables_name_exactly_the_routes_emitted,
a_reassigned_binding_records_no_value_route,
a_non_tail_dynamic_emit_payload_release_carries_its_receipt}` (the tables are
the emit sites, so a route the emitter declined has no entry and the one other
site that records carries both halves of a value route's receipt), with
`tests/elle/region-dynamic-emit-statement-uaf.lisp` as that site's guardfree
complement, and `tests/elle/region-error-unwind-uaf.lisp` (the soundness
complement — the payload the raising native builds while the frame holds its
argument, a value the frame stored into a container that outlives it, a parked
frame the restarts system replays, and a catching frame's own values, all under
`--trace=guardfree`). The squelch face carries the same pair —
`tests/elle/region-squelch-unwind.lisp` (the leak gauge: a pending value in the
emitting frame, two of them, an enclosing frame's, and the same under an
`attune` boundary, each bounded beside a violation that has nothing pending) and
`tests/elle/region-squelch-unwind-uaf.lisp` (the soundness complement: what the
catching activation, an outer non-discarded frame, and a longer-lived container
still read after the discard ran) — with
`runtime::tests::ownership::discard_runs_the_abandoned_frames_release_tables`
driving the chokepoint directly, where both routes and the untabled slot beside
each are visible without a program that allocates into them.

