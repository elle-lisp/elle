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
self-reference borrows the executing closure, which lives in that region). For the dominant
shape — `(letrec [loop …] (loop k))`, and the same body under a `def` — its `DecrefRegion`
lands **past the frame-replacing `TailCall`**, dead code, so without a supplied release the
per-call region leaks. This is the same stranding the forward cell used to suffer; no-cell
removes the cell, not the stranding.

Where the closure's single `DecrefRegion` lands, and who frees the region once, splits by
whether the binding's body **tail-calls** it — and, one level up, by which node the binder
gives the analysis to hang the demise on:

| binding shape | closure `DecrefRegion` placement | who frees the region once |
|---|---|---|
| `letrec`, body tail-calls the binding | letrec scope end — dead code past the `TailCall` | the tail-call **deferred release** |
| `letrec`, body tail-calls another callee | letrec scope end — dead code past the `TailCall` | the **frame-exit relocation** |
| `letrec`, body tail-calls nothing | fires live at the letrec scope end | the live `DecrefRegion` |
| `def`, body tail-calls the binding | the binding's last use IS that `TailCall` — dead code past it | the tail-call **deferred release** |
| `def`, any other body | the binding's last use — the node that CONSUMES the closure | the live `DecrefRegion` |

The tail-call rows are the load-bearing case (the dominant self-recursive helper is a
tail loop). There the closure's release must not run before the recursion completes,
because the recursion re-enters the closure living in that region; freeing it there is a
use-after-free of the closure's own env — the self-call re-dispatch reads a recycled page.

The rows read **per path**, and the premise is a tail call to the binding rather than a
body that is wholly one. A body reaches its tail call through statements and through
branches — `(begin (stmt …) (go k))`, `(if c go (go k))` — and the frame is replaced just
the same. On the paths that take the tail call the scope end is dead and the
deferral supplies it; on a path that falls through instead — a sibling arm, or a callee
that turns out to be a native and keeps the frame — the scope-end `DecrefRegion` runs live
and no deferral is recorded at all (`tail_call_inner` builds `DeferredReleases` only on
the closure arm). The two channels are exclusive per path by construction, so a body whose
paths disagree needs no choice made for it.

Which channel carries the release turns on **who the body tail-calls**, and the two are
exclusive by construction. A body that tail-calls the binding itself makes the closure
region the call's own **callee**, which the frame-exit relocation exempts by design —
moving that release ahead of the call would free the closure the call is about to enter —
so the deferral is its channel. A body that tail-calls anything else has finished with the
closure before the call is made: the recursion has already completed and nothing the call
reaches names the region. The dominant shape is a helper pair —
`(letrec [helper …  go …] (helper (go n)))` — where `go`'s recursion produces the
argument and a sibling consumes it.

- A `letrec` binding's scope **is** a node, so the region analysis carries the closure
  region's demise out to the `Letrec` — which the lowerer emits after the body. When the
  body is a frame-replacing tail call that is dead code, and the two rows above say which
  channel supplies it: the deferral for a member callee, and otherwise the relocation,
  which carries the scope-end release back ahead of the `TailCall` under its own
  frame-held admission ([region/mechanism.md](region/mechanism.md) § "A release past a
  frame-replacing tail call is not a release"). `lower_letrec` marks a cell-free
  self-recursive member `stranded_self_bindings` when the body tail-calls it, reading the
  body's tail callees rather than asking whether the body IS a tail call
  (`lir/lower/binding/let.rs`).
- A `def` has no such node, so its closure region keeps the demise the ordinary binding
  chain computed: the binding's **last use**. That is the tighter of the two placements
  and it needs no relocation, because a use of the binding as a **callee** resolves
  through `last_use` to the node that *consumes* it — the call. The release is therefore
  emitted where that call has returned, so it post-dates the recursion; it is never
  emitted between loading the closure and entering it. The one shape where the point is
  not live is the one where the consuming call is itself the frame-replacing tail call —
  the same dead block the `letrec` rows name, supplied by the same deferral, which is why
  `lower_define` marks the binding `stranded_self_bindings` too.

What keeps the `def` rows off the initializer is the general binder rule
([region/mechanism.md](region/mechanism.md) § "A binder's init release lands after the
slot store"): a `def` evaluates to what it bound, so the unused-binding narrowing floors
its init's demise at the `def` itself rather than pulling it back to the closure's own
`MakeClosure`. A demise there would fire before the binder had stored, freeing the closure
region while the slot still holds `nil`. That floor is the whole of the `def` face's
release discipline; there is no self-recursion-specific suppression.

Both binders' markings are gated on **cell-freedom** (`!needs_capture()`): the
strand+deferral is the release route *only* for a self-recursive binding with no forward
cell. A member that is **also sibling-captured** has a cell that holds a counted reference
to the closure region
and releases it by the cell's cascade — a lifetime that outlives any single tail-call
activation. Stranding such a binding would make the deferred release decref its region a SECOND
time, freeing it under the still-live cell (the scheduler's mutually recursive
`handle-fiber-after-resume` group — each member self-recursive AND sibling-captured;
`region-selfrec-captured-tail-release.lisp`, whose regression SIGSEGVs `process-io.lisp`).
So the cell owns the release for a captured member; the deferral owns it only for the
cell-free case.

In both stranded cases the runtime **deferred release** supplies it.
`tail_callee_defers_release` (`lir/lower/control/call.rs`) returns true for every tail call to a
`stranded_self_bindings` callee (§ "The deferral needs no escape gate"); the `TailCall` then
carries `DeferredReleases::callee = region_of(callee)`, and `trampoline_loop` (`vm/execute.rs`)
decrefs each deferred region exactly once on the recursion's **normal completion** (deduped — a
tail-recursive `loop` re-enters with the same closure each iteration but carries one
stranded decref).

That marking is consulted **before** the predicate's demise reading, which asks whether some
region's `decref_point` is this call node. The stranded binding's release is at its binder's
scope end by definition, so the demise reading answers about a different release entirely,
and a call node that is nobody's `decref_point` would otherwise refuse the one callee whose
release nothing else supplies.

### The deferral needs no escape gate

The deferred release for a stranded self-recursive binding is **unconditional**: it consults
no escape facet at all. The strand loses exactly one reference — the frame's own, taken where
the closure was allocated — and the deferral supplies exactly that one, as a **decref** run at
the recursion's normal completion. Each facet a gate could ask about answers for its own
reason, and none of them is the frame's reference.

The full activation escape (`binding_escapes_activation`) folds in the store
and capture facets — **containment** relations that keep a closure inside the activation's
owned subtree (it dies WITH the activation), not frontier crossings. A cell-free
self-recursive closure held by a local container would then read as escaping and falsely
block the deferral, re-stranding the decref into a leak. (The *capture* facet never applies
to a binding that reaches this gate: a sibling capture would make the binding
`needs_capture`, so it is not cell-free, so the § cell-free gate never strands it — its
forward cell's cascade owns the release instead of the deferral.)

**The return facet is funded by the callee's own `Return` mint.** Read as a count question,
a returned closure is not a reason to withhold the release — the deferral is a *decref*, not
a free, and frees only if it takes the count to zero. Three references are in play over one
call:

- the **frame's own**, taken where the closure is allocated. This is the one the strand
  loses and the only one the deferral is there to supply.
- the **caller's**, minted by the `Return` that hands the closure out — `LoadSelf` in
  return position takes `lower_return`'s `IncrefValueRegion` exactly as any other returned
  value does.
- any **container's**, taken by the store funnel when the closure is stored on the way out
  — the store facet, which never refused.

The deferral runs at the recursion's **normal completion**: `trampoline_loop` breaks only
once the final body has returned, so every `Return` mint on the taken path has already
executed. The order over one call is: frame reference taken, callee mint, deferred release,
caller's release — and between the mint and the caller's release the standing reference is
the caller's. On a path that returns something *else* no mint fired and no one else holds
the region, so the deferral's decref is the last reference and freeing there is exactly
right.

This is the same "the retain on this node funds this release" argument the frame-exit
relocation makes for a returned region ([region/mechanism.md](region/mechanism.md) § "The
callee's return mint, and why the point owes it nothing"), with the ordering the other way
round and therefore nothing to bridge: that relocation moves a release *ahead* of the call
and so needs a captured edge to hold the region off zero until the mint lands; the deferral
runs *after* the mint and needs none.

**The fiber facet is funded by the crossing itself.** A value handed across the fiber
frontier is *delivered*, not borrowed: every route that carries it counts its own reference
at the crossing, so the receiver's hold is never the frame's.

- an **emitted** value (`yield`/`emit`) takes the park retain as it escapes into
  `fiber.signal` (`EscapeSite::EmitEscape`, `handle_emit`), and the resumer consumes that
  retain through its own result release — the delivery hands the resumer one owning
  reference ([region/owner.md](region/owner.md) § "Park/unpark symmetry");
- a **sent** message — the other fiber-frontier seed, `chan/send`'s `Sends` declaration —
  is increfed at the send site and held until the receive builds the result carrying it
  (`release_received_message`, `primitives/chan/prims.rs`);
- a **halted** payload takes the terminal park retain instead (`incref_signal_region`), the
  one signal `handle_emit` deliberately leaves unretained.

The order is structural rather than argued: the crossing is a node of the defining body and
the deferral runs at the recursion's normal completion, so a crossing that executes at all
executes first. A crossing *inside* the recursion suspends, and a suspending exit abandons
the trampoline's whole deferred set (`trampoline_loop`) — a bounded over-keep, never a
second release. So the fiber frontier is the return facet's case again with the receiver's
reference in place of the caller's mint, and there is no body shape the deferral declines.

## Per-call cost, and the irreducible deferred release

A retained self-recursive closure mints exactly **2 objects/call** — the closure and its env
— with no per-call cell, the same cost as a foreign-capturing closure of equal capture arity
(`self_recursive_loop_is_cell_free`). The tail-call **deferred release** is **irreducible for a
cell-free self-tail-loop**: such a closure's region is a per-call allocation whose release is
stranded past the recursive tail call — removing the *self*-cell removed the cell, not the
stranding — so it always needs the deferral to supply that once-only release (§ above).
Cell-freedom buys the per-call object and better locality; it does not remove the deferral.
The one exception is a **sibling-captured** self-recursive
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

They also share the **return-funded** reading of the frontier above. The merge admits a
returned cycle on the same argument — the returned member lives in the merged arena, so the
`Return` mint raises the arena's count, and the member-callee tail deferral runs after it
([region/letrec.md](region/letrec.md) § The frontier gate). The difference is only in what
each has to prove about *placement*: this deferral is the cell-free binding's sole channel
by construction (the two rows of the § table are exclusive), while the merge must first
establish that every tail exit of the letrec body is a member call — otherwise the arena's
live scope-exit drop is reachable and would fire ahead of the mint.

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
  (the strand frees the closure region exactly once).
- `…::self_recursive_define_off_tail_reclaims_per_call` — the other `def` rows of the
  placement table: a body that consumes the recursion's result instead of tail-calling the
  binding releases live at that consuming node, with heap-churning arithmetic to make a
  premature release loud.
- `…::closure_cycle_nested_letrec_reclaims_per_call` — the full-stdlib per-call reclamation
  gauge for a nested self-recursive letrec loop.
- `…::recursive_returned_closure_reclaims_per_call` — the return-funded admission: a
  RETURNED cell-free self-recursive closure still reclaims per call, through both binder
  faces of the dead scope-end drop.
- `…::unused_define_init_reclaims_per_call` — the binder rule underneath the `def` rows: an
  unused `def`'s heap init is released, which it is only if the release lands after the
  slot store (`region-unused-let-binding.lisp` is the `let` face).
- `tests/elle/region-selfrec-return-release.lisp` — the soundness half of the same
  admission under the UAF oracle: every returned handle is RE-ENTERED after the
  deferred release, across allocation churn that recycles a prematurely freed page.
- `tests/elle/region-tail-frame-exit.lisp` § the `def` binder — the four `def` bodies
  of the placement table driven as leak rows, beside the `letrec` faces of the same
  three.
- `tests/elle/region-define-init-release{,-uaf}.lisp` — the binder rule the `def` rows
  rest on, in both directions: an unread `def`'s init is released, and a `def`'s value
  survives every way it leaves the `def`.
- `tests/elle/oracle.lisp` — `recur-local-self` (leak rate 0), `recur-local-self-mint`
  (0, a returned self-recursive closure reclaims) beside `recur-local-foreign-mint`.
