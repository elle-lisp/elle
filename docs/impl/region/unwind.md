# An abandoned frame runs the releases it still owes

<!-- audited: 2026-10-06 -->

The two tables naming what an abandoned frame still owed, and the seams that walk
them. A call boundary, a refusing host and a discharge share one walk.

[What a signal exit owes](signalexit.md) places one release in the block a signal
exit skips. That block is not the only thing skipped. A parked frame runs none of
its remaining instructions until a resume replays them, and every release the
frame still owed is among them. The frame that called a raising native holds the
arguments it materialized for that call, and every binding whose last use lies
past it. A park that nothing resumes leaves each of those as one region nobody
releases: a fiber released while it waits, a call boundary's refusal, a host that
cannot hold the park. The rate is per abandoned frame and per pending value, so a
`try`/`protect` in a loop grows without bound — the shape a retry loop and a
server request loop both are.

The frame will not run again, so the release cannot be reached by resuming it,
and the seam that ends the park runs it instead. What that needs is the set the
frame still owes, and the emitter already names it. A **value-routed** release is
three instructions, `LoadLocal s; DecrefValueRegion; StoreLocal s nil` ([two
resolutions](mechanism.md)), and the slot `s` is its whole identity: the
instruction releases whatever `s` holds at the moment it runs, and the nil stamp
is what records that it ran. `Code::frame_release_slots` carries those slots per
function; the walk reads them, and a slot still holding a heap value is a release
that did not run.

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

**Two other sites record, on the same three facts.** Each retains a payload its
body releases nowhere and releases it in the continuation past the suspend
([park.md](park.md)): a suspending `Emit` of a borrowed payload, and a dynamic
`emit` off tail position, at its payload argument. Each release is a value route
in every respect the walk reads: a slot the site allocates for itself, writes
once before the suspend and reads once after it, and clears as it releases.

The record serves the parks whose continuation never runs and whose payload the
walk does not protect. A **terminal** dynamic `emit` nobody restarts is one: the
catcher consumes the delivery the raise minted, so the retain answers to the
continuation alone. A `fiber/abort` or `fiber/refuse` that raises in place over a
suspending park, left unrestarted, is another: the injected error displaces the
payload the discharge would have released. A call boundary is the third,
because what it discards exempts nothing (below). While a suspending park
stands, its payload is what the walk protects, so the walk passes the slot over
and the discharge answers for the retain instead.

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
so it is carried on the activation itself, parked with it, and discharged at the
same seams ([owner.md](owner.md)).

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
  the call received as an argument and records it there ([park.md](park.md)). What the walk then reclaims is the
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
discharge runs each parked frame's owed release for an emitted payload and leaves a
native payload's standing. Nothing else survives the frame either way: a value the
frame stored elsewhere is held by a counted edge the store funnel recorded, which this
release cannot take below that holder's count.

**An error parks its chain, so nothing walks at the raise.** An error exit parks
every frame from the raise to the fiber body, as a suspend does: the raising frame,
and each caller behind it (`complete_call`, `do_fiber_first_resume`,
`resume_suspended`). A restart replays those instructions and the releases among
them, so a walk at the raise would release twice. What a parked frame owes runs
where no resume can reach it: the fiber's own discharge reads the same two tables
off each parked `BytecodeFrame`, its saved locals and its saved activation map
standing in for the live ones ([owner.md](owner.md)).

**A host discards an error's chain as it discards a refused park.** `eval`,
`import`, the compile hosts, `compile/run-on`, the root driver, macro expansion and
the other re-entrant callers run code on the current fiber and cannot hold a park
of it. An error the hosted code raises parks its chain all the same, and the host
ends that park through `VM::discard_suspended_frames` before it raises at its own
call. The discard protects the error's payload, which leaves with the host's
error, and runs every other release the tables name.

**A call boundary abandons frames the same way, so it runs the same walk.** A
`squelch`/`attune` boundary raises a `signal-violation`, and a silent function's proof
raises a `broken-proof`, that the activation it breaks out of never catches
(`VM::enforce_boundary`). So that activation reaches none of its remaining
instructions either. At a call, the boundary belongs to the caller, and the callee's
activation has closed before the caller asks: the caller leaves by the error exit like
any other.

At a tail call the callee has already replaced the activation's body, so the refusal
ends the activation itself, whichever entrant runs it (`VM::end_refused_activation`).
No frame of it is left to park, not even for a fiber body's restart, so the exit
carries `RaiseSite::TailRefused` and every parker keeps none ([park.md](park.md)).
What the activation owed runs at this exit instead, and where it lives depends on
whether the refused code parked the activation's frame before the boundary saw the
signal:

- **It did.** An emit, a paused callee's return, or a compiled callee's side exit
  moved the frame's locals and dues into the park, and the discard below runs what
  they owed. The live activation keeps only its region map, which the park cloned. A
  walk over the live frame would find the same mappings and release them a second
  time, so it does not run.
- **It did not.** A tail call to a primitive that suspends parks no frame of its own,
  so the live frame still holds its locals and its map, and the walk reads them.

Either way, the activation's dues are released at the exit, owner node included.

The boundary abandons a whole chain rather than one frame. Every frame parked between
the emitting site and the boundary is discarded at one chokepoint,
`VM::discard_suspended_frames`, which each enforcement site reaches through
`VM::boundary_violation`. So the discard reads each discarded `BytecodeFrame`'s two tables
off its own `Code` — the reading `Fiber::take_parked_state` already makes for a fiber
that can never run again, with the frame's saved locals and saved activation map
standing in for the live ones ([owner.md](owner.md)).

The fiber **survives** a boundary, and that is what the reading has to answer for: an
outer, non-discarded frame and the activation the boundary's error reaches both run on.
The tables answer it, and the fiber's survival never enters the argument. A discarded
frame's saved stack and its map were taken and cloned at that frame's own park, and the
activation they came from returned before the boundary was reached, so no live frame
reads either. Within them, each route names only the executing function's own slots and
carries its own receipt, so a slot still holding a heap value, or still mapped, is a
release that frame genuinely owed and did not run. The rest of the parked map stays
untouched for the opposite reason: a blanket release of it has no receipt at all, and
the regions it names may be an outer frame's.

**What the boundary raises exempts nothing, and neither does what it displaces.** The
boundary's error is built by `escaping_error` in a fresh region of its own and records
no delivery mint, so no frame's reference funds its delivery and every release the
tables name runs. The signal the boundary displaces is not the exemption's subject
either: a refused park's payload is delivered to nobody, and the park's own escape
retain holds it independently of any frame, so a frame's reference to it is that
frame's to release like any other. That retain is then the boundary's own to release,
along with whatever a displacing install would have owed — the park having ended with
neither a reader nor an install ([owner.md](owner.md)).

**A compiled frame parks, so the compiled tier walks nothing.** A compiled frame
leaves by an error at the check after a call or after an instruction that may raise,
and at an `Emit` of `SIG_ERROR`. Each exit parks the frame as a bytecode frame at the
raising site's resume ip, from the locals and operands the emitter recorded for that
site ([jit.md](../jit.md)), exactly as a suspend parks it. The parked frame then
answers to the interpreter's seams like any other: a restart replays it, and a
discard reads its tables off its own `Code`.

Every exit — compiled or not — pops the map it pushed, and the park depends on it:
the park clones `last()`, which must be the parking activation's own frame, not a
callee's leftover. `execute_bytecode_saving_stack` asserts the balance in debug
builds, so an exit path that returns without popping detonates at the first
activation to return through it rather than resolving some later release against
the wrong frame.

A compiled frame whose callee suspends or raises parks itself at the post-call check
and returns the yield sentinel, and that exit pops too: the park reads the map first,
so what the pop discards is a frame nothing needs again. Left behind, it is what
`last()` names for the interpreter activation above — which then parks a map that was
never its own — and the remap stack never shrinks back, one frame per park through a
compiled callee. Nothing runs before the pop, because the frame resumes and still
owes its releases to the resumed body.
`jit::compiler::tests::every_compiled_exit_pops_the_region_map` pins it on the emitted
code, where a missing pop is visible without an activation having to return first.

Pinned by [region-error-unwind.lisp](../../../tests/impl/region-error-unwind.lisp) (the leak gauge — the pending
release of a raising call's argument, of two of them, of a binding live across
the raising call, and of an enclosing frame, each bounded beside a control that
raises holding nothing), the `error-payload*` closed controls in
[oracle.lisp](../../../tests/impl/oracle.lisp) (the emitted payload's own region, bounded per face —
raised in the parked body frame, in a parked non-tail callee, handed down as an
owned parameter, and as a two-region struct — beside the native-raise control
whose gap isolates the recorded mint from the walk and discharge) with
[region-error-payload-uaf.lisp](../../../tests/impl/region-error-payload-uaf.lisp) as their guardfree complement (the
payload a catcher stores outward, a borrowed module payload raised repeatedly, a
native raise's unrecorded install, and a restarted `:error` fiber's replay), the
`denied-discard` probe in [oracle.lisp](../../../tests/impl/oracle.lisp) (the per-op rate of what the
tables cannot name), [region-jit-error-unwind.lisp](../../../tests/impl/region-jit-error-unwind.lisp) with
[region-jit-error-unwind-uaf.lisp](../../../tests/impl/region-jit-error-unwind-uaf.lisp) as its guardfree complement (the
compiled face — one subject per compiled error exit, and, on the soundness side,
the caller's binding live across a compiled callee's exit),
[region-error-park.lisp](../../../tests/impl/region-error-park.lisp) with
[region-error-park-uaf.lisp](../../../tests/impl/region-error-park-uaf.lisp) as its guardfree complement (a
callee's error chain restarted, discarded by the fiber's release, and discarded by
a host),
`lir::lower::tests::release::frametables::{frame_release_tables_name_exactly_the_routes_emitted,
a_reassigned_binding_records_no_value_route}` (the tables are the emit sites, so
a route the emitter declined has no entry),
`lir::lower::tests::release::parkmint::{a_non_tail_dynamic_emit_payload_release_carries_its_receipt,
a_borrowed_emit_payload_release_carries_its_receipt}` (each of the two other
sites that record carries both halves of a value route's receipt), with
[region-dynamic-emit-statement-uaf.lisp](../../../tests/impl/region-dynamic-emit-statement-uaf.lisp) as the dynamic site's
guardfree complement and [region-fiber-yield-borrow-uaf.lisp](../../../tests/impl/region-fiber-yield-borrow-uaf.lisp) as the
`Emit` site's (a borrowed yield's body reference, released once where an abort
or a call boundary ends the park and once where a restart replays it), and
[region-error-unwind-uaf.lisp](../../../tests/impl/region-error-unwind-uaf.lisp) (the soundness complement — the payload
the raising native builds while the frame holds its argument, a value the frame
stored into a container that outlives it, a parked frame the restarts system
replays, and a catching frame's own values, all under `--trace=guardfree`).
The squelch face carries the same pair —
[region-squelch-unwind.lisp](../../../tests/impl/region-squelch-unwind.lisp) (the leak gauge: a pending value in the
emitting frame, two of them, an enclosing frame's, and the same under an
`attune` boundary, each bounded beside a violation that has nothing pending) and
[region-squelch-unwind-uaf.lisp](../../../tests/impl/region-squelch-unwind-uaf.lisp) (the soundness complement: what the
catching activation, an outer non-discarded frame, and a longer-lived container
still read after the discard ran) — with
`runtime::tests::ownership::discard_runs_the_abandoned_frames_release_tables`
driving the chokepoint directly, where both routes and the untabled slot beside
each are visible without a program that allocates into them.

