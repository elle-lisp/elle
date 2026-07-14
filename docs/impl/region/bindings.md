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
- **Fn-local (the cell takes a COUNTED reference).** The compiler suppresses
  only the init region's decref; each assign-value region's ordinary decref is
  KEPT (it is the scope-exit demise of whatever the cell last holds). The
  producer reference is thus still released on its own, so the cell must
  **incref-on-store** to hold a balanced reference of its own, which
  drop-on-overwrite releases. Removing it would free the value at the producer
  decref before drop-on-overwrite loads it (a UAF).

**The gate.** The model trades static releases for suppression plus a
value-based store/overwrite pair, so it is sound only when the cell's claim
on each value region's single compiler-owned reference is exclusive. Every
region the cell may hold (init and every assign value) must be, and the
solver must verify both before applying the model:

- **sole-held** — no other *read, user* binding may hold the region (a
  synthetic ANF producer temp or a write-only statement wrapper is not an
  alias); and
- **not returned** — the region must not appear in any return site or lambda
  tail set. A return transfers the value's initial reference to the caller,
  whose value-based release consumes it; the cell claims the same reference
  for drop-on-overwrite/teardown. Two static owners of one reference is a
  double-free.

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

**Returned fn-local reassigned mutables.** A returned fn-local reassigned
mutable stays balanced under one rule: exactly one callee decref per
callee-held reference, plus the caller's mint. The mint-at-return convention
supplies the caller's side — every `Return` mints one owning reference
(`lower_return`'s `IncrefValueRegion`) which the caller balances with a
`DecrefValueRegion` at the call result's decref_point — while the callee
releases its own single reference with the reaching **assign-value** region's
ordinary decref. So the callee KEEPS its assign-value decref (dropping it, or
the mint, unbalances the pair into a leak or a double-free — the io/scheduler
cross-fiber guard `tests/elle/region-reassign-return-park-uaf.lisp`, where a
scheduler park that rebuilt the value at rc 1 makes a dropped-mint double-free
fatal rather than latent).

When the binding is assigned ONCE, its binding region and its assign-value
region coalesce onto a single region (`binding_regs == regions`): one callee
decref fires, the mint carries ownership to the caller, balanced — and there
is nothing to suppress. A **loop** over the cell breaks that coalescing: the
binding gets its own loop-carried region (the slot that carries the
accumulator across the back-edge) DISTINCT from the per-iteration assign-value
region, yet both name the same runtime value at the tail. The unsuppressed
baseline then emits a value-route decref for EACH at the `Return` — two callee
decrefs of the one callee-held reference, the second freeing the caller's
minted reference before the caller reads it (the loop-reassigned-return
double-free, `tests/integration/fixtures/region-capture-cell-string-accum-uaf.lisp`,
guardfree pin `region_capture_cell_string_accum_uaf`). So a sole-held returned
fn-local reassigned mutable suppresses the binding's OWN regions
(`binding_regs \ regions` — the init region and the loop-carried region) while
KEEPING the assign-value regions' decref: one callee release plus the mint,
exactly as the single-assign case. The single-assign case has
`binding_regs == regions`, so this suppresses nothing there and leaves the park
guard's baseline untouched. (No drop-on-overwrite for a returned binding: the
displaced intermediates leak as tolerated debt, never a UAF.)

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
semantics live in the update opcode itself. The soundness obligation
therefore falls on readers: **a direct whole-value binding read out of a
reassigned captured cell takes a counted reference** — incref at the bind,
value-based release at the reader's last use, exactly Rule 5's "new
reference" pass-through discipline. An uncounted alias would be freed under
the reader by the next reassignment's overwrite-release (the captured-alias
double-free). The obligation is scope-independent — a fn-local
`is_restorable_capture_cell` read through an upvalue by a nested closure
(the std/process scheduler's `sched-run` `(let [batch ready] (assign ready
@[]) (each pid in batch …))`, where `ready` is a `make-scheduler` local) is
exactly as exposed as a top-level `def @cell` read, and both are pinned by
`region-reassign-captured-cell-reader.lisp`. The read is recognised at the
binding init (`RegionInfo::counted_cell_read_sites`: a bare `Var`/`DerefCell`
of an `is_restorable_capture_cell` source), minted a placeholder region so it
rides `call_result_regions` for the value-based release, and retained by an
`IncrefValueRegion` at the read (`emit_counted_cell_read_retain`). Element
reads (`first`/`get`/destructuring) need no counting: an element's region is
independently counted by its parent's alloc-time scan, so the parent's demise
cascades rather than freeing the element under the reader. A reader that is
itself a capture cell is not counted (its own cell machinery owns its
references); the alias-of-a-mutable-by-a-mutable pairing stays within the
cells' own store/overwrite accounting.

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

Named, tolerated edge (not specific to binding cells — true of every mutable
container): a read consumed *within the same expression* that also removes or
overwrites the value (`(list x (begin (assign x nil) 1))`) can observe the
removal's release mid-expression. The static analysis does not order
intra-expression reads against runtime removals; this is the mutable-store
analogue of the [theory](../../regions/semantics.md)'s cycle incompleteness —
confined to mutation, named here so it is not rediscovered as a separate bug.
