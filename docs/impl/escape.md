# Escape analysis — the authoritative true-escape pass

Escape is **one** analysis, computed over the canonical (functionalized + ANF)
HIR, and it is authoritative: every consumer that needs to know whether a value
outlives the activation it was born in reads it, rather than recomputing a proxy.
It lives in [`src/hir/escape.rs`](../../src/hir/escape.rs) as `analyze_escape`,
producing an `EscapeInfo` fact-set.

This is the keystone of the region forest: the property the forest classifies
regions by — Owned (reclaimed by subtree drop) vs Shared (reference-counted) —
*is* true escape, so computing it once, over the regularized IR, is building the
forest's substrate at the front edge. The historical alternative — a **syntactic
proxy** (`is_captured`: "a free variable
crosses a lexical function boundary"), computed early as a side-effect of name
resolution — diverges from true escape exactly at the interesting cases (a
closure that lexically captures but never escapes; a value shared across fibers)
and is baked before the regularizing passes that would change it. Lexical capture
is therefore demoted to a **structural hint with no escape-authority** (its
surviving roles are below).

## The two questions

`EscapeInfo` answers two questions; consumers query through methods, never the
private sets, so the representation can change without touching call sites.

- **per binding** — does the binding's value *escape its defining activation*?
  `binding_escapes_activation(b)`. Absence = `false`.
- **per lambda** — does the closure *escape its definition*?
  `lambda_escapes_definition(id)`. Absence = `false`.

A third, narrower query exists for one consumer:

- **per binding, return facet only** — does the value escape *specifically by
  flowing to a tail/return*? `binding_escapes_via_return(b)`. This is a strict
  sub-question of the full set (a value stored into a container or captured by a
  closure escapes its activation but is **not** returned), read by the reassign
  1-slot-container gate (below).

## The four facets

Escape is a value-flow over **atoms** — the only two things that carry
escape-authority are a binding reference and a lambda node. Everything else an
expression evaluates to is either an immediate (no escape to track) or a freshly
minted region the solver names by allocation site (a `Call` result, an
aggregate), which is neither a binding nor a lambda and so propagates no escape
backward.

A value escapes its activation through any of four facets:

- **return** — its value reaches a function's tail/return position. This also
  covers a fiber's *terminal* value: a fiber body is a lambda whose tail is
  seeded, and that value crosses to the joiner.
- **store** — its value is stored into a longer-lived region, exactly the region
  solver's `cross_region_refs` edge `src=value-region → dst`. Two store sources,
  both mirroring `regions/walk/walkrest.rs`: the allocating **intrinsics**
  (`(%pair v …)` every arg, `(%array-push coll v)` arg 1, `(%put obj k v)` arg 2 —
  the value embeds in the fresh aggregate), and **native calls that declare a
  store** via their `RegionEffect` (read from the `CallClassification` the lowerer
  supplies): `Stores{args}`/`Sends{args}` escape those args, `Mixed`/`Unknown`
  escapes every arg (the solver's full mutual clique), and `Fresh`/`Immediate`/
  `PassThrough`/`Funnel` escape nothing. (`Sends` is a `Stores` that also crosses a
  fiber boundary; the two escape identically here.)
- **capture** — a value *captured by a closure that itself escapes* escapes too,
  transitively. The capture facet has **no seed of its own**: a closure escapes
  its definition ONLY when its value returns/stores/crosses a fiber boundary (the
  frontier facets above). Each escaping closure then propagates escape to every
  binding it captures, transitively. A closure that is captured but never crosses a
  frontier is called in place and escapes nothing, so the lexical proxy
  `is_captured` seeds escape nowhere (precision-point-3, below).
- **fiber boundary** — a value handed across a fiber boundary escapes:
  - *yield/emit* — an `Emit` node's value is delivered to the resumer.
  - *terminal value* — the return facet (above).
  - *send* — the store facet: `chan/send` declares `Sends{[1]}` (a `Stores` that
    also crosses the fiber frontier), so its message escapes.
  - *spawn* — `fiber/new` is `Fresh`: the spawned closure rides the fresh fiber
    result and escapes only if that result does (ordinary result-flow), so it
    needs no separate rule. (`ev/spawn` is a stdlib fn, accounted in its own
    compilation.)

All four facets **seed** atoms, then a single backward propagation to a fixpoint
follows two edge kinds: **binding-definition edges** (an escaping binding pulls in
the atoms its initializer flows from — aliases) and **capture edges** (an escaping
lambda pulls in every binding it captures). So an alias of an escaping value, and
a value captured by an escaping closure, escape too. The return-only set
(`binding_escapes_via_return`) is the same propagation seeded with the return
facet alone and following binding-definition edges but **not** capture edges (a
captured value is not itself *returned*).

## Interprocedural return transparency

The return facet is **interprocedural** for an *arg-returning* callee. A tail call
`(id y)` to a function that returns its parameter is region-transparent in that
argument: the call yields whatever flowed into the arg, so `y` escapes when the
call's result does. This mirrors the region solver's `try_inline_call`
(`regions/walk.rs`), which re-walks an inlinable callee's body with its params
bound to the caller's arg regions.

It is realized as an **arg-return summary** (`compute_arg_return`): per inlinable
lambda binding, which fixed-param indices flow to its tail, computed to a fixpoint
(so `(fn (z) (id z))` chains through `id`'s summary). "Inlinable" mirrors the
solver's `binding_lambda` exactly — an immutable, unmutated `Let`/`Letrec`-bound
lambda, **never** a top-level `Define` (the solver never inlines those, so a
`def`-bound callee stays opaque and an arg returned through it does not escape via
the call). The whole-program fixpoint can in principle propagate through a deeper
arg-return chain than the solver's inline-depth-4 re-walk — a sound
over-approximation (mark a true escape the solver misses), not observed in the
corpus; bound the propagation depth if a real-corpus golden ever surfaces it.

## Consumers

Every consumer reads `EscapeInfo`; nothing keeps a parallel escape judgment.

- **The region solver** projects the verdict onto regions in `regions::escape`
  (`return_frontier_regions` / `shared_seed_regions`), through its own
  `alloc_region` / `binding_source_regions` maps — escape never sees a region.
  Four consumers read the projection or the atom facets directly:
  - the ownership **Shared seed** (`compute_shared_seeds` = the return ∪ fiber
    frontier — the regions a value crosses the activation/fiber boundary through,
    which cannot be Owned);
  - the builder-idiom **merge** gate's not-returned check (`returned_regions` = the
    return frontier, together with the region capture-graph
    (`regions::escape::captured_bindings`) for gate 5's sole-held reachability refusal
    — [region/merging.md](region/merging.md) § Merging; storing the child into the parent
    is the *allowed* escape, so the seed reads the return facet, not the full set). The
    capture refusal is a *reachability* question the region forest answers from its own
    capture-graph, never the lexical proxy `is_captured` the solver is locked out of;
  - branch **compensation**'s escaping-exclusion (the return frontier — those
    regions are the caller's to free, so compensating them would double-free);
  - the reassign 1-slot-container gate's *not-returned* check
    (`binding_escapes_via_return`, per binding —
    [region/bindings.md](region/bindings.md); read per binding, never by projecting
    a returned region onto a cell, since `binding_source_regions` is "where the
    value points," not "where it lives").
- **The lowerer** (`lir/lower`) reads `lambda_escapes_definition` /
  `binding_escapes_activation` in `control/call.rs::tail_callee_adopts`, the
  escape half of the per-call adopt decision (a per-call callee closure that dies
  at the call → the runtime supplies the stranded decref). Region-locality — does
  the callee have a per-call region demising here, vs a program-root/primitive —
  stays a region fact: `EscapeInfo` cannot express it, and only a per-call region
  may be adopted.

Two lowerer/HIR decisions deliberately **do not** read this analysis, because the
question they answer is *ownership-location / mutation-sharing*, which is
structural lexical capture, not true-escape:

- `tail_arg_is_borrowed` (`lir/lower/control.rs`): a tail-arg is borrowed iff its
  binding is a captured upvalue (the env owns the capture-incref). This is an
  ownership-location question, and escape over-approximates it — a born-here value
  that flows to a tail *escapes* but is *owned* — so escape is the wrong input. It
  reads `upvalue_bindings` (structural capture).
- cell insertion in `functionalize` / the lowerer's closure-env layout: a captured
  *mutable* binding needs a shared cell so its mutations cross the closure
  boundary, independent of whether the capturing closure escapes — a
  mutation-sharing question, not escape. It reads `needs_capture` — for a local,
  `is_captured ∧ (¬immutable ∨ is_prebound)`: a captured *mutable* local, **or** a *prebound*
  immutable one that a **sibling** closure captures (mutual recursion / a forward reference —
  the carve-out the closure-cycle merge relies on); for a param, `is_mutated`. A binding
  captured *only* by its own self-edge is **not** `is_captured` (below), so it stays cell-free
  even though it is prebound.

A same-binding self-reference — a binding's own initializer lambda referencing that
binding across the lambda boundary, in the enclosing `letrec` SCC — is recorded by the
analyzer as a first-class **`CaptureKind::Recursive`** (carrying the SCC-binding identity),
distinct from a sibling/foreign capture's `Local`/`Capture`. Unlike a sibling capture, a
self-edge does **not** mark the binding captured (`hir/arena.rs::mark_captured` is skipped
for it), so a binding captured *only* by itself has `needs_capture() == false` — no cell —
and its self-reference resolves to the currently-executing closure (`LoadSelf` in value
position, a self-call re-dispatch in call position), never a cell load, making a
self-recursive local `loop` RC-identical to a top-level recursive `defn`. It carries **no**
escape authority either (the self-edge is inert in the escape fixpoint: a self-recursive
binding's escape rides its binding-definition edge to its own lambda, so the self-capture
edge only ever self-loops, contributing nothing — `analyze_escape`/`flow.rs` build
`lambda_captures` by binding, never by kind). Its purpose is to let the lowerer resolve the
self-reference to the executing closure from the classified fact instead of re-deriving the
self-edge from a `current_function_binding` heuristic. A mutual member's *sibling* capture
(`ev` capturing `od`) stays `Local`/`Capture` and DOES mark captured — so a member a sibling
captures keeps its forward cell (which the closure-cycle merge collapses); only the self-edge
is `Recursive`.

These are the *structural-only* role of lexical capture, and the **only** roles
left to `is_captured`: it feeds NO escape facet (the capture facet is flow-true —
transitive `lambda_captures` propagation from genuine frontier seeds), and it is
module-private with no getter, so no consumer can read it as escape-authority — the
escape-authority defect is closed by construction, not by promise. The region pins
under `tests/elle/region-*.lisp` are the canonical reference for what each of these
decisions must preserve.

## Precision characteristics

Escape is finer than the structural proxies it replaces. These are the
characterized points where its verdict is the *precise* one; each is pinned by a
unit test asserting escape's own spec.

1. **Stored borrowed param.** A stored borrowed param — `(fn (x) (%pair x x))`, or a
   `chan/send` whose message is a param — genuinely escapes (it embeds in a
   longer-lived aggregate / crosses a fiber), so the store facet marks it. A param's
   region is a runtime placeholder the region projection treats as not-ownable, so
   marking it is sound and costs the consumers nothing.
2. **Store target vs stored value.** `(%put obj k v)` / `(assign acc v)` escapes the
   stored *value*, never the *container* `obj`/`acc` (writing into a container is not
   the container escaping). Escape marks only the value; the projection acts per
   region, so the value's region is marked exactly once.
3. **Lexical capture is not escape.** A value captured by a closure that is *called
   in place* (never escapes) does not escape via capture — the capture facet marks it
   only when its capturing closure escapes, where `is_captured` marks every captured
   binding unconditionally. (A value the closure *returns* still return-escapes
   through the closure's own tail — a separate facet.)
4. **Fiber boundary.** An emitted/sent value crosses to the resumer/receiver, so the
   fiber facet marks it (and `fiber_frontier_sites` catches an atomless
   `(yield (%pair …))`). There is no compile-time RC edge at an `Emit` (the runtime
   incref in `handle_emit` keeps it alive) — the fiber crossing is purely escape's.
5. **Native `Mixed`/`Unknown` clique is conservative.** A native declared `Mixed`
   (uncounted store, examined) or `Unknown` (unexamined — the default) marks every
   heap argument escaping. As imprecise as the declarations are honest; examining a
   primitive and declaring a tighter `RegionEffect` narrows it.

## Verification

Escape is the authority, so it is pinned by its **own** spec, not by agreement with
another analysis. Three layers:

- **The unit tests** (`src/hir/escape/tests/`) assert each facet's discriminating
  behaviour directly: a returned value return-escapes; a stored value escapes its
  activation but is not returned; a value captured by a non-escaping closure does
  not escape; an emitted/sent value crosses the fiber frontier; an arg-returning
  callee propagates its arg's escape; a `def`-bound callee does not.
- **The escape golden** ([`tests/elle/escape-golden.lisp`](../../tests/elle/escape-golden.lisp))
  pins the normalized `escape` dump kind ([`src/dump/escape.rs`](../../src/dump/escape.rs),
  reached from Elle via `compile/dumps` → `:escape`, and `--dump=escape` on the
  CLI) of a bounded set of real corpus files, byte-for-byte. The dump is
  id-normalized so two compiles render identically; its `[return_frontier]` section
  records escape's verdict projected to regions and its `[region_instrs]` section the
  RC instructions that verdict drives. A change to escape or its consumers shows up
  as a snapshot diff to review (the emitted RC may *tighten* as escape's precision
  lands; it must never coarsen or introduce a UAF/leak). The corpus is bounded
  because `compile/dumps` compiles each source twice and leaks regions (it OOMs a
  full make-smoke run — [test-runner.md](../test-runner.md) § CAS asset capture).
- **The region suite + `oracle.lisp`** prove the projection reclaims soundly: no UAF
  (`--trace=guardfree` under the full stdlib) and no leak regression (every closed
  leak class stays closed).

## Relationship to the region forest

`EscapeInfo` is the durable artifact: it becomes the forest's Owned-vs-Shared
classifier — the hierarchical single-owner region endpoint the region work builds
toward. The lowerer's value-RC predicates `tail_arg_is_borrowed` and
`tail_callee_adopts` (mint/adopt compensation) are **transitional** — the forest
reclaims an intra-fiber Owned subtree by drop, including any reference cycle
interior to it (no mint, no adopt), and edge-RC scopes reference counting to
cross-fiber Shared edges, so both predicates are subsumed, not preserved. Lexical
capture (`is_captured`/`needs_capture`) persists only as the structural hint for
cell layout, with no escape-authority. The capture *kind* has no ownership
authority either: the forest's capture-adopt emit reloads an adopted captured
value through whichever access path the kind implies (a binding slot for a direct
local, the constructing function's environment for an upvalue or transitive
capture — region/adopt.md § "The capture adopt"), so admission of a capture
owner-edge is bounded by the subtree admission filters (decisively, the lifetime
obligation), never by how the capture happens to be loaded.
