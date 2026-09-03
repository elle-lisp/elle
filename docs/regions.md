# Region-Based Memory Management

Elle frees memory at compile-time-known program points: no tracing collector,
no liveness scan, no `Rc<Value>` (a `Value` is a `Copy` 16-byte tag+pointer).
Every value is born in a *region* — a set of pages with a reference count.
The reference count reaches zero at a point the compiler named, the pages are
returned, and the regions that value referenced are decremented in turn. You
never call `free`, and there is no GC pause.

The model is the Tofte–Talpin region calculus for immutable values, completed
with per-region reference counting for the one case TT cannot express —
mutation. There are exactly two measures: a region implementation is **correct**
(never reads freed memory, never frees live memory, never leaks past a value's
last reference) and then **optimal** (how few regions, how little RSS).
Optimization may never buy performance with correctness.

This file is the **index** to the region documentation, split by audience.

## For programmers writing Elle (the consumer model)

Read these to write code that is sympathetic to the memory system and to get the
semantics and performance you want from it:

| Topic | Content |
|-------|---------|
| [regions/semantics](regions/semantics.md) | The model you write against: TT for immutable values, RC for mutation; what escapes stays alive; the one thing that leaks (mutable cross-region cycles); regions are not fibers, not scopes, and are flat. |
| [regions/lifetime](regions/lifetime.md) | How long things live: the naive user model, why your "constants" are immutable but not eternal (re-materialized per `eval`), and what is true after your program ends. |
| [regions/performance](regions/performance.md) | Where region performance comes from: merging, the per-region cost model, and the habits that make code cheap. |

## For implementors of the region system

Read these before touching the region code — the exhaustive correctness contract
and how the compiler and runtime realize it:

| Topic | Content |
|-------|---------|
| [impl/region/rules](impl/region/rules.md) | The correct/optimal discipline, **the Rules 1–8**, the teardown sweep, and the soundness checklist. |
| [impl/region/mechanism](impl/region/mechanism.md) | The RC-instruction mechanism the rules constrain: value- vs slot-resolution, compile-time region selection (coalescing), the dynamic boundary, self-edge elimination, and the equivalence oracle. |
| [impl/region/model](impl/region/model.md) | The two id-spaces (static slot vs runtime physical), the per-execution region model, how constants lower as ordinary allocations, the page layout, and how an `RegionSlice` payload shares its object's region. |
| [impl/region/merging](impl/region/merging.md) | Merging collapses coincident-lifetime regions onto one physical region: the builder-idiom seed, the seed predicate, and the one-slot emission with mint-or-reuse. |
| [impl/region/letrec](impl/region/letrec.md) | The letrec closure-cycle merge — reclaiming immutable mutual-recursion cycles (the closure SCC and its capture cells) by collapsing them onto one arena. |
| [impl/region/ownership](impl/region/ownership.md) | The ownership forest: the `Counted`/`Owned` reclamation typestate, `owned_children`, subtree drop in phases, and the outgoing edge table that reclaims without a heap scan. |
| [impl/region/owner](impl/region/owner.md) | Owner nodes — an activation or fiber as a pages-less forest root: the capture-back-edge SCC and transferred-returned-subtree cuts, park/discard, and fiber teardown. |
| [impl/region/adopt](impl/region/adopt.md) | The interior owner-edges built where no store site names them — the capture adopt and the checked-on funnel adopt — plus the root's post-dominance + emit-order lifetime obligation, and where per-region RC remains. |
| [impl/region/effects](impl/region/effects.md) | Native region effects: the `RegionEffect` declarations (`Immediate`/`Fresh`/`PassThrough`/`Stores`/`Sends`/`Funnel`/`Opaque`/`Delivers`/`Mixed`/`Unknown`), the clique the solver derives, hard edges, and the declaration oracle. |
| [impl/region/ctx](impl/region/ctx.md) | `NativeCtx` — the allocation-and-heap capability handed to every native: it owns the call's region, so a primitive cannot allocate without naming one. The `PrimFn` signature and the `ctx.*` allocation surface. |
| [impl/region/bindings](impl/region/bindings.md) | Reassigned mutable bindings as 1-slot containers: the gate (sole-held, not-returned), the fallback, and captured reassigned cells. |
| [impl/region/template](impl/region/template.md) | Code objects as three things: the compile-time blueprint, the region-resident payload shared by every header built from it, and the per-creation header `MakeClosure` allocates. |
| [impl/escape](impl/escape.md) | The authoritative true-escape analysis: the four facets (return/store/capture/fiber), interprocedural return transparency, its consumers (the reassign gate's return facet, `tail_callee_defers_release`), the recorded divergences, and lexical capture demoted to a structural hint. |
| [impl/region/generations](impl/region/generations.md) | Region generations: page stamping that detonates stale derefs deterministically in debug builds. |
| [impl/region/diagnostics](impl/region/diagnostics.md) | Telling correct from broken: `--trace=guardfree`, the generation panic, `arena/dump`, and the validation/leak-suite scaffolding. |
