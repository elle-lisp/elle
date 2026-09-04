# Reassigned mutable bindings are 1-slot containers

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
own wherever its read is a whole-value one (next section), which withdraws it
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

**A whole-value read of a 1-slot container takes a counted reference.** The
container releases what it held at every re-store, so a name bound to a bare
`Var` (or `DerefCell`-wrapped) read of it borrows a reference that dies at the
next overwrite. Rule 5's "new reference" pass-through answers it: the read mints
a placeholder region — a call-result region, so the reader carries a value-based
release at its own last use — and takes an `IncrefValueRegion` at the binder
(`RegionInfo::counted_cell_read_sites`, `emit_counted_cell_read_retain`).

The predicate is **re-stored content**, not the container's realization
(`BindingInner::is_one_slot_container`). A captured cell re-stores through
`capture_store_with_rebind`, which decrefs the displaced prior; an uncelled
`@`-mutable local re-stores through the compiler's own drop-on-overwrite. Both
release the reference the read borrowed, so both expose a reader identically, and
splitting the rule by realization would leave the uncelled half relying on a
producer reference the cell may not even own. A reader that is itself celled is
exempt — its own store opcode owns its references.

Counting the read is also what keeps the **donation** available. The reader holds
a reference of its own, so it is not a holder of the container's init region, and
that region's one remaining claim is the cell's — which drop-on-overwrite and the
content drop already release. The reader's own release routes through the
reader's slot, which is bound once and never repointed, so it has an untainted
route by construction where the init region has none. The reference is the test:
`reassign_gate_counts_a_read_of_an_uncelled_cell` for the admission,
`reassign_gate_counts_an_aliased_init` for the alias that is *not* such a read and
so keeps the counted-init route, and
`tests/elle/region-cell-alias-after.lisp` for the measured shape.

An **element** read (`first`/`get`/destructuring) is not a whole-value read and
needs no counting: an element's region is independently counted by its parent's
alloc-time scan, so the parent's demise cascades rather than freeing the element
under the reader.

**A branch is a read of whichever arms read.** What obliges the reader is the
value it ends up holding, not the syntax that selected it: in
`(let [k (if c r s)] …)` the name `k` is, on every path, a borrow out of a
container that re-stores, and the retain the binder takes protects whichever
container's content arrived — `IncrefValueRegion` names the runtime value, so one
instruction covers every arm. The two containers each keep their donation, and
correctly: on the path that did not run, the reader never became a holder of that
container's init.

A **mixed** branch — one arm reading a container, another allocating — takes the
same retain, and the replacement it pays with is per-arm. What the counted read
does is *replace* the reader's source regions with the placeholder, which is how
the reader stops being a holder; for an allocating arm those source regions are
the only thing extending that value's last use out to the reader, so cutting them
would put the arm's own release ahead of the binder's retain. So the descent cuts
the regions of the arms that read a container and **keeps** every other arm's.
Both halves stay balanced, because one `IncrefValueRegion` names whatever value
arrived: on a reading path the retain and the placeholder's release are the
reader's whole account, and the container's own drop-on-overwrite releases what
it holds; on an allocating path the value carries two references — its birth and
the retain — against two releases, that arm's ordinary decref at the reader's
last use and the placeholder's. What the reader is left holding is exactly what
each half needs: the allocating arm's regions, so its value stays extended, and
none of the container's, so the container is its init's sole holder and the
donation runs.

A statement wrapper is descended for the same reason, with one path rather than
several: `(let [k (begin (log) r)] …)` leaves `k` holding exactly what the tail
read, because the walk gives a `Begin` its last expression's regions and nothing
else. What obliges the reader is the value it ends up holding, not the syntax
that selected it, and a `begin` selects one exactly as an arm does.

A branch NO arm of which reads a container is not a read of anything and declines
as any other init does. A path with no value at all — a `Cond` without an else
clause — is one of the arms that read nothing: it contributes no source region to
keep, and carries no reference for the retain or the placeholder release to name,
so both are no-ops on it. (A `Match` needs no else — an unmatched value signals
rather than falling through to one — so its arms *are* every value-producing
path.)

**A version of the container is not an alias of it.** The reader's own source
name is excluded from the arms, because functionalization's `fresh_version` keeps
the name: `(let [x (if c x x)] …)` is the SSA phi carrying `x`'s content past a
conditional `assign`, and every later read of `x` resolves to it. That is the
same forwarding edge "A loop parameter's init source is not a second holder"
(below) describes at a loop — the versions hold **one** reference between them
because a `Var` read mints nothing — so counting the phi would claim a second
reference for a single holding, and the container's returned-binding suppression
would then run against a reader that had not paid for it. So a version arm is one
of the arms that read nothing, and its regions are among the ones the descent
keeps — the phi hands its one reference along exactly as before, and the retain
and the placeholder release that name the runtime value on that path balance each
other. A user rebinding that shadows the container reads as a version too. A
branch of nothing but versions is a read of nothing and declines whole: the
reader keeps holding the container's region and the container keeps the
counted-init route, which costs promptness only.

The reference is the test: `reassign_gate_counts_a_branch_read_of_a_container`
for the admission, `reassign_gate_counts_a_mixed_branch_init` for the mixed
branch whose allocating arm keeps its regions,
`reassign_gate_counts_a_begin_wrapped_read` for the statement wrapper,
`reassign_gate_declines_a_branch_reading_no_container` for the decline,
`reassign_gate_refuses_returned_value` for the phi that must stay uncounted, and
`tests/elle/region-cell-alias-branch.lisp` for the measured shape.

**Every binder form that records the read must emit the retain.** The analysis
side is one function reached from both binder arms of the walk, and what it
records is worth a *release* — the placeholder's value-based decref at the
reader's last use — plus, at the container, a *donation* the reader's own
reference is what pays for. A binder that recorded the read and emitted no retain
would therefore run both halves of the bargain against a reference nobody took:
the container's first overwrite frees the value under the reader, and the
reader's own release then decrefs it a second time. So `lower_let` and
`lower_letrec` each call `emit_counted_cell_read_retain` at the same point — with
the read value on the operand-stack top, ahead of the slot store — and the
file-letrec binder that carries a module-scope reader is covered exactly as a
fn-local `let` is. `Define` records no read site at all, so a `def`-bound reader
stays a holder of the container's init region and the container keeps the
counted-init route. The reference is the test:
`region_container_read_toplevel_uaf` for the module-scope binder.

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
(region-tailcall-arg-transfer). Counted cell reads (below, "Captured reassigned
cells") keep a read from claiming the init region; the backstop is the
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

**Captured reassigned cells.** A captured (`needs_capture`) reassigned
binding is the same 1-slot container realized at runtime — the capture
cell's update increfs the new content and decrefs the displaced prior
unconditionally; there is no fallback to suppress, because the cell's RC
semantics live in the update opcode itself. Its readers are covered by the
general rule above ("A whole-value read of a 1-slot container takes a counted
reference"): the overwrite-release is `capture_store_with_rebind`'s here rather
than the compiler's drop-on-overwrite, and an uncounted alias would be freed
under the reader by it (the captured-alias double-free). The obligation is
scope-independent — a fn-local `is_restorable_capture_cell` read through an
upvalue by a nested closure (the std/process scheduler's `sched-run`
`(let [batch ready] (assign ready @[]) (each pid in batch …))`, where `ready`
is a `make-scheduler` local) is exactly as exposed as a top-level `def @cell`
read, and both are pinned by
`region-reassign-captured-cell-reader.lisp`.

The writer side owes one rule of its own, at the **init**. A compiled-cell
binding's slot holds the CELL, so routing the init value's release through that
slot makes `DecrefValueRegion` reload the slot and — via `result_region_of`,
which unwraps a capture cell — free whatever the cell holds when the release
fires. Once a reassignment has repointed the cell, that is a different, live
value (the capture-cell reassign UAF). So a reassigned captured binding drops
its init's producer reference off the value register at the define
(`store_captured_cell_init`) and the cell-slot routing is skipped; the cell's
own counted reference (taken by the store, `capture_store_with_rebind`) then
holds the init until the next overwrite or the cell's free cascade.

**The reassign is a fact about the BINDING, not about where the assign sits.**
`RegionInfo::captured_reassigned_bindings` names every captured binding some
`assign` repoints, wherever that `assign` appears — including inside a closure
the definition scope encloses. The binding `results` in `(begin (var results
(list)) (defn collect () (assign results (pair 55 results))) (collect))` has a
compiled cell (its define is outside any lambda) and is repointed from inside
`collect`; classifying it by the *assign site*'s scope would call it fn-local,
leave the cell-slot routing in place, and free the reassigned value under the
program that returns it (`region-capture-cell-closure-reassign-uaf.lisp`). A
genuinely fn-local captured binding — defined inside a lambda — is unaffected
either way: its cell is a `populate_env` env cell reached by `StoreCapture`, a
path that never consults this set.

**A read through an env cell is an uncounted borrow: the cell's last use is the
READER's.** `DerefCell` wraps every read of a captured binding, and it emits no
instruction of its own — `lower_deref_cell` delegates to the cell operand and
`lower_var` unwraps the `CaptureCell` through `LoadCapture`. The value it hands
back is still the cell's content, and the load raises no count on it, so the
cell's own lifetime is the borrow's only protection. The cell owns that content
outright (`AdoptCellRegion` links it into the cell's region), so releasing the
cell cascade-frees exactly what the read borrowed out of it.

That makes the wrapper transparent to last-use, not a consumer of it. A
`DerefCell` in operand position hands its `Var` the *deref's* effective last use
— the enclosing call, `let` binding, or statement that consumes the borrow —
which is the same value the identical read of an *uncaptured* local gets, where
no wrapper stands in between. Treating the wrapper as the consumer instead ends
the cell's life at the load, one node ahead of the reader, and the reader then
derefs a page the cell's free cascade already reclaimed. This is the env-cell
statement of the rule [rules.md](rules.md) Rule 4 makes for container reads, and
the mechanism is the one `uncounted_read_sites` uses for `%get`/`%first`/`%rest`:
the container's last use is the READER's, not the read's.

The hazard is latent by construction. A freed page keeps its bytes, so the stale
read returns the right answer and the program looks correct; `--trace=scrub`
blanks a released page's body and turns the same read into a panic at the deref
site ([diagnostics.md](diagnostics.md)). `tests/region_cell_borrow.rs` runs the
shapes with scrub armed, and `tests/elle/region-capture-cell-borrow.lisp` holds
them for the plain corpus.

**Env cells in loops: release once per activation, not per iteration.** A
captured local (`needs_capture` binding defined inside a lambda) and a captured
param are materialized as a per-value env cell by `populate_env` — a
`StoreCapture` into a cell pre-allocated from `capture_locals_mask`, NOT a
compiled `MakeCaptureCell`. (One carve-out: an immutable, never-mutated,
lambda-initialized `letrec` binding — the recursive-closure shape — compiles its
forward cell as a `MakeCaptureCell` in a plain stack slot even inside a lambda,
the same route as top level, so the closure-cycle merge can collapse it;
`BindingInner::letrec_compiled_cell` is the predicate, and such a binding never
has an env cell to release.) That cell is minted **exactly once per activation**
(populate_env runs once when the activation is created), regardless of any loop
the binding's `def` sits in: a `def @s` re-executed each iteration only
re-stores the cell's *content* (StoreCapture: incref the new content's region,
decref the displaced prior). Its `DecrefCellRegion` (the release of the cell
box's own region) must therefore also fire **exactly once per activation**.

Which binder introduced the local decides nothing here. `def @s` and `let [@s …]`
inside a lambda are the same env cell, minted the same way and released the same
way, so both binders record the cell placeholder that arms the release — the
`Let` arm of [`region::infer::walk`](../../../src/hir/region/infer/walk.rs) and
`lower_let` beside their `Define` twins. A binder that records it for one and not
the other leaks one region and one object per activation per such local, which is
the cost of the closure-as-module idiom: a constructor returning a struct of
closures over its own mutable state pays it once per field, on every
construction (`tests/elle/region-let-capture-cell-leak.lisp`).

The binding-chain `decref_point` extension places a cell-release region's
release at the binding's last use. When the only use is a capture by a closure
built inside a loop, that last use sits *inside the loop body*, so the release
fires every iteration. For a closure that is **called in place and dies within
the iteration**, each iteration nets the cell box region `-1`: `+1` for the
closure's capture-incref, `-1` for the closure's free-time cascade, `-1` for the
per-iteration `DecrefCellRegion`. The box is allocated once (`+1`), so it is
freed at the end of iteration 1, and iteration 2 reads the freed (and recycled)
cell — a use-after-free (`as_capture_cell` deref tag mismatch under the plain
VM, a cascade free under `--trace=guardfree`). This bites whether the loop is
single or nested; a binding bound *between* two nested loops (the `(cap2)` shape,
`region-capture-cell-loop-uaf.lisp`) is just one instance. An *escaping* closure does not fault only because its
capture-incref outlives the iteration, masking the over-release as an
accidental balance.

The fix is a release-placement rule, not a new mechanism: a cell-release
region's `decref_point` is hoisted to the **outermost enclosing `While`/`Loop`
node**, which the lowerer emits *after* the loop (the proven post-loop emission
point the bound-outside `capture_loop_ext` extension already uses). The hoist is
sound for every env cell — the box is never re-allocated per iteration, so a
once-per-activation release can only over-keep (until the loop exits), never
mis-free. It composes with the closure-capture incref: an escaping closure's
reference keeps the box alive past the post-loop `DecrefCellRegion`, so the box
dies with the last surviving closure rather than at the loop. Contrast the
ordinary value-binding case: a value *bound inside* a loop IS re-allocated per
iteration, so its release must stay per-iteration — which is exactly why the
`capture_loop_ext` "bound outside" guard refuses to hoist non-cell regions. Env
cells are the exception that guard does not cover, because their allocation is
loop-independent.

The rule is about how many times the release RUNS, so it binds every mechanism
that places one. A branch whose arms each loop over the cell's holder gives the
cell a second placement: the arms that do not hold the region's `decref_point`
get a compensating release of their own, placed after that arm's last use
([`region::infer::compensate`](../../../src/hir/region/infer/compensate.rs)).
Where the arm's use is inside a loop, that point is per-iteration and frees the
once-per-activation box on iteration 1, exactly as the unhoisted `decref_point`
does. So the compensating release takes the same hoist, to the outermost
`While`/`Loop` **contained in that arm** — the loop the arm can host, since a
loop enclosing the whole branch is refused upstream by the loop-invariant guard.
`each` splices its body into one arm per sequence type, which is how ordinary
code reaches this shape; both are pinned by
`tests/elle/region-capture-cell-loop-uaf.lisp`.

The same "the box is not the slot" fact carries the cell's release past the other
placement rule it meets. A frame that ends in a closure tail call runs nothing the
lowerer emits after the `TailCall`, so a `DecrefCellRegion` landing there is
carried back ahead of the call under the frame-held admission
([mechanism.md](mechanism.md) § "A release past a frame-replacing tail call is not
a release"). That admission refuses a **mutated** holder — but only because a
value-routed release reads the holder's slot, and this release reads the box,
which no `assign` repoints (mechanism.md § "A mutated holder poisons its value
route, not its cell box"). So the env cell of a *reassigned* capture relocates
exactly as an unreassigned one does; refusing it strands one box per activation.

**A cell's release lands at or after every release routed through that cell.** A
captured binding whose init allocates owes two releases, and for an env-celled
binding both are addressed by the same env index. The init value's
`DecrefValueRegion` loads the cell RAW and lets `result_region_of` unwrap it to
the content, so it READS the cell's page; the box's `DecrefCellRegion` frees that
page. The value release therefore has to be emitted first.

Where the two land on one `decref_point` the release order already says so: a
`DecrefValueRegion` that unwraps a cell reads deepest and sorts ahead of the
`DecrefCellRegion` that frees the page ([rules.md](rules.md) Rule 4). Across two
points nothing does, and the two points genuinely diverge. Both regions ride the
binding's uses through the binding-chain extension, so they start together; the
value region then takes a second, later bound from its **allocation site's** last
use, which follows the `def` form's own value out to whatever consumes it. The
cell region is a phantom placeholder with no allocation site, so it keeps the
binding-use bound alone. The box is then freed at the capture while the value
release still has the enclosing statement to reach, and that release unwraps a
reclaimed page — a stray release of whatever region id the recycled page spells.

The rule is a clamp, run after every other `decref_point` pass: a cell-release
region's release lands at or after the release of the region a value route reads
through that cell. That is the region the binding's own binder ALLOCATED — the
one entry `record_region_slot` makes against the binder's slot, which for an
env-celled binding is the env index. A region the binding merely NAMES records no
slot and is released by id, reading no page: `(def @c n)` names its parameter's
phantom region and allocates nothing, so its box owes that region's release
nothing. The clamp is a maximum like every other pin, so it only moves the box
release later, and the point it produces is taken out of any enclosing loop by
the once-per-activation hoist above.

Named, tolerated edge (not specific to binding cells — true of every mutable
container): a read consumed *within the same expression* that also removes or
overwrites the value (`(list x (begin (assign x nil) 1))`) can observe the
removal's release mid-expression. The static analysis does not order
intra-expression reads against runtime removals; this is the mutable-store
analogue of the [theory](../../regions/semantics.md)'s cycle incompleteness —
confined to mutation, named here so it is not rediscovered as a separate bug.
