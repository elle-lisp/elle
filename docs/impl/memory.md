# The region memory model

<!-- audited: 2026-09-23 -->

The mission of the region system, the map of its documents, the settled
invariants, and the leak classes that name the open frontier.

This document sits at the top of the region documentation. The mechanism detail
lives in the per-mechanism specs under [region/](region/AGENTS.md); the code
cites those specs, never this document. The plan of work — how to read the
current state, the fix-selection discipline, and the open work in order — is
[assessment.md](assessment.md). Read this for the model, then that for what to
do next.

State is read from the instruments, never from prose. A class is closed when
[the oracle](../../tests/elle/oracle.lisp) measures its representative shape bounded and
`--trace=guardfree` is clean — not when a sentence claims it. Git holds the
history; this document describes the system as it stands.

## The mission

Elle gives the programmer Rust/Verona/Pony-grade control over memory — last-use
reclamation they can reason about — while paying **neither** cost those systems
charge:

- **no runtime cost** — no tracing or scanning collector and, by construction,
  no heap walk to reclaim: reclamation is driven by a per-region recorded
  outgoing-edge table (O(edges)), never by walking heap contents
  ([region/ownership.md](region/ownership.md));
- **no annotation cost** — no `iso`/`mut`/region annotations, no hand-written
  lifetimes or borrows.

The one lever that buys both: **frontload the work into the compiler.** The
programmer writes ordinary code; the compiler infers ownership and frees
promptly. The bar is exact:

> If the programmer can look at the code and see a value is dead, the compiler
> frees it there — silently. A gap between what the programmer sees and what
> the compiler proves is a compiler defect, not a nuance to paper over.

So **over-keep is a defect**: holding a value past its true last use. Bounded
by a small scope it is a promptness nit; bounded by a loop or the program it is
unbounded RSS growth — the limiting factor that blocks long-running production
workloads. "Never leaks, freed at teardown" is a leak in every way that matters
for a process meant to stay up. The fix is never to extend a lifetime to dodge
a use-after-free; it is tighter ownership (give the value a real owner that
dies at the right time) or refusal (leave it Shared, the always-legal
baseline).

### The north star — ownership is the substrate for parallelism

Leak-freedom is the floor, not the point. The two facts the compiler infers to
free promptly — where each value lives, and whether it escapes — are exactly
the facts needed to **dissolve** a computation into its most efficient
realization on each target. A `(map f xs)` over an owned, non-escaping `xs`
exposes no observable closure and no observable region layout, so the compiler
may realize it as a plain loop, a JIT'd group, CPU SIMD, or a wide GPU
dispatch: `xs`'s owned region becomes a device arena, `f` dissolves into the
kernel, and the arena is safe both to ship to the device and to free wholesale
when the dispatch completes. This is the design's reason to exist: inference →
ownership → dissolution into SIMD and heterogeneous execution. Dissolution's
win is **fewer allocations… which the leak oracle does not observe** — its
gauges count allocation events instead
([dissolution.md](dissolution.md)).

### The three legs

1. **Inference, not annotation.** Every value gets an inferred region; escape
   is computed over the canonical (functionalized + ANF) IR
   ([escape.md](escape.md)). The programmer writes nothing.
2. **Ownership, not tracing.** Regions form a single-owner forest rooted at
   fibers; a non-escaping subtree is freed wholesale by subtree drop, interior
   reference cycles included ([region/ownership.md](region/ownership.md)).
   Reference counting is scoped to the genuine cross-fiber residual.
3. **Dissolution.** A closure is a first-class value but the unit of nothing at
   runtime; guided by escape and ownership the compiler dissolves it — loop-ify,
   monomorphize, JIT-group, GPU-dispatch — so one program runs sympathetically
   on every backend ([dissolution.md](dissolution.md)).

The keystone binding the three: escape is the single authority (below).

### What persists, what dissolves

Preserve exactly the entities the programmer reasons about; dissolve everything
else.

- **Fibers persist.** The fiber is the contract and the root of the ownership
  forest. The bar is leak-freedom at the fiber boundary: a long-running fiber
  does not leak, and a driver that runs fibers to completion does not leak.
  Tighter locality is the programmer's lever, not an obligation the compiler
  owes — region fragmentation that is leak-free is not a defect.
- **Closures dissolve.** Every role a closure plays at runtime — sent across a
  thread, JIT-compiled, scheduled, spread across a GPU — is a developmental
  accident. Sending crosses fibers, not closures; compilation units are
  continuations; higher-order calls monomorphize. What creates cross-region
  edges and cycles is shared mutable capture; once the only sharing unit is the
  fiber, all intra-fiber capture is intra-subtree and dies on drop.

## How to read the documents

Each directory's `AGENTS.md` is the generated index; the roles and reading
order are here. Read whole files — the why is in the prose around the code —
and treat the pinning test as the reference for every correctness claim.

**The roadmap** — [assessment.md](assessment.md): the gauges and how to read
them, the fix-selection discipline, the measured dead ends, and the open work
in order. It uses this document's leak-class names and never redefines them.

**The consumer model** — how to write Elle sympathetic to the region system:
[the overview](../regions.md), then
[semantics](../regions/semantics.md), [lifetime](../regions/lifetime.md) and
[performance](../regions/performance.md).

**The implementor's contract** — read before touching region code:

- [region/rules.md](region/rules.md) — the exhaustive correctness obligations
  (Rules 1–8), teardown, and the macro allocation scope. Start here.
- [region/mechanism.md](region/mechanism.md) — the RC-instruction machinery:
  value/slot resolution, coalescing, self-edge elimination, the equivalence
  oracle. It also maps each older placement argument to the document that now
  holds it, so a pointer written against the old single file resolves in one
  step.
- [region/settled.md](region/settled.md) — the settled invariants, one line
  each, with the spec that owns each argument.
- Where a release is **placed**, one document per argument:
  [anchors](region/anchors.md) (binder pins, and what a `break` does),
  [window](region/window.md) (the branch-arm release window),
  [compensate](region/compensate.md) (the counted per-arm routes),
  [relocate](region/relocate.md) (a release past a frame-replacing tail call),
  [replicate](region/replicate.md) (the relocation point and its replicas),
  [signalexit](region/signalexit.md) (what a signal exit owes), and
  [unwind](region/unwind.md) (the abandoned-frame walk).
- The runtime substrate: [model](region/model.md) (id-spaces, the
  per-execution physical-region model, page layout),
  [merging](region/merging.md) (coincident-lifetime collapse; the builder
  seed), [letrec](region/letrec.md) (the closure-cycle merge),
  [ownership](region/ownership.md) (the forest: typestate, subtree drop, the
  edge tables), [owner](region/owner.md) (activation and fiber owner nodes),
  [adopt](region/adopt.md) (the capture and funnel adopts, the root's lifetime
  obligation), and [park](region/park.md) (what a suspended fiber's park
  retains, and who releases it).
- Bindings and cells: [bindings](region/bindings.md) (reassigned mutable
  bindings as 1-slot containers), [reads](region/reads.md) (what a read of one
  takes), and [cells](region/cells.md) (how a captured binding's cell is
  realized).
- Native calls: [effects](region/effects.md) (how each primitive declares its
  region behavior), [clique](region/clique.md) (what a declaration buys the
  solver), [ctx](region/ctx.md) (`NativeCtx`, the allocation capability every
  primitive is handed), and [errors](region/errors.md) (the region-coherent
  error struct).
- Instruments: [diagnostics](region/diagnostics.md) (guardfree, generations,
  scrub, the edge oracle, the over-free counter) and
  [generations](region/generations.md) (stale derefs detonate in debug
  builds).
- Cross-cutting analyses: [escape.md](escape.md) (the one authoritative
  true-escape pass), [dissolution.md](dissolution.md) (HOF-chain loop fusion,
  the third leg), and [selfrec.md](selfrec.md) (why a self-recursive closure is
  cell-free).
- [region/template.md](region/template.md) — code objects: a closure
  template's blueprint, payload, and header.

## The mechanism in brief

Reference altitude only; the cited specs carry the full detail.

### Escape — the single authority

The forest needs **true escape**: does this value outlive the activation or
fiber it was born in. It is one analysis ([escape.rs](../../src/hir/escape.rs)) over the
canonical IR, and the sole authority — the region solver holds no escape facts
of its own; it projects escape's verdict onto regions. Lexical capture is
demoted to a structural hint with no escape-authority. A value escapes through
any of four facets, seeded onto atoms and propagated backward to a fixpoint:
**return**, **store**, **capture** (by an escaping closure, transitively), and
**fiber boundary**. Building it once at the front edge builds the forest's
substrate — and makes GPU offload decidable: a kernel is dissolvable exactly
when its per-element work does not escape. The full spec is
[escape.md](escape.md).

### The ownership cut — Owned vs Shared

Every region is classified once, from escape facts
([region/ownership.md](region/ownership.md)):

- **Owned** — exactly one owner: a parent region, or an activation/fiber node.
  Freed by subtree drop; interior reference cycles reclaim with it. Owned ⇒ RC
  frozen (the `Counted` xor `Owned` typestate).
- **Shared** — referenced across a frontier the compiler cannot bound. Carries
  an incoming reference count whose zero frees it — the only class that carries
  a count, near-empty in practice.

External uniqueness leads; dominance and uniqueness refine. The owner is the
nearest activation dominating all referencers.

### Emit modes — collapse first, link as the fallback

A region free returns its arena's pages to the pool and every object dies with
it. The forest maximizes single-arena allocation, and the modes are ordered by
that:

- **MERGE** (primary — O(1)): collapse static-slot, coincident-lifetime,
  sole-held, non-escaping regions onto one physical region, freed by one
  `DecrefRegion` — the builder-idiom seed ([region/merging.md](region/merging.md))
  and the letrec closure-cycle merge ([region/letrec.md](region/letrec.md)).
- **ADOPT / owner nodes** (fallback — O(members)): where membership is
  runtime-determined, link distinct runtime regions with the adopt ops; every
  path bottoms out in one `free_region_set` primitive
  ([region/adopt.md](region/adopt.md), [region/owner.md](region/owner.md)).
- **edge-RC** for the genuinely-Shared residual.
- **Dissolution**, which removes whole classes from existence
  ([dissolution.md](dissolution.md)).

Domination is structural, never numeric: post-domination is decided over the
scope tree, never by comparing a `compute_order` index or a region id
([region/adopt.md](region/adopt.md)).

### The runtime substrate

- **Two id-spaces, two newtypes.** `StaticRegion` is a compile-time
  per-function slot, remapped per activation; `RuntimeRegion` is a physical,
  pages-owning region minted per allocation execution. The types make the
  confusion a compile error ([region/model.md](region/model.md)).
- **A reclamation typestate.** A region is `Counted(u32)` xor `Owned{owner}`,
  so owned-and-RC'd is unrepresentable; adoption moves `Counted → Owned`,
  consuming the count ([region/ownership.md](region/ownership.md)).
- **The outgoing edge table.** Every cross-region reference is recorded at
  creation through the store/capture/aggregate funnel. Reclamation walks the
  table, never the contents — what makes "no heap walk" literally true. The
  content scan survives only as a debug equivalence oracle.
- **Owner nodes.** A pages-less region used purely as a forest root, realizing
  an activation or fiber owner; parks move the node into the suspended frame,
  and teardown or discard subtree-drops it ([region/owner.md](region/owner.md)).
- **The borrow check.** Deliberately uncounted borrows are stamped with a
  generation-checked handle; a violation panics at the boundary instead of
  corrupting the heap ([region/generations.md](region/generations.md)).

### One semantics, every backend

Region operations are runtime methods reached identically by every tier —
never a per-backend allow/deny list. The whole forest runs unconditionally on
VM and JIT; the WASM tier tolerates the ops structurally (every region
instruction is a no-op there — the measured program-duration over-keep below),
and the MLIR/SPIR-V tier is GPU-ineligible for a region op today. Adding a
region instruction threads every tier coincidently, and several matches are
exhaustive with no `_`, so an omission is a build break, not a silent gap.

Intrinsics are one language ([intrinsics](../intrinsics.md)): a call-position
`%`-op discharges its operand contract at compile time, one fixed lowering
each. The storing/removing/copying ops are type-checked opaque `Funnel` native
calls — the escape-correct store path; everything else is an inline opcode.
`%pair` is the opcode in every compile, so the builder-idiom MERGE seed applies
unconditionally.

## Settled invariants

The invariants the system upholds — every cycle-and-transfer class of the
ownership forest, where each release lands, who owns a value at a boundary,
and the 1-slot container model — are listed one line each in
[region/settled.md](region/settled.md), with the spec that owns each
argument. They read closed in the oracle and are regression-pinned on the
interpreter and the JIT; build on them, do not re-litigate them.

## The leak roots

Ordered by production-path weight. Each open root is a tracked defect — the
bar is measured bounded plus guardfree-clean. The shapes, probes, and
acceptance bars are the roadmap's ([assessment.md](assessment.md)); a closed
root keeps a one-line entry so the name in an old probe or commit still
resolves.

**F1. Stdlib per-call scratch — the dominant production leak.** The everyday
collection/string/HOF API leaks intermediate scratch per call. Two
sub-mechanisms:

- **F1a — ephemeral copy / Fresh-native call-result scratch.** A non-escaping,
  acyclic region minted inside a stdlib body: a fresh slice per
  `(rest array)` step, a fresh accumulator a HOF fills and freezes. No static
  slot names it, it is not a cycle, and it is not RC-released promptly on
  discard. Its close is dissolution — index-walk instead of minting a slice —
  and the residue that is not copy-scratch decomposes into F5 strands.
- **F1b — dispatch-wrapper passthrough. Closed.** Container compensation
  covers the mutable case, and cross-unit dispatch-wrapper monomorphization
  collapses `(put c k v)` to the direct intrinsic at a proven container type
  ([monomorphize.rs](../../src/hir/typeinfer/monomorphize.rs)).

**F2. Fiber suspend/resume park residue.** Park/unpark symmetry holds by
construction ([region/park.md](region/park.md),
[region/owner.md](region/owner.md)), abandoned and parked frames run their
owed releases off the emitter's tables ([region/unwind.md](region/unwind.md)),
and the squelch boundary runs the same walk. What remains open is two shapes
the tables cannot name, each a value with no binding of its own and neither
with a gauge: a literal the raising call materialized straight into an
argument, and a parameter whose release routes through an env slot, which
carries no nil stamp.

**F3. Escape imprecision — closed, with no member ever confirmed.** The one
probe ever filed here measured no escape at all; its residual closed with the
native-result rule ([region/ctx.md](region/ctx.md)).

**F4. Cyclic refusal-to-Shared.** A recorded cycle the forest cannot own stays
Shared — UAF-safe but leaking. What stays open has no probe: the
ambiguous-owner / unemittable-edge subtree (`compute_adopt_edges` refusals),
and a letrec body whose tail is neither a frame exit nor a member value, where
nothing places the mint.

**F5. Named smaller over-keeps.** Each a tracked defect on a settled
mechanism:

- **The used-sibling arm** — the branch-arm window is admitted only where
  escape proves the frame holds the region alone; a region escaping by a
  containment facet keeps the conservative baseline, and a used sibling arm's
  tail release still needs a retain on its own node
  ([region/window.md](region/window.md),
  [region/compensate.md](region/compensate.md)).
- **The poisoned value route** — a release routed through a reassigned
  binding's slot is skipped; what remains is an alias the counted read does
  not claim, which keeps the counted-init route and its over-keep
  ([region/bindings.md](region/bindings.md), [region/reads.md](region/reads.md)).
- **`decref_point` over-extension** and **env-cell release at loop exit** —
  granularity nits kept to stay UAF-safe under ANF
  ([region/bindings.md](region/bindings.md), [region/cells.md](region/cells.md)).

**F6. Retired.** The cursor-walk probe filed under this name measured the
aliased-init donation, not a displaced-value cascade. No probe.

**The one deliberate non-defect.** The mutable cross-fiber cycle with no
bounded dominating activation is not a leak class — it is a genuine
unbounded-lifetime decision the programmer made, the line Project Verona
itself draws ([the theory](../regions/semantics.md)). A tracer to collect it
would forfeit the no-GC thesis; the design makes the shape near-unreachable
and names the boundary.

**Genuine growth is not a leak.** A module-level sink that genuinely retains
every prior reads open correctly — the oracle's discriminator probes are this
by design, and "fixing" one breaks the gauge. A block-local accumulator is
different: it frees at the block's return, so a probe that reads it as growth
is measuring an over-keep.

## The soundness axis — guardfree over-frees

Leak-freedom has two failure modes: a region no mechanism reclaims (the
F-classes above), and a region freed before its true last use (a UAF). The
forest runs unconditionally, so `--trace=guardfree` under the full stdlib is
the soundness gate; any over-free is a first-class defect pinned by a
guardfree fixture (the `region_*_uaf` family in
[tests/integration/elle_scripts/](../../tests/integration/elle_scripts.rs)). This axis is orthogonal to the leak
burndown — closing a leak class does not close it, and it does not close a
leak class.

## The backend-realization frontier

The arena gauges are host-side and tier-transparent, so the interpreter's
probes port under each tier's flag
([region/diagnostics.md](region/diagnostics.md)):

- **MLIR CPU/GPU** is allocation-free by construction — the GPU-eligibility
  whitelist admits no instruction that can put a heap value in a register, so
  the VM reclaims as usual.
- **WASM** is the named program-duration over-keep: every region instruction
  is a structural no-op in its emitter, so every allocating boundary call
  strands its fresh region to teardown. Bounded per compiled leaf call on the
  tiered path; pinned shrink-only in `wasm::tests`. The close — realizing the
  value-targeted release ops as host calls — is unscheduled until the tier
  carries production workloads.
- **SPIR-V device arenas remain unmeasured** — the gauge needs a GPU runtime.
  Do not build GPU offload on the device-arena claim until it lands.

## Verification

State is read from steady-state region growth and the oracle, never from
emitted RC. The trustworthy measurements: live-region and RSS growth across
repeated iterations (a reclaimed class is bounded — slope → 0), each probe
beside a live-growth discriminator that proves the gauge is not dead; and
`--trace=guardfree` under the full stdlib for UAF. A fix is proven by measured
slope → 0 plus guardfree-clean.

[The oracle](../../tests/elle/oracle.lisp) is the single leak-state dashboard: representative
shapes per class, an adaptive sequential rate estimator that catches
sub-integer leaks, and shrink-only pins. `oracle: ok` is a ratchet, not a
certificate — it asserts no leak got worse and no closed class regressed,
never that leaks are gone. An undeclared open probe fails a completeness gate,
so the split cannot drift. How to run all three gauges is
[assessment.md](assessment.md).

## Critical files

- **Escape:** [src/hir/escape.rs](../../src/hir/escape.rs) (`analyze_escape` → `EscapeInfo`).
- **Region analysis:** [src/hir/region.rs](../../src/hir/region.rs);
  [src/hir/region/infer/](../../src/hir/region/infer.rs) —
  [ownership/](../../src/hir/region/infer/ownership/mod.rs),
  [merge.rs](../../src/hir/region/infer/merge.rs),
  [analyze.rs](../../src/hir/region/infer/analyze.rs),
  [compensate.rs](../../src/hir/region/infer/compensate.rs),
  [arms.rs](../../src/hir/region/infer/arms.rs),
  [postdom.rs](../../src/hir/region/infer/postdom.rs).
- **Type-dispatch prune and fusion:** [prune.rs](../../src/hir/typeinfer/prune.rs),
  [monomorphize.rs](../../src/hir/typeinfer/monomorphize.rs),
  [fuse.rs](../../src/hir/typeinfer/fuse.rs).
- **Emit:** [src/lir/lower/](../../src/lir/lower/AGENTS.md) —
  [emitops.rs](../../src/lir/lower/emitops.rs),
  [regionemit.rs](../../src/lir/lower/regionemit.rs),
  [regiondecref.rs](../../src/lir/lower/regiondecref.rs),
  [binding.rs](../../src/lir/lower/binding.rs),
  [lambda.rs](../../src/lir/lower/lambda.rs),
  [control/call.rs](../../src/lir/lower/control/call.rs);
  [src/lir/types/instr.rs](../../src/lir/types/instr.rs);
  [src/compiler/bytecode.rs](../../src/compiler/bytecode.rs).
- **Runtime:** [src/vm/core/region.rs](../../src/vm/core/region.rs);
  [regionstore.rs](../../src/value/fiberheap/regionstore.rs) and
  [regionstore/refcount.rs](../../src/value/fiberheap/regionstore/refcount.rs);
  [src/value/arena/mutate.rs](../../src/value/arena/mutate.rs);
  [src/vm/fiber.rs](../../src/vm/fiber.rs) and
  [src/vm/fiber/refcount.rs](../../src/vm/fiber/refcount.rs);
  [src/value/fiber/delivery.rs](../../src/value/fiber/delivery.rs);
  [src/vm/dispatch/region.rs](../../src/vm/dispatch/region.rs).
- **Backends:** [src/jit/dispatch/region.rs](../../src/jit/dispatch/region.rs) and
  [src/jit/translate/instr/](../../src/jit/translate/instr.rs);
  [src/wasm/instruction/dispatch.rs](../../src/wasm/instruction/dispatch.rs) and
  [src/wasm/regalloc.rs](../../src/wasm/regalloc.rs); [src/mlir/](../../src/mlir/mod.rs).
- **Native effects:** [src/primitives/](../../src/primitives/AGENTS.md) (the
  `RegionEffect` declarations, oracle-checked).
- **The stdlib the F1 class lives in:** [src/core.lisp](../../src/core.lisp),
  [src/stdlib.lisp](../../src/stdlib.lisp), [src/prelude.lisp](../../src/prelude.lisp).
- **Tests and oracle:** [the oracle](../../tests/elle/oracle.lisp) and
  [its probes](../../tests/elle/probe/); [src/runtime/tests/ownership/](../../src/runtime/tests/ownership/);
  the `region-*`/`fiber-*` corpus under [tests/elle/](../../tests/elle/);
  [tests/integration/elle_scripts/](../../tests/integration/elle_scripts.rs);
  [tests/region_process_teardown.rs](../../tests/region_process_teardown.rs).
