# Self-recursion: the executing-closure mechanism (no cell)

Implementation-facing: how a self-recursive closure bound inside a lambda body —
`(letrec [loop (fn [m] … (loop …))] …)`, or the same as a nested `def` — refers
to itself without a forward cell, so it is reclaimed by ordinary region RC exactly
like a top-level recursive `defn`. Builds on the region model
([region/model.md](region/model.md)), the escape authority ([escape.md](escape.md)),
and the tail-call deferred release ([region/rules.md](region/rules.md) Rule 5).

## The shape this serves

A `letrec`/`def` binding whose initializer is a lambda that references the same
binding is **captured before its initializer runs**: the lambda is built before the
binding's slot holds a value. The question is how that self-reference is resolved.

The mechanism is: a same-binding self-reference is a first-class analyzer fact,
`CaptureKind::Recursive` (`hir/binding.rs`), classified when a binding's initializer
lambda references that same binding across the lambda boundary
(`hir/analyze/scopes.rs`). It resolves to the **currently-executing closure**, never
to a heap cell.

## Cell-free by construction

A `Recursive` self-edge does **not** mark the binding captured
(`hir/analyze/scopes.rs` skips `mark_captured` for it). A binding captured *only* by
self-references therefore has `needs_capture() == false` (`hir/arena.rs`) — **no
forward cell** is minted for it. This is the split that decides everything, made
once, at `mark_captured`:

- A **self**-edge does not mark → a purely self-recursive binding is cell-free.
- A **sibling/foreign** capture (a *different* closure captures this binding — the
  mutual-recursion case, `ev` capturing `od`) *does* mark → that binding keeps its
  forward cell, which the **closure-cycle merge** collapses
  ([region/letrec.md](region/letrec.md) § The letrec closure-cycle merge). A member
  that is *both* self-recursive *and* sibling-captured uses the executing-closure
  mechanism for its **own** self-edge while still exposing a cell for the sibling.

So a self-reference and a sibling capture are different relations that split cleanly
at one site; no flag, no fork.

## Resolution: the executing-closure register

`Fiber::current_closure` names the closure whose body is currently executing. It is an
**uncounted borrow** — a pure runtime register, not a heap object — live exactly where
a self-reference reads it: a self-recursive body's closure region outlives the
recursion (§ the tail-call deferred release below), so `LoadSelf` never reads a freed value, while
an activation whose body has no self-edge may outlive the value without ever reading
the register ([vm.md](vm.md) § The executing-closure register). It is threaded across every
control-flow boundary (nested call, tail-call frame replacement, suspend/resume, fiber
swap), snapshotted and restored exactly like `activation_region_map` — and across
**every entry path into a closure body**, not only the interpreter call path: the JIT
and WASM fallbacks into the interpreter, the JIT tail-call resolutions, forced-tier
dispatch (`compile/run-on`), the fiber's first resume, `arena/allocs`, macro-transformer
calls, FFI callback trampolines, and spawned-worker bodies each hand the callee value
through the one-shot entry register (vm.md lists the entrants). `LoadSelf` debug-asserts
the register is populated, so an unthreaded entrant fails loudly.

A `Recursive` reference lowers to `LoadSelf` in **every** position (`lir/lower/expr.rs`,
via `current_self_binding`):

| position | meaning |
|---|---|
| value (`loop` returned / stored / passed to a HOF) | materialize the executing closure and use it as a value |
| call (`(loop k)`) | the callee IS the executing closure — re-enter the current `code`+`env` with new args (a self-call re-dispatch), materializing no closure value |

Both are RC-identical to naming the closure through a binding slot, without any cell.
The one op serves every tier: the interpreter reads `current_closure`; the JIT reads a
compiled-body self parameter (and re-enters the same compiled body for a self-tail-call);
WASM reads a reserved self slot the host installs at every closure entry.

## The closure region is per-call and stranded — the deferred release is irreducible

Removing the cell does **not** remove the tail-call deferred release. The self-recursive closure's
own region is a **per-call** allocation whose lifetime spans the whole recursion (the
self-reference borrows the executing closure, which lives in that region). Its scope-end
`DecrefRegion` lands at the enclosing `letrec`/`def` scope, which for the dominant shape
`(letrec [loop …] (loop k))` is **dead code past the frame-replacing `TailCall`** — so
without a supplied release the per-call region leaks. This is the same stranding the
forward cell used to suffer; no-cell removes the cell, not the stranding.

Where the closure's single `DecrefRegion` lands, and who frees the region once, splits by
whether the binding's body is a tail call:

| binding shape | closure `DecrefRegion` placement | who frees the region once |
|---|---|---|
| `letrec`, tail-call body | scope end — dead code past the `TailCall` | the tail-call **deferred release** |
| `letrec`, non-tail body | fires live at scope end | the live `DecrefRegion` |
| `def`, tail-call body | suppressed (`suppressed_self_regions`) | the tail-call **deferred release** |
| `def`, non-tail body | fires live at last use | the live `DecrefRegion` |

The two tail-call rows are the load-bearing case (the dominant self-recursive helper is a
tail loop). There the closure's release must not run before the recursion completes,
because the recursion re-enters the closure living in that region; freeing it there is a
use-after-free of the closure's own env — the self-call re-dispatch reads a recycled page.

- For `letrec`, the region analysis already places the closure region's demise at the
  letrec scope end, which the lowerer emits **after** the body's frame-replacing
  `TailCall` — dead code that never runs. `lower_letrec` marks the binding
  `stranded_self_bindings` (`lir/lower/binding.rs`).
- For `def`, the binding is lowered without its enclosing scope's tail in hand, so the
  region analysis places the closure region's demise at the binding's last use — the
  func-load of the `(loop …)` recursive call — which the lowerer would emit as a **live**
  `DecrefRegion` immediately before that call. `lower_define` therefore SUPPRESSES it
  (`suppressed_self_regions`, checked in `emit_decrefs_for`) and marks the binding
  `stranded_self_bindings`, reproducing the `letrec` path's runtime accounting exactly.

Both markings are gated on **cell-freedom** (`!needs_capture()`): the strand+deferral is the
release route *only* for a self-recursive binding with no forward cell. A member that is
**also sibling-captured** has a cell that holds a counted reference to the closure region
and releases it by the cell's cascade — a lifetime that outlives any single tail-call
activation. Stranding such a binding would make the deferred release decref its region a SECOND
time, freeing it under the still-live cell (the scheduler's mutually recursive
`handle-fiber-after-resume` group — each member self-recursive AND sibling-captured;
`region-selfrec-captured-tail-release.lisp`, whose regression SIGSEGVs `process-io.lisp`).
So the cell owns the release for a captured member; the deferral owns it only for the
cell-free case.

In both stranded cases the runtime **deferred release** supplies it.
`tail_callee_defers_release` (`lir/lower/control/call.rs`) returns true for a tail call to a
`stranded_self_bindings` callee that does not cross a frontier; the `TailCall` then carries
`deferred_release_region = region_of(callee)`, and `trampoline_loop` (`vm/execute.rs`) decrefs each
deferred region exactly once on the recursion's **normal completion** (deduped — a
tail-recursive `loop` re-enters with the same closure each iteration but carries one
stranded decref).

### The deferral's escape gate is the frontier escape, not the full activation escape

The deferred release for a stranded self-recursive binding is gated on the **frontier** escape —
`binding_escapes_via_return ∪ escapes_fiber` (return ∪ fiber) — not
`binding_escapes_activation`. The full activation escape additionally folds in the store
and capture facets — **containment** relations that keep a closure inside the activation's
owned subtree (it dies WITH the activation), not frontier crossings. A cell-free
self-recursive closure held by a local container would then read as escaping (the store
facet is a containment relation, not a frontier crossing) and falsely block the deferral,
re-stranding the decref into a leak. Only a closure actually returned or sent to a fiber
outlives the activation and must not be freed by the new activation, so those two facets —
and only those — block the deferral. (The *capture* facet never applies to a binding that
reaches this gate: a sibling capture would make the binding `needs_capture`, so it is not
cell-free, so the § cell-free gate never strands it — its forward cell's cascade owns the
release instead of the deferral.)

**The residual: a cell-free self-recursive closure that is itself RETURNED.** The two
rows of the table above are exclusive by construction — a stranded binding's release is
the deferral's — so a binding the frontier gate refuses has *neither*: its scope-end
`DecrefRegion` still sits past the `TailCall`, and no deferred channel supplies it. The
frame's own reference to the closure region strands once per call, closure and env
together. The frame-exit relocation does not reach it either: the region is the tail
call's own **callee**, and a release moved ahead of the call would free the closure the
call is about to enter. Measured by the `recur-local-self-mint` probe in
`tests/elle/oracle.lisp` (2 objects/call, beside the non-recursive
`recur-local-foreign-mint` control at 0).

## Per-call cost, and the irreducible deferred release

A retained self-recursive closure mints exactly **2 objects/call** — the closure and its env
— with no per-call cell, the same cost as a foreign-capturing closure of equal capture arity
(`self_recursive_loop_is_cell_free`). The tail-call **deferred release** is **irreducible for the
cell-free case**: a cell-free self-recursive closure's region is a per-call allocation whose
scope-end release is stranded past the recursive tail call — removing the *self*-cell removed
the cell, not the stranding — so a cell-free self-tail-loop always needs the deferral to supply
that once-only release (§ above). Cell-freedom buys the per-call object and better locality;
it does not remove the deferral. The one exception is a **sibling-captured** self-recursive
member: it is not cell-free, and its *sibling forward* cell's cascade — not the deferral — owns
the once-only release (§ the cell-free gate), which is why marking it stranded would
double-free (once by the cascade, once by the deferral). So the deferred release is irreducible precisely
where no external owner (a forward cell) exists, and forbidden where one does.

## Relationship to the closure-cycle merge (mutual recursion)

The **merge** ([region/letrec.md](region/letrec.md) § The letrec closure-cycle merge) serves
**mutual** recursion: sibling-captured members each keep a forward cell, and the merge
collapses the closure SCC and its cells onto one arena. Pure self-recursion is cell-free
and never reaches the merge (it has no cell to merge). A member that is both self-recursive
and sibling-captured keeps a cell for the sibling edge; the single-closure self-edge
admission in the merge collapses that retained cell into the closure's region
(`merge_collapses_self_and_sibling_captured_member_cell`). The two mechanisms compose;
neither forks the other.

## Pinning tests

- `runtime::tests::selfrec::*` — self-recursion correct across value position, nested-lambda
  naming, the §hazard boundaries (survives yield/resume, tail-call frame replacement,
  identity to the base case), and the entry boundaries (the JIT→interpreter fallback and
  tail-call resolution, the forced bytecode tier, the fiber body's first resume, the
  measured thunk) — with `tests/elle/recur-entry.lisp` as the cross-tier corpus peer.
- `runtime::tests::ownership::self_recursive_loop_is_cell_free` — the cell-free mint gauge: a
  retained self-recursive `loop` pins ~2 objects/call, no per-call forward cell (the flip
  gate for a regression that reintroduces a cell).
- `…::self_recursive_loop_reclaims_per_call_no_stdlib` — the per-call closure region is
  reclaimed by the tail-call deferred release (`letrec` tail loop, region-count delta bounded).
- `…::self_recursive_define_with_arith_reclaims_per_call` — a `def` tail-loop recursing with
  heap-allocating arithmetic runs clean and bounded (the heap churn recycles a
  prematurely-freed page, turning the latent use-after-free loud).
- `…::self_recursive_define_in_lambda_no_double_free` — a `def` tail-loop runs to completion
  (the strand + suppress frees the closure region exactly once).
- `…::closure_cycle_nested_letrec_reclaims_per_call` — the full-stdlib per-call reclamation
  gauge for a nested self-recursive letrec loop.
- `tests/elle/oracle.lisp` — `recur-local-self` (leak rate 0), `recur-local-self-mint`
  (2 objects/call, cell-free).
