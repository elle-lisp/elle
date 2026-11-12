# An abandoned frame runs the releases it still owes

<!-- audited: 2026-11-12 -->

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
`try`/`protect` in a loop grows without bound. A retry loop and a server request
loop both have that shape.

## The tables name what the frame still owes

The frame will not run again, so no resume reaches the release, and the seam
that ends the park runs it instead. That needs the set the frame still owes, and
the emitter already names it. A **value-routed** release is three instructions,
`LoadLocal s; DecrefValueRegion; StoreLocal s nil`
([two resolutions](mechanism.md)). The slot `s` is its whole identity. The
instruction releases whatever `s` holds at the moment it runs, and the nil stamp
records that it ran. `Code::frame_release_slots` carries those slots per
function. The walk reads them, and a slot still holding a heap value is a
release that did not run.

The slot is the release, not the value. Three facts make the walk run *that*
release rather than a new one:

- A slot belongs to one binding for the whole body. `allocate_slot` counts
  `num_locals` up and never reuses a slot. It stamps each new slot nil where the
  binding is introduced, so the walk finds only that binding's own value.
- The nil stamp is the receipt. Every value route clears its slot as it
  releases, so a non-nil slot is an unrun release and a run one is invisible to
  the walk. An arm whose release the taken path would not have reached reads nil
  for the same reason a replicated release does
  ([the relocation point](replicate.md)).
- A release the emitter declined is not in the table. A slot is recorded where
  the plain value route is *emitted*. So a mutated route, a reassigned binding's
  slot, a cell release naming the box, and a transfer adopt each record nothing.
  The walk can only run a release the frame genuinely had.

Three other sites record, on the same three facts:

| Site | What its slot holds |
|------|---------------------|
| a suspending `Emit` of a borrowed payload | a payload retain the body releases nowhere ([park.md](park.md)) |
| a dynamic `emit` off tail position | the same retain, at its payload argument |
| a tail call's borrowed argument | the retain the call minted, released in the block past the `TailCall` ([signalexit.md](signalexit.md)) |

Each release is a value route in every respect the walk reads. The site
allocates the slot for itself, writes it once before the call, reads it once
after, and clears it as it releases.

The record serves the parks whose continuation never runs and whose payload the
walk does not protect. A **terminal** dynamic `emit` nobody restarts is one. The
catcher consumes the delivery the raise minted, so the retain answers to the
continuation alone. A `fiber/abort` or `fiber/refuse` that raises in place over a
suspending park, left unrestarted, is another. The injected error displaces the
payload the discharge would have released. A call boundary is the third, because
what it discards exempts nothing (below). A suspending tail call parks the
fourth kind. Its exit leaves each argument retain standing, and an argument the
payload embeds is not the payload, so an abort or a drop runs its release. While
a suspending park stands, the walk protects its payload. So
the walk passes over a slot whose value lives in the payload's region, and the
discharge answers for that retain instead.

The slot-resolved release (`DecrefRegion`) is named the same way, by the static
region slot it carries. Its receipt is the activation map itself. The alloc
mints the mapping and the release takes it
(`take_runtime_region_for_drop_slot`), so a slot still mapped is a release that
did not run. `Code::frame_release_regions` carries those. The walk names only
the slots the *executing* function releases for, so every mapping it takes is a
release this frame owed.

The tables are not everything the exit runs. A release the frame took over from
a frame-replacing tail call has no route and no receipt to record. The
instruction that would have run it is the *caller's*, dead past the `TailCall`.
So the activation carries it, parks it, and discharges it at the same seams
([owner.md](owner.md)).

## What the signal carries is not abandoned, unless the raise minted it

The error's payload leaves with the signal, and exactly one **delivery**
reference funds the catcher's read of it. Where that reference comes from decides
what the walk may release:

| Raise | Where the delivery comes from | What the walk does |
|-------|-------------------------------|--------------------|
| a **native** raise | no retain: a fresh error struct funds it with its birth reference, and a payload read out of an argument funds it with the frame's reference to that argument | skips a slot whose value lives in the payload's region, because that release *is* the delivery |
| an **`Emit`** raise, `(error v)` | the `EmitEscape` retain `handle_emit` takes, which the resumer's release of the resume result consumes | skips nothing, because the raise records the mint in the ledger (`Fiber::delivery`, `record_mint`) and the payload matches it (`mint_names`) |
| a **dynamic `emit`** | the mint the signal exit takes for a payload the call received as an argument, recorded there ([park.md](park.md)) | reclaims the frame's own reference: in tail position where the body allocated the payload, and off tail position the site's payload retain |
| an injected `fiber/abort` or `fiber/refuse` | the injection's mint ([effects.md](effects.md)), recorded on each fiber the payload travels through | releases a frame's reference to the payload like any other |

The injection records its mint on the aborted fiber and, where the error escapes
it, on the aborting one too. For the three raises that mint, the frame's own
reference funds nothing. A skip there would strand one region per
raised-and-caught error whose payload the raise chain owns.

The parked frame's discharge reads the same way. `Fiber::take_parked_state`
withholds its payload protection exactly where the mint is recorded. So the
free-path discharge runs each parked frame's owed release for an emitted payload,
and leaves a native payload's release standing. Nothing else survives the frame
either way. A value the frame stored elsewhere is held by a counted edge the
store funnel recorded, and this release cannot take it below that holder's
count.

## The seams that walk

**An error parks its chain, so nothing walks at the raise.** An error exit parks
every frame from the raise to the fiber body, as a suspend does. That is the
raising frame and each caller behind it (`complete_call`,
`do_fiber_first_resume`, `resume_suspended`). A restart replays those
instructions and their releases, so a walk at the raise would release twice.
What a parked frame owes runs where no resume can reach it. The fiber's own
discharge reads the two tables off each parked `BytecodeFrame`, with its saved
locals and its saved activation map in place of the live ones
([owner.md](owner.md)).

**A host discards an error's chain as it discards a refused park.** `eval`,
`import`, the compile hosts, `compile/run-on`, the root driver and macro
expansion run code on the current fiber. So do the other re-entrant callers, and
none can hold a park of that fiber. An error the hosted code raises parks its
chain all the same. The host ends that park through
`VM::discard_suspended_frames` before it raises at its own call. The discard
protects the error's payload, which leaves with the host's error, and runs every
other release the tables name.

**A call boundary abandons frames the same way, so it runs the same walk.** A
`squelch`/`attune` boundary raises a `signal-violation`, and a silent function's
proof raises a `broken-proof` (`VM::enforce_boundary`). The activation the
boundary breaks out of never catches either. So that activation reaches none of
its remaining instructions. At a call, the boundary belongs to the caller, and
the callee's activation has closed before the caller asks. The caller leaves by
the error exit like any other.

At a tail call the callee has already replaced the activation's body. So the
refusal ends the activation itself, whichever entrant runs it
(`VM::end_replaced_activation`). No frame of it is left to park, not even for a
fiber body's restart. So the exit carries `RaiseSite::TailEnded`, and every
parker keeps none ([park.md](park.md)). What the activation owed runs at this
exit instead. Where it lives depends on whether the refused code parked the
activation's frame before the boundary saw the signal:

| The refused code | What runs what the frame owed |
|------------------|-------------------------------|
| parked the frame: an emit, a paused callee's return, or a compiled callee's side exit | the discard below, because the park took the frame's locals and dues. The live activation keeps only its region map, which the park cloned, so a walk over the live frame would release twice and does not run |
| parked no frame: a tail call to a primitive that suspends | the walk, over the live frame's locals and map |

Either way, the exit releases the activation's dues, owner node included.

A compiled tail callee that raises and parks nothing, as a silent function's
error and a kill do, ends the activation the same way and at the same site. The
walk then finds nothing in the live frame. The frame replacement dropped its
locals and emptied its map, and the compiled callee kept its own on an
activation of its own.

The boundary abandons a whole chain rather than one frame. One chokepoint,
`VM::discard_suspended_frames`, discards every frame parked between the emitting
site and the boundary. Each enforcement site reaches it through
`VM::boundary_violation`. The discard reads each discarded `BytecodeFrame`'s two
tables off its own `Code`. `Fiber::take_parked_state` makes the same reading for
a fiber that can never run again, with the frame's saved locals and saved map in
place of the live ones ([owner.md](owner.md)).

The fiber **survives** a boundary. An outer, non-discarded frame and the
activation the boundary's error reaches both run on, and the tables answer for
that. A discarded frame's saved stack and map were taken at that frame's own
park. The activation they came from returned before the boundary, so no live
frame reads either. Within them, each route names only the executing function's
own slots and carries its own receipt. So a slot still holding a heap value, or
still mapped, is a release that frame owed and did not run. The rest of the
parked map stays untouched. A blanket release of it has no receipt, and the
regions it names may be an outer frame's.

**What the boundary raises exempts nothing, and neither does what it displaces.**
`escaping_error` builds the boundary's error in a fresh region and records no
delivery mint. So no frame's reference funds its delivery, and every release the
tables name runs. The signal the boundary displaces is no exemption either. A
refused park's payload is delivered to nobody. The park's own escape retain
holds it apart from any frame, so a frame's reference to it is that frame's to
release. That retain is then the boundary's own to release, along with what a
displacing install would have owed. The park ended with neither a reader nor an
install ([owner.md](owner.md)).

**A compiled frame parks, so the compiled tier walks nothing.** A compiled frame
leaves by an error at the check after a call, after an instruction that may
raise, and at an `Emit` of `SIG_ERROR`. Each exit parks the frame as a bytecode
frame at the raising site's resume ip, exactly as a suspend parks it. It parks
from the locals and operands the emitter recorded for that site
([jit.md](../jit.md)). The parked frame then answers to the interpreter's seams
like any other: a restart replays it, and a discard reads its tables off its
own `Code`.

Every exit, compiled or not, pops the map it pushed, and the park depends on it.
The park clones `last()`, which must be the parking activation's own frame, not
a callee's leftover. `close_activation` asserts the balance in debug builds. So
an exit path that returns without popping detonates at the first activation to
return through it. It does not resolve a later release against the wrong frame.

A compiled frame whose callee suspends or raises parks itself at the post-call
check and returns the yield sentinel. That exit pops too. The park reads the map
first, so the pop discards a frame nothing needs again. A frame left behind is
what `last()` names for the interpreter activation above. That activation then
parks a map that was never its own, and the remap stack grows by one frame per
park through a compiled callee. Nothing runs before the pop, because the frame
resumes and still owes its releases to the resumed body.
`jit::compiler::tests::every_compiled_exit_pops_the_region_map` pins it on the
emitted code, where a missing pop shows without an activation having to return
first.

## Pinned by

| Test | What it pins |
|------|--------------|
| [region-error-unwind.lisp](../../../tests/impl/region-error-unwind.lisp) | the leak gauge: the pending release of a raising call's argument, of two, of a binding live across the call, and of an enclosing frame, each beside a control that raises holding nothing |
| [region-error-unwind-uaf.lisp](../../../tests/impl/region-error-unwind-uaf.lisp) | its soundness complement under `--trace=guardfree`: the payload the raising native builds, a value stored into a longer-lived container, a replayed frame, and a catching frame's own values |
| the `error-payload*` controls in [oracle.lisp](../../../tests/impl/oracle.lisp) | the emitted payload's own region, per face, beside the native-raise control whose gap isolates the recorded mint |
| [region-error-payload-uaf.lisp](../../../tests/impl/region-error-payload-uaf.lisp) | their guardfree complement: a payload stored outward, a borrowed module payload raised repeatedly, a native raise's unrecorded install, a restarted fiber's replay |
| the `denied-discard` probe in [oracle.lisp](../../../tests/impl/oracle.lisp) | the per-op rate of what the tables cannot name |
| [region-jit-error-unwind.lisp](../../../tests/impl/region-jit-error-unwind.lisp) and [region-jit-error-unwind-uaf.lisp](../../../tests/impl/region-jit-error-unwind-uaf.lisp) | the compiled face: one subject per compiled error exit, and the caller's binding live across a compiled callee's exit |
| [region-error-park.lisp](../../../tests/impl/region-error-park.lisp) and [region-error-park-uaf.lisp](../../../tests/impl/region-error-park-uaf.lisp) | a callee's error chain restarted, discarded by the fiber's release, and discarded by a host |
| `lir::lower::tests::release::frametables` | the tables are the emit sites, so a route the emitter declined has no entry |
| `lir::lower::tests::release::parkmint` and `a_borrowed_tail_argument_release_carries_its_receipt` | each other site that records carries both halves of a value route's receipt |
| [region-dynamic-emit-statement-uaf.lisp](../../../tests/impl/region-dynamic-emit-statement-uaf.lisp) and [region-fiber-yield-borrow-uaf.lisp](../../../tests/impl/region-fiber-yield-borrow-uaf.lisp) | the dynamic and `Emit` sites under guardfree: a borrowed yield's body reference, released once where an abort or a boundary ends the park and once where a restart replays it |
| [region-tail-signal-exit.lisp](../../../tests/impl/region-tail-signal-exit.lisp) | a tail call's argument retain, released once where an abort or a drop ends a suspended park |
| [region-squelch-unwind.lisp](../../../tests/impl/region-squelch-unwind.lisp) and [region-squelch-unwind-uaf.lisp](../../../tests/impl/region-squelch-unwind-uaf.lisp) | the squelch face: pending values in the emitting and enclosing frames, under `squelch` and `attune`, and what still reads after the discard |
| `runtime::tests::ownership::discard_runs_the_abandoned_frames_release_tables` | the chokepoint driven directly, with both routes and the untabled slot beside each visible |
