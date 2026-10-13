# Escape analysis — the authoritative true-escape pass

<!-- audited: 2026-10-13 -->

Escape analysis decides, once for every consumer, whether a value outlives the activation it was born in.

Escape is **one** analysis, computed over the canonical (functionalized + ANF)
HIR, and it is authoritative: every consumer that needs the answer reads it,
rather than recomputing a proxy. It lives in [src/hir/escape.rs](../../src/hir/escape.rs) as `analyze_escape`,
producing an `EscapeInfo` fact-set.

This is the keystone of the region forest. The forest classifies regions as
Owned (reclaimed by subtree drop) or Shared (reference-counted), and that
property *is* true escape. So computing escape once, over the regularized IR,
builds the forest's substrate at the front edge.

The alternative is a **syntactic proxy**, `is_captured`: "a free variable
crosses a lexical function boundary", computed early as a side-effect of name
resolution. It diverges from true escape exactly at the interesting cases: a
closure that lexically captures but never escapes, and a value shared across
fibers. It is also baked before the regularizing passes that would change it.
So lexical capture is demoted to a **structural hint with no escape-authority**
(its surviving roles are below).

## The two questions

`EscapeInfo` answers two questions; consumers query through methods, never the
private sets, so the representation can change without touching call sites.

- **per binding** — does the binding's value *escape its defining activation*?
  `binding_escapes_activation(b)`. Absence = `false`.
- **per lambda** — does the closure *escape its definition*?
  `lambda_escapes_definition(id)`. Absence = `false`.

Two narrower queries split the full set by whether the **return** facet is the
reason for the verdict:

- **per binding, return facet only** — does the value escape *specifically by
  flowing to a tail/return*? `binding_escapes_via_return(b)`. A strict
  sub-question of the full set: a value stored into a container or captured by a
  closure escapes its activation but is **not** returned. The module-scope half
  of the reassign 1-slot-container gate reads it (below).
- **per binding, every facet but return** — does the value escape by a route
  *other* than flowing to a tail/return? `binding_escapes_beyond_return(b)`. The
  complement: the two together let a consumer say "this facet and no other". The
  full set alone cannot, because a binding on both the return and the fiber
  frontiers answers `true` to both queries above. The frame-held admission reads
  the narrower containment cut below instead.

## The four facets

Escape is a value-flow over **atoms** — the only two things that carry
escape-authority are a binding reference and a lambda node. Everything else an
expression evaluates to is an immediate (no escape to track) or a freshly minted
region the solver names by allocation site (a `Call` result, an aggregate). Such
a region is neither a binding nor a lambda, so it propagates no escape backward.

A value escapes its activation through any of four facets:

- **return** — its value reaches a function's tail/return position. This also
  covers a fiber's *terminal* value: a fiber body is a lambda whose tail is
  seeded, and that value crosses to the joiner.
- **store** — its value is stored into a longer-lived region, exactly the region
  solver's `cross_region_refs` edge `src=value-region → dst`. There are two store
  sources, and both mirror the solver's walk
  ([walkrest.rs](../../src/hir/region/infer/walk/walkrest.rs)):
  - the allocating **intrinsics**, where the value embeds in the fresh
    aggregate: `(%pair v …)` every arg, `(%array-push coll v)` arg 1,
    `(%put obj k v)` arg 2;
  - **native calls that declare a store** through their `RegionEffect`, read
    from the `CallClassification` the lowerer supplies. `Stores{args}`,
    `Sends{args}` and `Delivers{args}` escape those args. `Mixed`/`Unknown`
    escapes every arg (the solver's full mutual clique).
    `Fresh`/`Immediate`/`PassThrough`/`Funnel`/`Opaque` escape nothing.

  `Sends` and `Delivers` are stores that also cross a fiber boundary. All three
  escape identically here, and differ in what the SOLVER records: `Stores` an
  edge, `Sends` and `Delivers` none, because the send seam and the install seam
  each count their own reference at runtime.
- **capture** — a value *captured by a closure that itself escapes* escapes too,
  transitively. The capture facet has **no seed of its own**: a closure escapes
  its definition ONLY when its value returns/stores/crosses a fiber boundary (the
  frontier facets above). Each escaping closure then propagates escape to every
  binding it captures, transitively. A closure that is captured but never crosses a
  frontier is called in place and escapes nothing, so the lexical proxy
  `is_captured` seeds escape nowhere (precision point 4, below).
- **fiber boundary** — a value handed across a fiber boundary escapes:
  - *yield/emit* — an `Emit` node's value is delivered to the resumer.
  - *terminal value* — the return facet (above).
  - *send* — the store facet: `chan/send` declares `Sends{[1]}` (a `Stores` that
    also crosses the fiber frontier), so its message escapes.
  - *install* — the fiber value installers (`fiber/resume`, `fiber/abort`,
    `fiber/cancel`, `fiber/emit`) declare `Delivers{[1]}`: the delivered value goes
    into another fiber's signal slot, so it escapes on this facet too.
  - *spawn* — `fiber/new` is `Fresh`: the spawned closure rides the fresh fiber
    result and escapes only if that result does (ordinary result-flow), so it
    needs no separate rule. (`ev/spawn` is a stdlib fn, accounted in its own
    compilation.)

All four facets **seed** atoms. Then a single backward propagation to a fixpoint
follows two edge kinds. A **binding-definition edge** pulls in the atoms an
escaping binding's initializer flows from, its aliases. A **capture edge** pulls
in every binding an escaping lambda captures. So an alias of an escaping value,
and a value captured by an escaping closure, escape too. The return-only set
(`binding_escapes_via_return`) is the same propagation seeded with the return
facet alone and following binding-definition edges but **not** capture edges (a
captured value is not itself *returned*). Its complement
(`binding_escapes_beyond_return`) is the same propagation seeded with every facet
*except* return, following both edge kinds — so a closure that leaves only by
being returned propagates nothing there.

One more cut of the same propagation: **containment alone**
(`binding_escapes_by_containment`) is beyond-return less its fiber seeds — the
store facet and capture by a closure that itself escapes. It exists because a
consumer may care *who counts the second holder* rather than *whether one
exists*. A containment escape hands the value to a holder the frame cannot see,
while every fiber crossing counts a reference at its seam. The frame-held
admission is that consumer ([region/window.md](region/window.md)).

## Interprocedural return transparency

The return facet is **interprocedural** for an *arg-returning* callee. A tail call
`(id y)` to a function that returns its parameter is region-transparent in that
argument: the call yields whatever flowed into the arg, so `y` escapes when the
call's result does. This mirrors the region solver's `try_inline_call`
([walk/inline.rs](../../src/hir/region/infer/walk/inline.rs)), which re-walks an inlinable callee's body with its params
bound to the caller's arg regions.

It is realized as an **arg-return summary** (`compute_arg_return`): per inlinable
lambda binding, which fixed-param indices flow to its tail, computed to a fixpoint
(so `(fn (z) (id z))` chains through `id`'s summary). "Inlinable" mirrors the
solver's `binding_lambda` exactly: an immutable, unmutated `Let`/`Letrec`-bound
lambda, **never** a top-level `Define`. The solver never inlines a `Define`, so a
`def`-bound callee stays opaque, and an arg returned through it does not escape
via the call. The whole-program fixpoint can propagate through a deeper
arg-return chain than the solver's inline-depth-4 re-walk. That is a sound
over-approximation, marking a true escape the solver misses, and the suites have
not shown it. If the escape golden ever shows it, bound the propagation depth.

## Consumers

Every consumer reads `EscapeInfo`; nothing keeps a parallel escape judgment.

- **The region solver** projects the verdict onto regions in `region::infer::escape`
  (`return_frontier_regions` / `shared_seed_regions`), through its own
  `alloc_region` / `binding_source_regions` maps — escape never sees a region.
  Five consumers read the projection or the atom facets directly:
  - the ownership **Shared seed** (`compute_shared_seeds` = the return ∪ fiber
    frontier — the regions a value crosses the activation/fiber boundary through,
    which cannot be Owned);
  - the builder-idiom **merge** gate's not-returned check. It reads
    `returned_regions`, the return frontier, and the region capture-graph
    (`region::infer::escape::captured_bindings`) for gate 5's sole-held
    reachability refusal ([region/merging.md](region/merging.md)). Storing the
    child into the parent is the *allowed* escape, so the check reads the return
    facet, not the full set. The capture refusal is a *reachability* question the
    region forest answers from its own capture-graph. It never reads the lexical
    proxy `is_captured`, which the solver is locked out of;
  - branch **compensation**'s escaping-exclusion (the return frontier — those
    regions are the caller's to free, so compensating them would double-free).
    The exclusion is **per-path**. Escape marks the whole region returnable as
    soon as one path returns it. On a sibling arm that never uses the value no
    mint fires and the caller receives nothing, so that arm still owes the
    release ([region/compensate.md](region/compensate.md));
  - the reassign 1-slot-container gate's *not-returned* check
    (`binding_escapes_via_return`, [region/bindings.md](region/bindings.md)). It
    reads per binding, never by projecting a returned region onto a cell, since
    `binding_source_regions` is "where the value points," not "where it lives."
    Only the **module-scope** half asks it: that cell adopts the producer's
    reference, which the return also transfers. A fn-local cell takes a counted
    reference of its own and so claims nothing the return needs;
  - the **frame-held admission** (`region::infer::escape::frame_held_regions`),
    which the branch-arm release window and the lowerer's frame-exit release
    share. Both make a release fire on a path where none fired before, so both
    must know this frame holds the region's one reference. What it needs is
    narrower than "escapes": an **uncounted** second holder. So it reads
    `binding_escapes_by_containment` per holder (the store facet, and capture by
    a closure that itself escapes) plus the fiber frontier's atomless site half.
    It admits the two facets whose holder is counted at the crossing.

    Unlike the merge gate above, it does **not** consult the structural
    capture-graph. A closure's hold on what it captures is a counted (or owning)
    edge rather than an uncounted borrow, and capture by a closure escaping
    *beyond the return facet* is already an escape facet
    ([region/window.md](region/window.md)). The **fiber** facet rides along for
    the same reason. The park's `EmitEscape` retain going out, the resume value's
    own mint coming back, and `chan/send`'s send-site incref each count a
    reference before this frame runs on ([region/window.md](region/window.md)).

    The **return** facet rides along rather than refusing. The caller does read
    such a region afterwards, through the tail callee's return mint. But that
    callee reaches a value this frame owns only as an operand or through its
    captured environment. So it either counts the region or cannot mint against
    it ([region/relocate.md](region/relocate.md)).

    The admission also refuses a **mutated** route, which is the release-route
    fact compensation makes rather than an escape fact. So it asks that of the
    one binding whose slot the release loads: the binding whose init allocated
    the region, or the parameter the prologue recorded. It never asks it of every
    holder, and an env cell's `DecrefCellRegion`, which names the box no `assign`
    repoints, is exempt ([region/window.md](region/window.md));
- **The lowerer** ([src/lir/lower/](../../src/lir/lower/AGENTS.md)) reads
  `lambda_escapes_definition` / `binding_escapes_activation` in
  `tail_callee_defers_release` ([defer.rs](../../src/lir/lower/control/call/defer.rs)).
  That is the escape half of the per-call adopt decision: when a per-call callee
  closure dies at the call, the runtime supplies the stranded decref.
  Region-locality — does the callee have a per-call region demising here, vs a
  program-root/primitive — stays a region fact: `EscapeInfo` cannot express it,
  and only a per-call region may be adopted.

  A **stranded recursive** callee takes a narrower question in the same predicate:
  `escapes_fiber` alone. Its region has no other release channel at all, so the
  full activation escape would re-strand it. The store and capture facets are
  containment relations, and a closure a local container holds dies *with* the
  activation. The return facet is admitted too, for a reason of its own rather
  than the frame-exit release's. This release runs at the recursion's
  completion, *after* the callee's return mint, so there is no gap to span
  ([selfrec.md](selfrec.md)). Only the fiber facet hands the closure to a holder
  the compiler did not place, so only it refuses.

Two lowerer/HIR decisions deliberately **do not** read this analysis, because the
question they answer is *ownership-location / mutation-sharing*, which is
structural lexical capture, not true-escape:

- `tail_arg_is_borrowed` ([control.rs](../../src/lir/lower/control.rs)): a tail-arg is
  borrowed when the frame holds no reference of its own to move, by the routes
  that [region/rules.md](region/rules.md) lists under Rule 5. This is an
  ownership-location question, and escape over-approximates it. A born-here value
  that flows to a tail *escapes* but is *owned*, so escape is the wrong input. The
  predicate reads structural facts: capture, pattern position, and the binder's
  counted reads.
- cell insertion in `functionalize` / the lowerer's closure-env layout: a captured
  *mutable* binding needs a shared cell so its mutations cross the closure
  boundary, whether or not the capturing closure escapes. That is a
  mutation-sharing question, not escape. It reads `needs_capture`. For a local
  that is `is_captured ∧ (¬immutable ∨ is_prebound)`: a captured *mutable* local,
  **or** a *prebound* immutable one that a **sibling** closure captures. The
  second is mutual recursion or a forward reference, the carve-out the
  closure-cycle merge relies on. For a param it is `is_mutated`. A binding
  captured *only* by its own self-edge is **not** `is_captured` (below), so it
  stays cell-free even though it is prebound.

A same-binding self-reference is a binding's own initializer lambda referencing
that binding across the lambda boundary, in the enclosing `letrec` SCC. The
analyzer records it as a first-class **`CaptureKind::Recursive`**, carrying the
SCC-binding identity, distinct from a sibling/foreign capture's
`Local`/`Capture`. Unlike a sibling capture, a self-edge does **not** mark the
binding captured: `mark_captured` in [arena.rs](../../src/hir/arena.rs) skips
it. So a binding captured *only* by itself has `needs_capture() == false` and no
cell. Its self-reference resolves to the currently-executing closure, never a
cell load: `LoadSelf` in value position, a self-call re-dispatch in call
position. That makes a self-recursive local `loop` RC-identical to a top-level
recursive `defn`.

It carries **no** escape authority either. The self-edge is inert in the escape
fixpoint: a self-recursive binding's escape rides its binding-definition edge to
its own lambda, so the self-capture edge only self-loops and contributes
nothing. `analyze_escape` and [flow.rs](../../src/hir/escape/flow.rs) build
`lambda_captures` by binding, never by kind. Its purpose is to let the lowerer
resolve the self-reference to the executing closure from the classified fact
instead of re-deriving the self-edge from a `current_function_binding`
heuristic. A mutual member's *sibling* capture (`ev` capturing `od`) stays
`Local`/`Capture` and DOES mark captured. So a member a sibling captures keeps
its forward cell, which the closure-cycle merge collapses; only the self-edge is
`Recursive`.

These are the *structural-only* roles of lexical capture, and the **only** roles
left to `is_captured`. It feeds NO escape facet: the capture facet is flow-true,
a transitive `lambda_captures` propagation from genuine frontier seeds. It is
module-private with no getter, so no consumer can read it as escape-authority.
The gap is closed by construction, not by promise. The `region-*.lisp` pins in
[tests/impl](../../tests/impl/overview.md) are the canonical reference for what
each of these decisions must preserve.

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
3. **Container-read escape.** A value stored into a container, then read back OUT
   (`first`/`rest`/`get`/`pop`) and ESCAPED, must be marked escaping too. The
   ownership forest records `content ⊇ container` at the store (a `%array-push`
   funnel). Unmarked, the forest would adopt the content into the container's
   Owned subtree and free it at the container's scope-exit subtree drop, under the
   escaped read reference
   ([region-container-read-escape-uaf.lisp](../../tests/impl/region-container-read-escape-uaf.lisp)).
   A per-container **stored-contents map**, built by `collect_container_contents`
   from the `push`/`put`/`add` store sites, feeds a **read-result →
   container-contents flow edge** at each element read (`collect_flow`). So an
   escaping read result pulls the container's stored contents into its own facet
   through the ordinary fixpoint. It is PRECISE, not "every read escapes": the
   contents are marked ONLY when the read result itself reaches a facet, so a
   container merely read/indexed with the result consumed locally keeps its Owned
   reclamation.
4. **Lexical capture is not escape.** A value captured by a closure that is *called
   in place* (never escapes) does not escape via capture. The capture facet marks
   it only when its capturing closure escapes, where `is_captured` marks every
   captured binding unconditionally. (A value the closure *returns* still
   return-escapes through the closure's own tail — a separate facet.)
5. **Fiber boundary.** An emitted/sent value crosses to the resumer/receiver, so the
   fiber facet marks it (and `fiber_frontier_sites` catches an atomless
   `(yield (%pair …))`). There is no compile-time RC edge at an `Emit` (the runtime
   incref in `handle_emit` keeps it alive) — the fiber crossing is purely escape's.
6. **Native `Mixed`/`Unknown` clique is conservative.** A native declared `Mixed`
   (uncounted store, examined) or `Unknown` (unexamined — the default) marks every
   heap argument escaping. As imprecise as the declarations are honest; examining a
   primitive and declaring a tighter `RegionEffect` narrows it. The seed is the
   whole cost for a single-argument native, whose clique is empty in any case. So
   this analysis reads a `Mixed` declaration as a store claim and nothing else,
   and a native that stores nothing declares `Opaque`
   ([region/effects.md](region/effects.md)).

## Verification

Escape is the authority, so it is pinned by its **own** spec, not by agreement with
another analysis. Three layers:

- **The unit tests** ([src/hir/escape/tests/](../../src/hir/escape/tests/mod.rs))
  assert each facet's discriminating behavior directly:
  - a returned value return-escapes;
  - a stored value escapes its activation but is not returned;
  - a value captured by a non-escaping closure does not escape;
  - an emitted/sent value crosses the fiber frontier;
  - an arg-returning callee propagates its arg's escape, and a `def`-bound
    callee does not.
- **The escape golden** ([tests/impl/escape-golden.lisp](../../tests/impl/escape-golden.lisp))
  pins the normalized `escape` dump kind of a bounded set of real test files,
  byte-for-byte. The dump is [src/dump/escape.rs](../../src/dump/escape.rs),
  reached from Elle through `compile/dumps` → `:escape` and from the CLI through
  `--dump=escape`. The dump is id-normalized so two compiles render identically;
  its `[return_frontier]` section records escape's verdict projected to regions
  and its `[region_instrs]` section the RC instructions that verdict drives. A
  change to escape or its consumers shows up as a snapshot diff to review (the
  emitted RC may *tighten* as escape's precision lands; it must never coarsen or
  introduce a UAF/leak). The set is bounded because `compile/dumps` leaks
  regions, so a pass over every file runs out of memory
  ([test-runner.md](../test-runner.md)).
- **The `region-*.lisp` files and the probes in
  [tests/impl/probe/](../../tests/impl/probe/)** prove the projection reclaims soundly: no UAF
  (`--trace=guardfree` under the full stdlib) and no leak regression (every closed
  leak class stays closed).

## Relationship to the region forest

`EscapeInfo` is the durable artifact: it becomes the forest's Owned-vs-Shared
classifier — the hierarchical single-owner region endpoint the region work builds
toward. The lowerer's value-RC predicates `tail_arg_is_borrowed` and
`tail_callee_defers_release` (mint/adopt compensation) are **transitional**. The
forest reclaims an intra-fiber Owned subtree by drop, including any reference
cycle interior to it, with no mint and no adopt. Edge-RC then scopes reference
counting to cross-fiber Shared edges, so the forest subsumes both predicates
rather than preserving them. Lexical capture (`is_captured`/`needs_capture`)
persists only as the structural hint for cell layout, with no escape-authority.

The capture *kind* has no ownership authority either. The forest's capture-adopt
emit reloads an adopted captured value through whichever access path the kind
implies: a binding slot for a direct local, the constructing function's
environment for an upvalue or transitive capture
([region/adopt.md](region/adopt.md)). So the subtree admission filters bound the
admission of a capture owner-edge, decisively the lifetime obligation, never how
the capture happens to be loaded.
