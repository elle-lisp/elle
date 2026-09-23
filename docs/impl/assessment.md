# The region roadmap

<!-- audited: 2026-09-23 -->

The region system's plan of work: the state gauges, the fix-selection
discipline, the measured dead ends, and the open work in order.

[memory.md](memory.md) is the model: the mission, the mechanisms, and the
definition of every leak class named here (F1a, F1b, F2–F5 are its names; this
document never redefines them). Read the model first, then this — then
re-measure before touching code. State is read from the instruments, never
from numbers written here.

This document holds **state, not change**. A closed mechanism belongs in
[the settled invariants](region/settled.md) and in its spec; git holds who
closed it and when. What earns a line here is what is still open, or a
constraint that would otherwise be re-violated.

## Reading the state — three gauges, all three required

```sh
cargo build -p elle && cargo build --release -p elle
./target/debug/elle tests/elle/oracle.lisp                    # leaks: the dashboard + ratchet
cargo test -p elle --test lib region_ -- --test-threads=1     # soundness: the guardfree UAF pins
make smoke-elle ELLE=./target/release/elle CARGO_PROFILE=--release  # semantics: the whole corpus
```

`make smoke-elle` defaults to the debug binary outside CI, which takes hours
rather than about thirty minutes — pass `ELLE`/`CARGO_PROFILE` as above.

- **The oracle** prints the split — `open defects: N across M roots;
  by-design: K` — and a completeness gate fails the run if any open probe is
  undeclared, so the split cannot silently drift. `oracle: ok` is a ratchet,
  not a certificate: every pin is the current measured rate, shrink-only, so a
  green exit asserts no leak got worse and no closed class regressed — never
  that leaks are gone.
- **Guardfree** is the soundness axis, orthogonal to the leak burndown.
  `--trace=guardfree` under the full stdlib is the only trustworthy UAF
  oracle — plain-VM green is not evidence, and neither is a tight leak rate.
  The `region_*_uaf` family in
  [tests/integration/elle_scripts/](../../tests/integration/elle_scripts.rs) is the
  pinned corpus; the full `cargo test` suite OOMs, so run it filtered.
- **The corpus smoke is a real third gauge, not a formality.** Oracle-green
  and guardfree-green together still admit a corpus over-free no pin covers.
  `make smoke-elle` batches the corpus so a killed batch fails loud. Never
  read a batched suite's exit through a pipe (`| tail` reports the pipe's
  exit); use `elle test --summary` or the run DB.

Every task lands documentation → failing counterfactual test → code. A leak
fix is proven by measured slope → 0 plus guardfree-clean, with its pins
lowered — or its probe block deleted — in the same change.

### The resting state

**Every declared probe is closed.** Both dashboards — [oracle.lisp](../../tests/elle/oracle.lisp)
and the io [plumb.lisp](../../tests/elle/plumb.lisp) — read zero open defects, so the ledgers' own burndown is
empty and every probe in them is a closed control: regression insurance for a
settled mechanism, not work. That is not "leaks are gone": the ratchet
asserts nothing regressed, over the shapes somebody wrote a probe for. The
open work is what has no probe, and it is read from § "The open work", never
from the dashboards. A new fix under any root needs a new probe rather than a
re-pin.

The by-design probes must **stay** open — a "fix" that closes one has broken
the gauge: the four live-growth discriminators (object, id, region, and byte
dimensions — each proves its gauge is not dead, so a discriminator reading
closed voids every closed verdict of that run), and the sub-integer estimator
self-test. A block-local accumulator is not genuine growth — it frees at the
block's return; only a module-level sink is.

**The h2 per-request rate reads zero.**
[h2-stress-scoped.lisp](../../tests/elle/h2-stress-scoped.lisp) holds the ceiling,
shrink-only, at two request counts; the merge-inherits-its-entry and
break-relocation mechanisms keep it there
([region/replicate.md](region/replicate.md)). The subject stays live even at
zero, and without a dashboard probe: it is where the last measured defects on
this mechanism came from, and its own gauge-live sink is what says a green
ceiling is the loop reclaiming rather than the gauge dying.

**Direct gauges live outside the dashboards**, all of the ledger's own kind:
[region-error-unwind.lisp](../../tests/elle/region-error-unwind.lisp) (the error exit's release tables),
[region-squelch-unwind.lisp](../../tests/elle/region-squelch-unwind.lisp) (the same tables at a squelch/attune boundary),
[region-boundary-park.lisp](../../tests/elle/region-boundary-park.lisp) (what the park itself owes there),
[region-tail-deferred-exits.lisp](../../tests/elle/region-tail-deferred-exits.lisp) (the deferred tail-call set across all four
exits), and [region-break-loop-replica.lisp](../../tests/elle/region-break-loop-replica.lisp) (the release the breaking
iteration owes). A shape with a direct gauge needs no dashboard probe; what
it needs is to be run, which the corpus smoke does.

## The fix-selection discipline — invariants over shape-patches

Two kinds of fix, opposite long-run behavior:

- **Class-scale (correct-by-construction):** make the bad state
  unrepresentable or remove the shape from existence — the store funnel's
  private accessors, the `Counted` xor `Owned` typestate, subtree drop,
  dissolution. One mechanism greens a probe block.
- **Shape-scale (compensation):** add a pass or case for one named syntactic
  shape. Locally cheaper, and each breeds successors.

**The litmus:** does the fix state an invariant over a structure the compiler
already holds (escape facts, ownership edges, the scope tree), or enumerate a
shape? Prefer the invariant; a shape-patch needs an explicit reason the
invariant form is infeasible.

**The one shape-enumerating locus to watch:** the compensation family
([compensate.rs](../../src/hir/region/infer/compensate.rs) and the gates it drives, including the four
funnel site-lists). Each gate answers a pinned over-free, so none may be
removed casually — and the family only grows. The class-scale close for the
dispatch-wrapper family is dissolving the dispatch shape itself, so a fifth
site-list needs an explicit reason that close is infeasible.

**The lever that keeps beating compensation:** when a per-path release wants
a count argument you cannot supply, ask instead whether the one release is
merely in the wrong **place**. A release moved to a point every path reaches
needs only a placement argument and adds nothing. Its limit: a placement
argument still needs to know the frame holds the value alone for as long as
the frame lives — escape's question, which no arm-structure premise answers —
so every such window is gated on `frame_held_regions` and everything else
keeps the counted route ([region/window.md](region/window.md)). The return
facet costs nothing there, and the fiber facet is a counted holder
([the settled invariants](region/settled.md)).

**Done looks like deletion.** When a class becomes unrepresentable, its probe
block is deleted (keeping one control as regression insurance), not re-pinned
at 0 forever. The ledger shrinks as classes close.

## Constraints — measured dead ends, do not redo

- **Do not defer release to activation completion for an acyclic result.**
  `AdoptIntoActivation` frees at activation completion, not at last use, so
  an acyclic call-result released that way over-keeps at loop scale, where
  the RC cascade reclaims per iteration. For a cycle the activation bound is
  unavoidable and correct. Corollary: the per-op oracle is blind to
  within-activation growth, so any mechanism that defers release to
  activation completion needs a within-activation gauge before it ships.
- **Do not trade a bounded scratch leak for an unbounded time cost.**
  Rewriting `take` over `(->array coll)` removes the reverse-scratch and
  materializes the whole input — O(length) where the first/rest walk is O(k).
  Measure the access pattern, not only the per-op leak.
- **Do not add a compiler-side `%pair` containment edge.** The edge is
  self-balanced; removing or adding it moves the rate by exactly 0. When two
  paths look symmetric, trace the whole incref chain — hand-written increfs
  and the alloc funnel both — before attributing a count to the site you
  happen to be reading.
- **Do not blanket-release a discarded fiber's parked state.** A mapped slot
  can be stale, so releasing every entry of the parked map frees recycled
  ids. What the runtime may release is the compiler's own release tables,
  each entry carrying a receipt ([region/unwind.md](region/unwind.md)), plus
  the deferred tail-call set the activation carries explicitly
  ([region/owner.md](region/owner.md)). Per-value ownership is compiler
  knowledge; do not reconstruct it at the discard.
- **Do not hold a per-activation obligation in the driving loop's own
  local.** The deferred tail-call set was a `Vec` inside `trampoline_loop`,
  discharged on that loop's clean break — so every other exit dropped it, and
  the worst was the **park**, which returns out of the loop and resumes
  through a fresh one. Anything an activation owes belongs on the activation
  (`ActivationDues`). When a release fires "at completion", ask which loop
  holds it and what a suspend does to that loop.
- **Do not admit a per-arm tail release at every arm-last-use node.**
  Symmetry with the global `decref_point` is a placement argument where
  admission needs a count argument: an arm that used the region can hold out
  an uncounted borrow the solver never named. The same-node retain
  requirement and the escape admission on the branch-arm window are required
  ([region/compensate.md](region/compensate.md)). Neither failure shows in
  the guardfree pins — both surface only in the corpus, one under
  `--jit=eager`.
- **Relocating an existing instruction does not waive the count argument.**
  On the path the release did not previously run, it is a new release at
  runtime and owes what any new release owes.
- **Do not run the whole abandoned post-`TailCall` block at a signal exit.**
  Two of the three things in there refuse: the call's own result, whose slot
  a signal exit never stored, and each argument, whose release is the
  ownership move — and the payload may BE that argument. Only the frame's
  extra borrowed-argument retain has a count argument
  ([region/signalexit.md](region/signalexit.md)).
- **A release another channel owns is not a release to move.** Where a
  mechanism exempts a release, ask what stands in — and whether that
  substitute is keyed on the release's position. An argument's substitute
  runs wherever the caller's copy sat; the callee's own region has only the
  deferred channel, keyed on where the release sits, so moving it silently
  deletes it ([region/relocate.md](region/relocate.md)).
- **A probe named after its innermost op is not a diagnosis of that op.**
  Vary the probe's scaffold (statement vs. tail, one call vs. two) before
  its op.
- **Attribute by decomposition, not resemblance.** A shape that looks like a
  known family is not a member of it; the only settling test is removing one
  ingredient and re-measuring. Four reading rules the corpus keeps demanding:
  - Vary the shape's bindings, not only its ops — a name that merely reads a
    value can decide whether a whole model applies to it.
  - A rate flat in input size and keyed on which arm runs is a placement
    fact about the branch. Vary the arm before the op.
  - A release hoisted to a loop node is invisible to a per-op thunk probe:
    the hoist lands at the thunk's loop and measures 0. Whenever a suspected
    leak reads 0, re-measure it inline before concluding it is not there.
  - A rate that grows with N is per-element; constant across N is a per-call
    strand — but a per-element rate can still be an F5 strand, so remove the
    suspected closure or copy and re-measure.
- **`freeze` is not a leak mechanism.** A builder that fills a mutable
  accumulator and freezes it reads 0 in every position and at every element
  count. Where removing `freeze` moves a rate it does so by changing which
  arm wins the `decref_point` max. Do not build move-consumption for it.
- **A closed leak routinely exposes a latent over-free.** Closing a leak runs
  a free path that never executed before, so run the batched corpus smoke and
  the guardfree family per landing. Budget for the other face too, which no
  region gauge sees: a leaked region can be the only thing holding an OS
  resource open, so freeing it hands a descriptor number back
  ([io.lisp](../../tests/elle/io.lisp)). Run the io, fiber, and posix corpus files, not only
  the region ones.
- **Do not chase the may-store clique further; it is discharged — but a
  `Mixed` declaration is never free.** The `Unknown` census over the
  canonical tables is held empty by a build test, the fiber value installers
  declare `Delivers`, and every remaining single-arg `Mixed` declarant's
  store is real (`fiber/propagate`'s handler writes the child chain's one
  counted field). What is left is the escape side, which reads the same
  declaration and does not care how many arguments there are
  ([region/effects.md](region/effects.md)). `git` is the one multi-arg
  `Mixed` declarant left, and the clique's inclusion-side unit tests use it,
  so tightening it needs a replacement declarant first.

## The open work, in order

Priority: **de-risk unproven patterns before shrinking already-proven
mechanisms.** F5's accounting fixes lower leaks on mechanisms that already
work; F1's dissolution leg proves a mechanism the model asserts, so it leads.

Nothing jumps that queue right now: no entry below has a measured rate. Every
one is "write a probe first". A measured defect on a settled mechanism goes
first when one appears again — it is always the cheapest close on the board.

### F1 — the dominant production leak

**Dissolution** (the mission's third leg) is realized by HOF-chain loop
fusion; [dissolution.md](dissolution.md) is the spec, and the seams to read
before widening are under [src/hir/typeinfer/fuse/](../../src/hir/typeinfer/fuse.rs): the pipeline builder
(`build.rs`), the legality gate (`chain.rs`), and the clone whitelist
(`collect.rs`), each with its decline pins in `fuse::tests`.

The pipeline carries every array arm the stdlib has — `map`,
`map-indexed`, `filter`, `take-while`, `drop-while`, `mapcat`, under the
scalar terminals — with the capture gate closed (a call-site lambda literal
may capture; only a cloned template must be non-capturing). Dissolution is a
**realization** goal, not a leak goal: gauged by cumulative allocation counts
(the `dissolution-*.lisp` corpus), with the leak oracle only a non-regression
check and soundness pinned by the `region-*-fuse-uaf.lisp` family. Widening
it is gauged by a new allocation-count subject per op admitted, never by an
oracle re-pin.

**When a new stage reads as a wash, weigh the scaffold before the stage.**
A counter the fused loop advances through the variadic `+` re-mints per
element the closure the pass exists to dissolve, so every counter advances by
the raw `%add` opcode ([dissolution.md](dissolution.md)). The loop the pass emits is code like any other and can carry the
very cost it was written to remove.

**Hand-dissolution of F1a is exhausted for the probed corpus.** What was left
under those probes decomposed into F5 strands, so a new hand-rewrite needs an
explicit reason. F1a has no probe at all — the ephemeral copy-scratch the
model describes still has no gauge of its own, so a new F1a fix needs a new
probe.

Beyond the ops, the widening of the same mechanism is **backend realization**
(below): the fused loop is the substrate for JIT-group / CPU SIMD / GPU
dispatch; until then it runs on the VM.

### F5 — named accounting fixes

No probe is declared here. These are named strands, each a tracked defect on
a settled mechanism; the mechanisms live in
[region/bindings.md](region/bindings.md), [region/reads.md](region/reads.md),
[region/cells.md](region/cells.md) and [selfrec.md](selfrec.md). What remains
open:

- **An escaping region's used sibling arm.** The branch-arm window is
  admitted only where escape proves the frame holds the region alone; a
  region escaping by a containment facet keeps the conservative baseline and
  the counted compensation routes. The gate itself is not the defect — a
  genuinely escaping region is correctly refused — so audit the **verdict**
  first, it being the cheaper half. Two audited sources of an unreal refusal
  are now empty: the native declarations, and the facets whose second holder
  is counted (the fiber facet). What is left is the genuine containment
  escape, where a used sibling arm's tail release still needs a retain on
  its own node.
- **`decref_point` over-extension** and **env-cell release at loop exit** —
  granularity nits kept to stay UAF-safe under ANF. The cell store pin is one
  of them: an assign that reads the cell records the init's region as that
  store's own and pins it there. No probe reads the residue.

### F2 — the dead-continuation residual

The mechanism is settled — the release tables, the deferred set, the squelch
boundary, the park's own two references
([the settled invariants](region/settled.md)) — and every probe this root
carried is a closed control. What remains open are the two values the tables
cannot **name**, each with no binding, no gauge, and no measured rate: a
literal the raising call materialized straight into an argument, and a
parameter released through an env slot, which carries no nil stamp, so a
release that ran reads exactly like one that did not. Giving either a stamped
stack slot is a lowering change, to be weighed against the `StoreLocal` it
costs every call — and it needs a probe of its own first.

The counterweight to keep in view: the discharge that stands in for a dead
continuation's payload release is exact only because every park leaves
exactly one stranded reference ([region/park.md](region/park.md)). A fix
that changes what a park retains changes what the discharge must release;
check both sides together.

### F4 — no probe; two shapes refused to Shared

- A letrec body whose tail is neither a frame exit nor a member value: its
  value is a call result, so nothing places the mint
  ([region/letrec.md](region/letrec.md)).
- The ambiguous-owner / unemittable-edge subtree (`compute_adopt_edges`
  refusals, [region/adopt.md](region/adopt.md)).

**Do not build owner-node admission for the returned self-recursive
closure.** It resembles this class and is not a member: it records no region
cycle and is cell-free, so nothing holds a runtime self-reference. It is a
control at 0 ([selfrec.md](selfrec.md)).

### The backend gauge — SPIR-V remains

The arena gauges are host-side and tier-transparent, so the interpreter's
probes port under each tier's flag. MLIR-CPU is bounded by construction; WASM
is the named program-duration over-keep, pinned shrink-only in `wasm::tests`,
its close unscheduled until the tier carries production workloads. **SPIR-V
device arenas remain unmeasured** — that needs a GPU runtime beside the
corpus. Do not build GPU offload on the device-arena claim until it lands.

## Costs and risks to budget

- Adoption is the O(members) fallback and the mission promises no runtime
  cost; prefer MERGE and dissolution wherever a static slot can name the
  region.
- Deferred release needs a within-activation gauge — the per-op oracle alone
  false-greens it.
- A green dashboard is the resting state, so it discriminates nothing. A task
  that claims to close something must show its own direct gauge failing
  first; re-running the dashboard is the non-regression half and never the
  proof.
- A closed leak routinely exposes a latent over-free, and the exits this most
  recently reached — a park's discharge, a discard, a squelch boundary — are
  the ones with the fewest probes. Run the fiber, io and posix corpus files
  per landing, not only the region ones.
- Run all three gauges on every task; keep the by-design probes open; re-ask
  the compensation locus before adding a gate.
