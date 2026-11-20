# The frame-held admission

<!-- audited: 2026-10-21 -->

Which regions a frame holds alone, so a release may fire on a path where none
fired before without dropping another holder to zero.

## This frame must be the region's only holder

Two mechanisms make a release fire where none fired before:
[the branch-arm window](window.md) anchors a release at a merge, and
[the frame-exit relocation](relocate.md) moves one ahead of a tail call.
Placement is enough for either *only* where this frame holds the region's one
reference. If the new release drops another holder to zero, that is an
over-free. The reachable other holder is an uncounted borrow in a frame that is
**parked** when the release runs, and the resume's uncounted-borrow check
detonates on it ([generations.md](generations.md)). No premise about arm
structure discharges that. It is a count question wearing a placement
question's clothes, and it is the same wall
[the per-arm route](compensate.md) hits.

Escape answers exactly that question, and is the sole authority for it
([escape.md](../escape.md)). What the admission needs is narrower than
"escapes", though: an **uncounted** second holder. So the facets split:

- The **containment** facets, store and capture by a closure that itself
  escapes, hand the value to a holder the frame cannot see. For a declared
  native store, the frame cannot count that holder either. Both refuse.
- The **return** and **fiber** facets each create a holder that is counted at
  the crossing. Each rides along instead (below).

So a region is admitted when three things hold. No holder binding escapes by a
containment facet. Its own release route is unmutated
([a mutated holder](window.md)). It is absent from the fiber frontier's
atomless site half, which no binding names. A region with no holder binding
offers nothing to judge and is refused too. Everything else keeps its in-arm
release and the per-arm compensation routes, which carry a count argument
instead. The mechanisms partition the obligation rather than overlap on it.

`frame_held_regions` in
[region/infer/escape.rs](../../../src/hir/region/infer/escape.rs) computes this
admission once, before any placement pass runs. The window and the lowerer's
frame-exit relocation both read it through `RegionInfo::frame_held_regions`.

## The return facet costs the merge nothing

A region on the **return** frontier is read after the frame is gone, by the
caller, through the reference a `Return` mints. The merge is a point *in this
frame*, so every path that reaches it has already run whatever mint it was
going to run. An arm that hands the region over minted the caller's reference
before it jumped here. The anchored release drops the frame's own reference and
leaves the caller's standing. An arm that hands nothing over leaves the frame's
reference the only one in existence. The head route makes the same per-path
reading ([the return frontier](compensate.md)).

The arms that never arrive take a **replica** ahead of their own `TailCall`
([the branch-arm window](window.md)). A replica *is* a release before the
callee's mint. That gap needs no edge at the point either, for the reason the
exemption already rests on. A callee reaches a value this frame owns as an
**operand** or through its **captured environment**, and both are safe
([the relocation](relocate.md)). So the return facet is admitted for the whole
class, at the anchor and at every replica alike. The point still decides the
exemption alone: a region an arm's own call names keeps its copy in the dead
block as the ownership move.

The shape this closes is the polymorphic helper that returns what it was
handed. `push-all`'s bulk arm is one, and with it every `append`/`concat` over a
byte-family argument, whose one release sits in the index-walk arm the call
never takes. Through the replica it also closes the index-walk fold driver
behind `fold`/`reduce`/`concat`. Its base arm returns the accumulator, which its
recursive arm's callee cannot reach.

## Lexical capture is not a second holder to fear

Reachability through a closure's environment looks like the counter-example to
every one of these admissions. It is not, because the funnel already paid for
it. A by-value capture becomes scannable content of the `Closure` env. The
allocation funnel's cross-region scan increfs the captured region when the
closure is built, and the closure region's free-time cascade decrefs it again.
A capture materialized through a cell takes the same count at the cell store.

Where the ownership forest admits the containment instead, the capture lowers
to `AdoptRegion`/`AdoptCellRegion` and the member's RC is *frozen*. A decref
against an `Owned` region is a structural no-op ([ownership.md](ownership.md)).
Either way the closure's hold is a counted or an owning edge, never the
uncounted borrow this admission protects against. A release of the frame's own
reference cannot be the one that reaches zero.

What *is* refused is capture by a closure that escapes **beyond the return
facet**. There the closure reaches a holder the compiler did not place, and
escape's capture facet already marks every binding such a closure captures. A
closure the frame merely hands back carries its captures on the same counted
edge, so it refuses nothing ([the relocation](relocate.md)).

That is a flow fact. The structural capture graph
(`region::infer::escape::captured_bindings`) marks every captured binding
whether or not its closure ever leaves. That conservatism is right for the
**merge** gate, which asks where a value may *live* and so needs raw
reachability. It is wrong here, where the question is who holds a count.

## A fiber crossing is a counted holder too

The fiber facet reads exactly as capture does, and for the same reason. Every
seam that hands a value to another fiber counts a reference of its own before
this frame runs on. Each direction of each seam:

- **out, at a park.** The emit's `EmitEscape` retain is the delivery reference.
  The resumer's release of the resume result consumes it. The emitting body may
  hold no reference to give up: a capture, an enclosing frame's parameter, a
  module-level binding. There the compiler supplies one, so a park's payload
  carries exactly one body reference beside the delivery's
  ([owner.md](owner.md)).
- **in, at a resume.** The resumer pushes the value onto the parked frame's
  stack and takes nothing for it. The `Emit` mints the reference the resumed
  body holds it by, released at that node's own `decref_point` as a call result
  is ([owner.md](owner.md)).
- **send.** `chan/send`'s seam retains the message's region at the enqueue
  (`EscapeSite::ChanSend`). The buffer holds it until the receive lowers the
  count ([effects.md](effects.md)).

So a fiber crossing leaves a *counted* second holder, and the frame's own
release still drops the only reference it owns. That is why the admission reads
the containment facets (`EscapeInfo::binding_escapes_by_containment`) rather
than everything beyond return. The two halves stand or fall together. Withdraw
the resume value's mint, and a body that parks again holding the value reads
the resumer's freed reference.
[tests/impl/region-fiber-frontier-window-uaf.lisp](../../../tests/impl/region-fiber-frontier-window-uaf.lisp)
drives that shape.

The fiber frontier's **atomless site half** still refuses. A value emitted or
sent with no binding to name it is judged by no holder here at all. It keeps the
conservative baseline, as a region with no holder binding does.

The shape this closes is the owned parameter a frame receives, hands to another
fiber on one path, and reaches the end of on every other.
`wake-select-waiters` takes the completed fiber by tail-call move from
`complete-fiber`, and resumes a select waiter with it. Without the window, its
release sits inside the arm that finds a waiter, and a program with no select
outstanding never runs that arm. Every `ev/spawn` / `ev/join` pair would then
strand the fiber, the closure it was made from, and the `[ok? value]` pair the
join delivered.

The pins that read the projection directly, including a compiled forward cell
that takes its binding's verdict, are in
[region::infer::tests::cells::frameheld](../../../src/hir/region/infer/tests/cells/frameheld.rs).
