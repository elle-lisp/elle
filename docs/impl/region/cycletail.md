# A merged cycle behind a tail call

<!-- audited: 2026-10-21 -->

The release channels a closure-cycle merge's arena takes when the binding scope's body ends in a tail call.

[The letrec closure-cycle merge](letrec.md) owns the shape, the gates and the
drop site. This document owns what happens when the body replaces the frame
before the drop site runs.

## A tail-call body hands the drop to a deferred release

When the body ends in a frame-replacing tail call, the binding-scope
`DecrefRegion` is emitted past the `TailCall` as dead code. The release must
ride the activation's completion instead, for a member callee *or* a non-member
one. **For nearly every callee the compiler cannot know whether a tail call
replaces the frame.** The runtime callee *value* decides it: a
`func.as_closure()` replaces the frame and trampolines, while a
`func.as_native_def()` keeps the frame and falls through to the live scope-exit
drop. A name that looks native reaches a closure often enough, since the stdlib
binds a bytecode `+` over the native one.

The compiler reads one callee only: a binding whose compile-time constant IS a
native function. It is immutable, never mutated, and lowered as a `LoadConst` of
that native, so no other value is ever called (`BindingInner::may_replace_frame`).
The release channel does not use that reading. The merge wires **both** channels
and lets exactly one fire, which costs nothing and gives every callee one shape.

- **A tail call to an SCC member** rides the stranded-cycle channel.
  `lower_letrec` marks the member bindings the body tail-calls
  (`stranded_cycle_bindings`, derived from the body's `is_tail` calls without
  descending into nested lambdas). `tail_callee_defers_release`
  ([defer.rs](../../../src/lir/lower/control/call/defer.rs)) returns true for
  such a callee, read through a **non-upvalue** reference only, so a nested
  closure in the body can never free the arena out from under a later use. The
  `TailCall` then carries the callee channel's `region_of(callee)`, which is the
  merged arena, because a member lives in it.

  That consumer refuses a callee crossing the **fiber** frontier and admits the
  return facet, for the same reason as the merge's own frontier gate ([letrec.md](letrec.md)). This
  deferral runs at the recursion's normal completion, after the `Return` mint
  that funds the caller's reference. The marking is keyed on
  `closure_cycle_members`, so only a member of an **admitted** merge reaches it,
  and the consumer never has to re-argue admission. The consumer still states
  the premise, so the two ends of the channel agree.

- **A tail call to a NON-member** (a native `%add`, a redefined operator `+`, a
  foreign closure `g`) rides an explicit slot instead. The arena is therefore
  exempt from the frame-exit hoist ([relocate.md](relocate.md)). Its
  binding-scope `DecrefRegion` is dead past the frame replacement *by design*,
  and hoisting it ahead of the `TailCall` would make both channels fire.

  The analysis records the tail site in `RegionInfo::cycle_tail_release` (site
  HirId → the merged root region), from
  `ClosureCycleMerge::tail_release_sites`. The lowerer sets the `TailCall`'s
  `deferred_release_slot` to the root's static slot. The runtime resolves that
  slot through the executing activation's region map, since the arena was minted
  during the scope setup. If the callee turns out a **closure**, the frame is
  replaced, and the activation's own deferred set frees the resolved arena once
  (deduped) at whichever end that activation reaches ([owner.md](owner.md)). If
  it turns out a **native**, the frame is not replaced, and the slot is never
  consumed. The live scope-exit `DecrefRegion` then frees the arena. The two
  are mutually exclusive, so exactly one release runs, and the compiler
  classified nothing.

**The arena channel and the callee channel are independent.** The
member/non-member split above decides which channel carries the **arena**'s
release. It says nothing about the callee's **own** region. A non-member callee
that is itself a per-call local closure strands its own `DecrefRegion` at the
very same `TailCall`, and `tail_callee_defers_release` claims it. So one tail
call carries the arena slot and the callee flag together, and the runtime must
run both. Reading them as alternatives reclaims **nothing** where the callee
captures a member: the callee's counted `closure ⊇ cell` edge holds the arena
off zero, so the arena's own decref frees neither region.

The everyday shape is a letrec whose sibling captures a self-recursive member:
`(letrec [go (fn [m] … (go …)) outer (fn [m] (go m))] (outer n))`. The merge
collapses `go`'s closure, its env and its forward cell onto one arena, and the
body tail-calls `outer`. The two channels never name the same region. A merge
MEMBER callee is absent from `cycle_tail_release`, and
`tail_callee_defers_release` refuses every `closure_cycle_members` region. Order
between them is immaterial: each is a decref of a `Counted` region, so the
cascade and the direct decref commute. The `fwd-cell-sib` row of
[region-tail-frame-exit-letrec.lisp](../../../tests/impl/region-tail-frame-exit-letrec.lisp)
pins it, beside the `tail-frame-exit-fwd-cell-sib` probe in
[the branch probes](../../../tests/impl/probe/branch.lisp).
[region-tail-frame-exit-uaf.lisp](../../../tests/impl/region-tail-frame-exit-uaf.lisp)
holds the soundness side.

Interior sibling calls, such as `ev` tail-calling `od` inside the SCC bodies,
never defer. `tail_callee_defers_release` refuses any callee whose region is a
closure-cycle merge member (`RegionInfo::closure_cycle_members`), since the
merge owns the release, and only the body marking overrides that refusal. On a
body with mixed tail exits, `(if c (ev k) (%add (ev k) 0))`, exactly one release
fires per path. The member arm defers through `region_of`, with its
binding-scope drop dead there. The non-member arm takes `deferred_release_slot`
or the live scope-exit drop.

**The by-move boundary.** A cycle member passed **by move as a tail argument**,
`(g od)` with `od` itself rather than `(ev k)`'s result, refuses the whole cycle
to Shared. The new activation's owned-parameter release would decref the merged
arena a second time, colliding with the deferred release. The escape gate does
not catch it, since an opaque callee's argument is not a return or fiber Shared
seed. So the tail gate reads each argument's region-transparent flow bindings,
and refuses when one is an SCC member. That reading mirrors escape's
`tail_sources`: it passes through control, select and deref, and stops at a
`Call`, an `Intrinsic` or a `Lambda`. A member stored into a fresh aggregate and
then passed, `(g (%pair od 1))`, is RC-counted. A member *called* in an
argument, `(g (ev k))`, contributes its result, not itself. Both are admitted. An
unresolvable non-member callee refuses too, since there is no site to key the
deferred release at.

**The refusal answers a callee that REPLACES the frame.** The collision is
between the new activation's owned-parameter release and the deferred release
that frame replacement made necessary. A callee the compiler reads as a native
never reaches the collision. It borrows its arguments and releases none of them,
and it keeps the frame, which leaves the binding-scope `DecrefRegion` live as
the arena's single release. The gate therefore asks whether the tail callee can
replace the frame before it refuses. A **struct literal in tail position**,
`{:a fa :b fb}`, is a `struct` native call taking both members as direct
arguments, so it merges. That is the everyday factory: a `letrec` of mutually
recursive closures over some shared state, handed back in one struct. Where the
native's fresh result keeps the members, it holds them by a cross-region
reference into the arena, RC-counted exactly as a foreign capture is ([letrec.md](letrec.md)). So the
arena survives the binding-scope drop. A callee the compiler cannot read that
way keeps the refusal: a foreign closure `g`, a redefined operator, or an
expression.

## Pinning tests

`region::infer::tests::merge::tailgate` pins the tail channels.
`merge_admits_in_lambda_cycle_with_foreign_tail_callee` and
`merge_admits_native_tail` show that a non-member body tail MERGES and records
`cycle_tail_release`. `merge_refuses_member_passed_by_move_to_foreign_tail`
shows that the by-move boundary still refuses.
`merge_admits_member_passed_by_move_to_native_tail` takes the factory whose tail
is a struct literal over its own members, and the `recur-local-mutual-factory`
oracle probe gauges it.

[region-native-tail-mutual-cycle-uaf.lisp](../../../tests/impl/region-native-tail-mutual-cycle-uaf.lisp)
drives every non-member tail kind, mixed, and per-loop-iteration reclamation.
