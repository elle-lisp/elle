# The ANF lift

<!-- audited: 2026-09-23 -->

Which values the ANF lift names with a synthetic binding, and why each name lands on the node it does.

The pass is `anf_lift` in [src/hir/anf.rs](../../src/hir/anf.rs). It runs
immediately after `functionalize`, before `typeinfer` and region analysis. It
names every allocating expression by wrapping it in a synthetic `let` whose body
is the bound variable: `(g (f x))` becomes `(g (let [t0 (f x)] t0))`.

`t0` is a synthetic immutable binding. Region inference runs after ANF and sees
`f`'s call result as bound to `t0`, so escape analysis owns its lifetime through
a single mechanism. After the pass, every heap-allocating value that its frame
releases through a slot has a `Binding`. The lowerer therefore keys slot
ownership entirely off `binding_to_slot`, with no shadow mechanism for unnamed
call results.

The release rules the names serve are in [the region rules](region/rules.md), and
the `Return` mint they pair with is in [the mechanism](region/mechanism.md).

## The name has to land on the node that allocates

A name is worth exactly what the lowerer can key off it, and the lowerer keys off
one map: `record_region_slot` records a binder's slot against
`alloc_region[init.id]`, the region the init node **itself** allocates. A binder
whose init allocates nothing at its own id therefore records nothing. Its value
gets no release route, and the release the solver placed for it emits no
instruction at all.

So "name the allocating value" is a claim about a node, not about a position.
Every position below is read that way.

## What gets wrapped

The traversal recurses through every child. After the recursive call returns,
the parent decides whether to wrap based on the child's position.

**Consumer positions (wrap allocating children):** `Call.func` and
`Call.args[*].expr`; `Intrinsic.args[*]`; `Emit.value`; `Recur.args[*]`;
`Eval.{expr, env}`; `Parameterize.bindings[*].{key, value}`;
`If.{cond, then, else}`; `Cond` clauses (cond and body); `Match.value` and arm
bodies; every `Begin` expression and every `Block.body` expression;
`And`/`Or` elements; `Break.value`; `SetCell.{cell, value}`; `Assign.value`;
`Destructure.value`; `While.{cond, body}`. A non-last `Begin` or `Block`
position discards its value, and the binding's slot is what `emit_decrefs_for`
uses to release the call result region there.

**Binder positions (the binder's own slot is the name):** `Let` / `Letrec` /
`Loop` binding RHS; `Define.value`. An init that allocates at its own id is
recorded against that binder's slot, so wrapping it would chain a second name
for one region.

**Transparent in the lowerer (do not wrap):** `MakeCell.value`,
`DerefCell.cell`. The lowerer is transparent for these, and the implicit
`MakeCaptureCell` happens at the binding site. Wrapping their child makes a
region with no matching allocation.

**Returning positions (name only an owed release):** `Lambda.body`, and the root
of the compilation unit. See the section below.

## A propagating tail is named through, never named

A `Let`, `Letrec`, `Loop` or `Parameterize` hands its **body's** value up
unchanged, and the lowerer stamps no allocation at the form's own id. The form is
therefore the wrong node to name. A wrap around it binds a slot that
`record_region_slot` leaves empty, and a binder that already holds it —
`(let [a (let [x …] [x x])] …)` — holds a name with no route.

So both naming positions descend the tail and name the node they find there. A
consumer position wraps that node. A binder position names it too, because the
binder's own slot cannot stand for a region the init node did not allocate.
`(g (let [x 7] [x x]))` becomes `(g (let [x 7] (let [t [x x]] t)))`, and the
inner walk a fused `mapcat` runs over its function's result reaches its
per-element array the same way ([dissolution](dissolution.md)).

## A returning position names only what it must release

A lambda body and the root of a compilation unit are returning positions. Their
value leaves through the `Return` mint ([return_incref.rs](../../src/hir/return_incref.rs)), which hands the
caller one owning reference. Most values there need no name. A tail call hands
its callee's reference straight through, and a fresh allocation's region has a
release of its own.

Two producers hand this frame an owning reference that only a slot can release:
an `Eval`, which is never a tail call, and a `Call` that is not a tail call. The
root makes the second kind, because `mark_tail_calls` marks no call at the top
level. A `parameterize` body makes it too, because that body is never a tail
position. Left unnamed, the frame's reference is never released, and the
`Return` mint adds the caller's on top of it. Every `(eval '(f …))` would then
hold one region, and so would every function that returned an eval's result.

So a returning position descends its propagating tails, as a consumer does, and
names an `Eval` or a non-tail `Call` it finds there. It names nothing else, so a
tail call stays where it is, unnamed. `wrap_tail_returns` then marks the name's
body as the returned value, which gives the canonical `(let [t e] (return t))`.

Pinned by `hir::anf::tests::tails` and by
[region-eval-return-leak.lisp](../../tests/elle/region-eval-return-leak.lisp),
which the guardfree oracle also runs.

## Idempotence

If a child is already an ANF wrap `(let [t e] (var t))`, the parent does not
re-wrap it. Re-wrapping would chain a redundant synthetic binding, and
`region_to_slot`, which keys on the region, would see two slots claim one
region.
