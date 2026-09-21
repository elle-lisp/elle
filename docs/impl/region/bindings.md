# Reassigned mutable bindings are 1-slot containers

<!-- audited: 2026-09-21 -->

Implementation-facing: how the solver and lowerer handle a binding that is
reassigned over its lifetime. This specializes Rule 5's mutable-container
treatment (see [rules.md](rules.md)) to a single slot.

A binding that is reassigned (`assign` / `set-cell`) holds different values
over time, so no single static program point names "the value's last use" —
a static last-use release routed through the binding's slot is a category
error: the slot's occupant at the release point is whatever was stored
*last*, not the value whose region is being released (the read-time
mistarget UAF). The model is instead Rule 5's mutable container, specialized
to one slot: **the cell holds exactly one reference to its current content,
released by drop-on-overwrite** (the displaced value's demise is the
overwrite, where the slot still names it), and the final, never-overwritten
value is released by the binding's scope demise (fn-local) or the file-letrec
frame teardown (module scope).

How the cell *sources* that one reference splits by scope, because it must
agree with which of the value's ordinary decrefs are suppressed:

- **Module scope (the cell ADOPTS the producer reference).** The compiler
  suppresses the init *and* every assign-value region's ordinary decref
  (`suppressed_decref_regions`; the final value is released by frame
  teardown), so the producer's single reference is donated to the cell. The
  lowerer therefore emits **no incref-on-store** — drop-on-overwrite is that
  donated reference's sole release. The assign sites are marked
  `donated_overwrite_sites`. An incref-on-store here would be unbalanced
  (born + store − overwrite = +1), holding every displaced prior to teardown:
  an unbounded over-keep on a module mutable reassigned in a long-running loop
  (`runtime::tests::reassign_toplevel_prior_release_is_bounded`).
  **CALL-RESULT content is excluded from the donation** and takes the counted
  store instead, as all fn-local content does. A call result carries a
  second compile-time name for the same runtime value — the opaque placeholder
  region the lowerer releases by value through the ANF temp's slot (rules.md
  Rule 2's bound-result shape) — and the suppression above reaches only the
  value's own source regions, never that placeholder. So the placeholder release
  fires regardless and consumes the callee's one returned reference; donating on
  top of it leaves the cell pointing at a freed value
  (`region-reassign-callresult-store.lisp`, `region-hof-tail-return-uaf.lisp`).
- **Fn-local (the cell takes a COUNTED reference).** A fn-local cell's scope
  *exits*, so its final content has no teardown to fall back on and the cell
  needs a release of its own — which means a reference of its own. The compiler
  therefore suppresses at most the init region's decref (the init is stored
  uncounted at the define, so drop-on-overwrite is its release; where another
  binding names that same value the init takes a counted store as well, and the
  suppression goes away with the donation — see "What the cell donates it must
  hold alone", below) and the lowerer
  **increfs on store** for every assign, whatever produced the value. That one
  reference is released by drop-on-overwrite for each displaced prior and by the
  **content drop** for the final one — the two channels a container's holding
  needs, recorded per binding in `RegionInfo::cell_containers`. A cell that
  forwards its final content into a second cell hands that second channel over
  with it (see "A chain of forwarding edges", below).

  The producer's reference is a *separate* claim, and it is dead at the store:
  from there on the cell's own reference keeps the value alive. So each stored
  value's region is released at its **store site**, pinned there exactly as a
  returned value's is pinned to its `Return`. Two things make the pin necessary
  rather than a nicety. ANF names the stored value in a `let` nested inside the
  assign, so the structural last use is *before* `lower_assign` increfs and
  stores it. And the binding-chain extension would otherwise carry the release
  out to the cell's last use — one release for a region that, in a loop, names a
  different runtime value every iteration, so every value but the last keeps a
  reference nobody drops.

  **The store site is the store that took THAT value.** A cell records one entry
  per store — the site, and the regions of the value stored there — so the pin
  follows the value (`CellStore`). Reading the stores as one set instead pins
  every value at the cell's *last* store, and that is a point the earlier value's
  path need not reach. Two `assign`s in mutually exclusive arms of a branch inside
  a loop are the ordinary shape: the first arm's value is pinned in the second
  arm, so an iteration that takes the first arm again displaces the previous value
  from its own ANF slot before the pin ever runs. That strands one region per
  repeat and grows with the iteration count
  (`tests/elle/region-cell-arm-store.lisp`). Where one region really is stored at
  several sites, the pin is the latest of *those* sites: it must sit after every
  store that takes a reference of it, which is what pinning each store in turn
  computes, the pin rule being a maximum.

  Because no release does double duty, the accounting is per-value in every
  shape: born `+1`, store `+1`, then either the overwrite or the content drop
  `−1` and the store-site producer release `−1`.

The two halves claim *different* references, so a binding must land in exactly one
of them. Landing in both suppresses the assign-value region's ordinary decref (the
module-scope half) while still emitting the counted store (the fn-local half): the
producer's reference then has no release channel and the cell strands one region per
assignment. **The scope split is therefore structural** — module-scope vs fn-local is
read off the walk's lambda depth at the reassignment site, so every visit to that
site must agree on the depth. The solver re-walks an inlinable callee's body at the
call site to discover the cross-region edges buried inside it (`try_inline_call`);
that re-walk enters a `Lambda`'s body directly, so it carries **that lambda's** depth
for its duration and reads the same classification the structural walk reads. The
same depth gates the compiled-capture-cell mints (`Begin`/`Let`/`Letrec` emit a
`MakeCaptureCell` only outside a lambda), which is why the depth — rather than a
per-recording re-walk guard — is where the fact belongs.

**Where the content drop lands.** It is a value route through the cell's own
slot — which value the cell holds at that point is a runtime fact, and loading
the slot reads exactly that (`nil` for a cell never written, whose release is a
no-op). This is the one place a reassigned binding's slot *is* a release route,
precisely because the release names the slot's **current** occupant rather than
some earlier value whose region the compiler picked; a release aimed at a
specific region must still refuse the slot ("a mutated slot is not a release
route", below).

The point is the cell's last access — the latest of its reads and its writes —
with one hoist. A cell **carried across a loop** is re-pointed every iteration,
so a drop inside the body would free what the next iteration reads. Such a cell
is a loop *parameter*: its scope node is the loop itself, so hoisting to that
node puts the one drop after the loop, where the lowerer emits the loop's own
releases. A cell bound **inside** a loop body has a body scope node instead, so
it is not hoisted and drops once per iteration — matching its per-iteration
mint. The hoist is a max rather than a move because a loop's parameters stay
readable past the loop (`(while … (assign acc …)) acc`).

**The counted store is emitted BEFORE the slot store.** `StoreLocal` consumes
the value register, so a retain emitted after it no longer names the stored
value — the emitter brings the operand stack's top instead, which is the
displaced prior (`nil` on the first overwrite). The retain then pins nothing and
the stored value dies at its producer release. Same retain-while-on-top
discipline as `lower_call`'s borrowed-arg retain; pinned by
`region-reassign-callresult-store.lisp`.

**A value a 1-slot container holds is a runtime fact.** Which region the content
lives in is decided per store, so every mint of that content that is not adjacent
to the store — the `Return` handing the final value to the caller — reads the
region off the VALUE. The reason is mechanical: each store discharges its value's
producer claim with a release that *unmaps* the allocation's static slot
(`take_runtime_region_for_drop_slot`), so past the last store the slot names
nothing. `cell_stored_regions` carries the class to the one predicate that
decides the encoding (`coalescible_solver_region`), beside the module-scope
dynamic classes it already refuses. Left coalescible, the mint resolves an
emptied slot and the equivalence oracle detonates, which is the loud face of a
mis-coalesce and what that oracle is for
(`coalescible_refuses_a_cell_stored_value`,
`tests/elle/region-pair-heap-content-uaf.lisp`).

**The gate.** The model trades static releases for suppression plus a
value-based store/overwrite pair, so it is sound only when the cell's claim on a
value region's single compiler-owned reference is exclusive — and the two
questions below are asked per region, over the regions each one governs (the
next section splits them):

- **sole-held** — no other *read, user* binding may hold the region (a
  synthetic ANF producer temp or a write-only statement wrapper is not an
  alias); and
- **not returned** — a *module-scope* cell's region must not appear in any
  return site or lambda tail set. That cell ADOPTS the producer's reference,
  and a return transfers the same reference to the caller, whose value-based
  release consumes it — two static owners of one reference is a double-free.
  A fn-local cell counts what it stores, so it claims nothing the return
  needs and asks the question of nothing (see "Returned fn-local reassigned
  mutables", below).

**What the cell donates it must hold alone; what it counts it need not.**
The sole-held question is asked on behalf of exactly one thing: the
**donation**. The init is the only value a fn-local cell takes uncounted — the
define stores it and the model suppresses its region's ordinary decref, so the
producer's one reference becomes the cell's, and a second binding naming that
value is left with no release of its own and a read that outlives the first
overwrite. Every *assign*, by contrast, takes a counted store, which claims
nothing from anyone: the region keeps its ordinary decref and the cell's
reference is its own.

So an alias of the init is a reason to stop donating, not a reason to refuse the
model. Where another read binding names the init value — `(let [xs (list …)
@r xs] …)`, a cursor walk's shape — the cell **counts its init too**: an
`IncrefValueRegion` ahead of the binder's store, balanced by the same
drop-on-overwrite that balances every later store, while the init region keeps
its ordinary decref — routed, as any release is, through the slot recorded for
it, which is the *allocating* binder's. Nothing is suppressed, so nothing is
claimed twice, and the alias's own read stays safe however late it sits.

That route is the allocating binder's slot, so the shape it serves is the one
whose init the **alias** allocated — `xs` above, whose slot no `assign`
repoints. An alias of a value the **cell's own** binder allocated has no such
slot to offer: the only recorded one is the cell's, which "a mutated slot is not
a release route" (below) refuses. Such an alias instead takes a reference of its
own wherever its read is a whole-value one ([reads.md](reads.md)), which withdraws it
from the sole-held question and hands the donation back — including where only
*some* path of the init reads the container, `(let [k (if c r (list))] …)`, whose
allocating arm keeps its own regions while the container's are withdrawn. What
still leaves the alias a holder, and the container on the counted-init route, is
an init NO path of which is a whole-value read.

Refusing instead costs the **store-site pin**, not merely the donation. On the
unsuppressed baseline the cell holds no reference at all, so each stored value is
protected only by its producer's — whose release the binding chain then extends
out to the cell's last use, one release for a region that names a different
runtime value every iteration. A loop that stores N values then releases one
(`tests/elle/region-cell-aliased-init.lisp`).

The requirement that survives is over the regions the model still *moves*: the
stored values, whose producer release is pinned back to the store site. A cell
whose assign value is aliased keeps refusing, whole. The counted init also needs
a store to retain at, which the chain's source binder supplies; a chain whose
source is a parameter has no such store, so it keeps donate-or-refuse. The
reference is the test: `reassign_gate_counts_an_aliased_init` for the admission,
`reassign_gate_refuses_an_aliased_assign_value` for the decline.

**What a read of the container takes is [reads.md](reads.md).** A whole-value
read borrows a reference the next overwrite kills, so the reader takes a counted
reference of its own. Which reads count, what a branch or `begin` reader is left
holding, and which binder forms emit the retain are that document's; the model
its readers read from is this one's.

**A loop parameter's init source is not a second holder.** `sole_held` counts
distinct *bindings*, and functionalization gives a cell carried across a loop a
second binding for one source name: the `while` becomes a `Loop` whose parameter
is a fresh version of the binding, initialized from the pre-loop version
(`(loop [last#1 last#0] …)`), with every read after the loop resolving to the
parameter. Counting names, the init region reads as two-holder and the gate
refuses any loop-carried cell whose init is a heap value — while a `nil` init,
carrying no region at all, passes.

The count argument says the pair holds **one** reference, not two. A plain `Var`
read mints nothing, so the loop's init edge *forwards* the reference the pre-loop
version held rather than adding one, and that version is dead from the loop's
entry. Admitting the cell puts the init region in `suppressed_decref_regions`,
which is keyed by *region*, so it cancels both names' ordinary decrefs together —
leaving exactly one release channel (drop-on-overwrite for a displaced init, the
content drop for one never displaced) against exactly one reference. So a holder
that is the binding's own loop-init source does not count as an alias of it.

The exclusion is that edge and nothing wider. A **genuine alias** — a *different*
source name bound to the same value, `(var keep last)` — is not a forwarding edge
and keeps refusing the fold, which it must: the region-keyed suppression would
cancel that name's own decref while it still holds the value. What such an alias
costs is the donation alone (above): the cell counts that init instead, and the
alias keeps the decref the fold would have cancelled.

**A chain of forwarding edges hands one reference along, so the fold follows it
whole.** Two sequential loops over one binding give the name three versions —
`last#2 ← last#1 ← last#0`, each `Loop` init the bare `Var` read that mints
nothing — so the three still hold **one** reference between them. The fold
resolves every version to the chain's **last** one, and it resolves the *queried*
binding too: each link asks the gate about the one folded name, which is what
lets a middle link take the model at all.

A middle link differs from `last#0` in the one way that matters: it carries a
1-slot cell of its own, so its content drop is a second channel for the reference
the chain forwards. The link that **receives** that reference already releases it
— at its first overwrite, where its slot still names the forwarded value, or at
its own content drop when nothing overwrites it. So a **forwarding** link emits
no content drop. It keeps the two other things a cell owes: drop-on-overwrite for
each prior it displaces, and the store-site pin that discharges each producer's
separate claim.

The suppression is read over the chain rather than over one link. Every link
keeps its **own** assign-value regions' decrefs — one producer release per stored
value — and a downstream link's source regions include every upstream link's,
because the `Loop` init copies them. So a link suppresses only what **no** link
in the chain keeps: the init region, and nothing else. Suppressing an upstream
link's value regions would leave each value that link displaced with a store
incref and no producer release.

The same fact decides *where* those regions are released, against two routes
that would each drag one release past a loop that stores N values. A cell binding
names the slot rather than any one value, so no cell's stored value rides **any**
cell binding's uses — the downstream link's uses sit past the loop the upstream
link stores in. And an **uncounted opcode read** of a cell (`%get`/`%first`/
`%rest`) borrows out of whatever the cell holds now, and the *cell's* reference
is a second protector of that borrow: where the cell drops it at or after the
borrow dies, extending the producer's release to the reader buys nothing, so the
stored value keeps the store-site pin. Where the borrow flows on past the cell's
own last access, the producer's reference is its only protection and the
extension stands. An ANF producer temp is neither a cell binding nor a stored
value, and still extends normally, which is what keeps the release after the
allocation it names. (An uncounted read in **tail** position is a different
question and keeps its own answer: the borrow leaves the activation, so the
return claims the cell's reference and the gate refuses the model.)

The chain is admitted or declined **whole**. A link the gate refuses stays at the
unsuppressed baseline, where each value's ordinary decref is the release of the
producer's reference — and the next link's drop-on-overwrite would then release
that reference a second time. Declining every link together keeps the "one
reference, one channel" accounting true by construction rather than by
coincidence. The reference is the test:
`reassign_gate_keeps_loop_carried_cell_forwarded_from_a_cell` for the admission,
`reassign_gate_refuses_forwarding_chain_with_an_aliased_link` for the decline, and
`tests/elle/region-cell-forward-chain.lisp` for the measured shape.

Module scope never reaches this edge: a top-level reassigned mutable compiles to
a capture cell, and functionalization does not promote a capture cell to a loop
parameter (its RC lives in the cell-update opcode — see "Captured reassigned
cells" below).

Runtime-counted escapes do **not** refuse the gate, deliberately: a store
into another container (the push/put funnel increfs at runtime), a capture
into a closure env (alloc-scan incref, cascade decref), an opaque-call arg
clique (mutual may-store edges whose compile-time increfs the target's
free-time cascade balances), and value-succession into the binding's own
next value (`(assign acc (pair i acc))`, alloc-scan counted) each add a
*counted* reference with its own balanced release — orthogonal to the cell's
claim. Refusing them regresses the canonical accumulator and reassign pins
straight back to UAFs; the boundary is pinned by tests.

The check is per-binding, all-or-nothing: if any held region fails, the
binding falls back entirely. Failing the gate is never a correctness loss —
the fallback is the unsuppressed baseline, where every value region is
released by its ordinary decref at its binding-chain-extended `decref_point`:
over-keeping (a displaced prior lives until the binding's last use), never
mis-freeing — *with one exception the returned-binding case introduces, below,
and one the fallback's own value-route demands a backstop for, next.*

**The fallback's value route is not unconditionally safe — the mutated-slot
backstop.** "Released by its ordinary decref at its `decref_point`" is, for a
call-result region with a known binding slot, a `LoadLocal slot` +
`DecrefValueRegion` (`emit_decrefs_for`): release by the *runtime* value in the
slot, not a static region id. For a TOP-LEVEL (file-letrec) reassigned binding
that route is poisoned: a `(deref-cell x)` read is solved to the cell's INIT
region (the cell-Var walk returns the init region), so the init region's
`decref_point` is extended to that read's last use — which a reassignment has
pushed PAST the first overwrite. The emitted `LoadLocal slot` then loads
whatever the slot holds at the read (a later, live value) and frees *it*, not
the init. That is the no-alias corruption UAF: two file-letrec cells `rc`/`rd`
interleave-reassigned, and reading `rd` returns `rc`'s last value because the
init-region decref, routed through the cell slot, freed a live region
(region-mutable-reassign-flow facet 3; region-mutable-reassign-branch;
region-toplevel-mutable-reassign). So `analyze_regions_with` records the init
and assign-value regions of every top-level reassigned binding in
`RegionInfo::mutated_binding_value_regions` UNCONDITIONALLY (before the gate),
and `emit_decrefs_for` SKIPS the value-routed release for any region there: an
over-keep until file-letrec frame teardown (the final value's region lives in
the cascade-freed frame region — no leak), never a mis-free. When the gate
succeeds these are already in `suppressed_decref_regions` and never reach the
route. Fn-local reassigns are deliberately NOT recorded: their final value's
release *is* a legitimate scope-exit slot route (no teardown root frees it), and
the scope-based solver shares regions, so skipping there leaks an aliased value
(region-tailcall-arg-transfer). Counted cell reads ([reads.md](reads.md)) keep
a read from claiming the init region; the backstop is the
correct-by-construction floor they build on.

**Returned fn-local reassigned mutables — the return claims the MINT's
reference, not the cell's.** Every `Return` mints one owning reference
(`lower_return`'s `IncrefValueRegion`), which the caller balances with a
`DecrefValueRegion` at the call result's `decref_point`. That mint is a
reference the callee did not have a moment earlier, so it takes nothing from
anyone — and a fn-local cell's own reference is likewise its own, taken by the
counted store. Two references, two independent channels: the returned binding
takes the **same container model** an unreturned one takes, and being returned
decides nothing about it. Each channel is exactly one release, and a scheduler
park is what makes a second one fatal rather than latent — a park rebuilds the
value at rc 1, so the extra decref frees it before the caller reads
(`tests/elle/region-reassign-return-park-uaf.lisp`).

The order is what makes the pair exact, and the lowerer supplies it. The mint is
emitted before the `Return` node's own releases (`lower_return`), and the cell's
demise is that node — the tail read of the binding is the cell's last access. So
the sequence at the tail is mint, then content drop: the caller leaves holding
the reference the mint created and the cell's is gone. A loop-carried cell's
displaced priors take drop-on-overwrite exactly as an unreturned cell's do, which
is what keeps the accounting per-value rather than per-binding. Without it every
value but the last is stranded, one region per trip
(`tests/elle/region-loop-acc-return.lisp`).

What the returned binding does still suppress is the binding's OWN regions
(`binding_regs \ kept`). When the binding is assigned ONCE its binding region and
its assign-value region coalesce (`binding_regs == regions`), so there is nothing
to suppress. A **loop** over the cell breaks that coalescing: the binding gets its
own loop-carried region (the slot that carries the accumulator across the
back-edge) DISTINCT from the per-iteration assign-value region, yet both name the
same runtime value at the tail. Leaving both unsuppressed emits a value-route
decref for EACH at the `Return` — two releases of one reference, the second
freeing the caller's minted reference before the caller reads it (the
loop-reassigned-return double-free,
`tests/integration/fixtures/region-capture-cell-string-accum-uaf.lisp`, guardfree
pin `region_capture_cell_string_accum_uaf`).

**A `Return` is a reader of the cell's content.** A stored value's producer
release is pinned to its store site because the cell's counted reference takes
over from there — so anything that borrows the value afterward is protected by
the cell, up to the point the cell drops it. The return borrows exactly that way:
it hands the content out and the mint pays for the caller's copy, so the
producer's claim owes the `Return` nothing. Left unheld, the ordinary
returned-region extension (`return_sites`, `decref::populate_decref_points`)
drags the store-site pin back out to the `Return`, where one release names
whatever the producer's ANF slot holds LAST — every earlier value of a loop
stranded.

The hold-back is the same predicate the uncounted-read extension already asks,
against the same `cell_drop_point`, because it rests on the same fact: the cell's
reference protects a borrow only up to the point the cell drops it. Where the
cell drops the value at or after the `Return`, the extension buys nothing and is
skipped; where it drops EARLIER the producer's reference is the return's only
protection and the extension stands, which costs the store-site pin and leaves
the over-keep — the safe direction to be wrong in. The reference is the test:
`reassign_return_does_not_extend_a_cell_stored_value` for the hold-back, and
`tests/elle/region-loop-acc-return.lisp` (guardfree pin
`region_loop_acc_return_uaf`) for the measured shape.

One obligation binds the fallback for a NON-sole returned binding (left at the
unsuppressed baseline): **a mutated slot is not a release route.** A
value-routed release (`LoadLocal slot` + `DecrefValueRegion`) may target a slot
only if the slot's occupant at the release point is provably the value whose
region is being released; a reassigned binding's slot fails that by
construction. With no untainted route the release is skipped — an over-keep,
never a mis-free.

**Returns mint one owning reference (borrowed captured upvalues).** Every
`Return` (and the native-tail post-block in `src/lir/lower/control.rs`) emits an
`IncrefValueRegion` that hands the caller exactly one owning reference, which the
caller balances with a `DecrefValueRegion` at the result binding's decref_point.
This single mint-at-return convention is what makes a returned **borrowed
captured upvalue** safe. Such a value is owned by the closure env (the
capture-incref, cascade-released when the closure region dies), so this
activation has no claim of its own to hand out. The mint supplies the caller's
reference *without* touching the env's: the caller's `DecrefValueRegion` drains
the mint, not the captured value's rc, so the env keeps holding the upvalue and
the next read is safe (`lib/http.lisp`'s `require-compress` returning the
captured compress module; `tests/elle/region-captured-return-move-uaf.lisp`).
Symmetrically, a freshly-allocated callee result survives its own decref_point
because the mint is emitted *before* it: the producer's claim is released there,
but the mint keeps the value alive for the caller.

Because `lower_return` mints unconditionally, the distinction
between an escaping-closure return (needs a mint) and a same-activation return
(could move) does not affect the return path: both get the mint, and the only
cost of minting a return that *could* have moved is a +1 that the caller's
decref reclaims. Whether a closure escapes its
definition is answered authoritatively by the escape analysis
(`EscapeInfo`/`src/hir/escape.rs`), read by the consumers that genuinely need it
(`tail_callee_defers_release`, the reassign gate's return facet) — not by the return-mint
path, which is unconditional. The tail-call-arg twin (`tail_arg_is_borrowed`,
`src/lir/lower/control.rs`) likewise needs no escape test — its mint is balanced
by the callee's owned-param release, which always fires.

**How a captured binding realizes its cell is [cells.md](cells.md).** A captured
binding is the same 1-slot container this document describes, realized at
runtime: the capture cell's update increfs the new content and decrefs the
displaced prior unconditionally, so there is no fallback to suppress — the
cell's RC semantics live in the update opcode itself. Which realization a
binding takes (a compiled `MakeCaptureCell` in its own slot, or a
`populate_env` env cell), what a read through one borrows, and where the cell's
own release lands are that document's; the model they realize is this one's.

Named, tolerated edge (not specific to binding cells — true of every mutable
container): a read consumed *within the same expression* that also removes or
overwrites the value (`(list x (begin (assign x nil) 1))`) can observe the
removal's release mid-expression. The static analysis does not order
intra-expression reads against runtime removals; this is the mutable-store
analogue of the [theory](../../regions/semantics.md)'s cycle incompleteness —
confined to mutation, named here so it is not rediscovered as a separate bug.
