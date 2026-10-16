# Escape analysis — the authoritative true-escape pass

<!-- audited: 2026-10-13 -->

Escape analysis decides, once for every consumer, whether a value outlives the activation it was born in.

Escape is **one** analysis, computed over the canonical (functionalized + ANF)
HIR, and it is authoritative. Every consumer that needs the answer reads it,
rather than recomputing a proxy. It lives in
[src/hir/escape.rs](../../src/hir/escape.rs) as `analyze_escape`, producing an
`EscapeInfo` fact-set.

This is the keystone of the region forest. The forest classifies regions as
Owned (reclaimed by subtree drop) or Shared (reference-counted), and that
property *is* true escape. Computing it once, over the regularized IR, builds
the forest's substrate at the front edge. The alternative is a **syntactic
proxy**: `is_captured`, "a free variable crosses a lexical function boundary",
computed early as a side-effect of name resolution. That proxy diverges from
true escape exactly at the interesting cases: a closure that lexically captures
but never escapes, and a value shared across fibers. It is also baked before the
regularizing passes that would change it. Lexical capture is therefore demoted
to a **structural hint with no escape-authority**, and its surviving roles are
below.

## The questions

`EscapeInfo` answers its questions through methods, never through the private
sets, so the representation can change without touching call sites. Absence
means `false` for each.

- **per binding**: does the binding's value *escape its defining activation*?
  `binding_escapes_activation(b)`.
- **per lambda**: does the closure *escape its definition*?
  `lambda_escapes_definition(id)`.
- **per binding, return facet only**: does the value escape *specifically by
  flowing to a tail/return*? `binding_escapes_via_return(b)`. This is a strict
  sub-question of the full set. A value stored into a container or captured by a
  closure escapes its activation but is **not** returned.
- **per binding, containment only**: does the value escape by a store or by
  capture into a closure that itself escapes?
  `binding_escapes_by_containment(b)`.
- **per binding, fiber only**: is the value emitted or sent?
  `escapes_fiber(b)`.
- **per allocation site**: does an atomless value reach a return, or cross a
  fiber boundary? `escapes_return_frontier(id)` and `escapes_fiber_frontier(id)`.

## The four facets

Escape is a value-flow over **atoms**. Only two things carry escape-authority: a
binding reference and a lambda node. Everything else an expression evaluates to
is either an immediate, with no escape to track, or a freshly minted region the
solver names by allocation site (a `Call` result, an aggregate). That region is
neither a binding nor a lambda, so it propagates no escape backward.

A value escapes its activation through any of four facets:

- **return**: its value reaches a function's tail/return position. This also
  covers a fiber's *terminal* value, since a fiber body is a lambda whose tail
  is seeded, and that value crosses to the joiner.
- **store**: its value is stored into a longer-lived region, which is the region
  solver's `cross_region_refs` edge `src=value-region → dst`. The seeds mirror
  the solver's `walk_intrinsic`
  ([intrinsic.rs](../../src/hir/region/infer/walk/intrinsic.rs)) and its
  opaque-call edges. The allocating **intrinsics** store their value operands: a
  `%pair` every arg, the push and add ops arg 1, and the put ops arg 2. A
  **native call** stores what its `RegionEffect` declares, read from the
  `CallClassification` the lowerer supplies. `Stores{args}` stores those args,
  and `Mixed`/`Unknown` stores every arg (the solver's full mutual clique). The
  other effects store nothing.
- **capture**: a value *captured by a closure that itself escapes* escapes too,
  transitively. The capture facet has **no seed of its own**. A closure escapes
  its definition ONLY when its value returns, is stored, or crosses a fiber
  boundary. Each escaping closure then propagates escape to every binding it
  captures. A closure that is captured but never crosses a frontier is called in
  place and escapes nothing, so the lexical proxy `is_captured` seeds escape
  nowhere (precision point 4, below).
- **fiber boundary**: a value handed across a fiber boundary escapes.
  - *yield/emit*: an `Emit` node's value is delivered to the resumer.
  - *terminal value*: the return facet (above).
  - *send*: `chan/send` declares `Sends{[1]}`, so its message crosses to the
    receiving fiber.
  - *install*: the fiber value installers (`fiber/resume`, `fiber/abort`,
    `fiber/cancel`, `fiber/emit`) declare `Delivers{[1]}`. The delivered value
    goes into another fiber's signal slot.
  - *spawn*: `fiber/new` is `Fresh`. The spawned closure rides the fresh fiber
    result, and escapes only if that result does, by ordinary result flow.
    (`ev/spawn` is a stdlib fn, accounted in its own compilation.)

The solver records no edge for `Sends` or `Delivers`, because the send seam and
the install seam each count their own reference at runtime. Escape still marks
the value on the fiber facet, since it crosses to another fiber.

## The propagation

All four facets **seed** atoms. Then a backward propagation to a fixpoint
follows two edge kinds. **Binding-definition edges** make an escaping binding
pull in the atoms its initializer flows from, its aliases. **Capture edges**
make an escaping lambda pull in every binding it captures. So an alias of an
escaping value, and a value captured by an escaping closure, escape too
([collect.rs](../../src/hir/escape/flow/collect.rs) gathers both edge kinds).

The narrower sets are the same propagation with different seeds and edges:

- the **return-only** set (`binding_escapes_via_return`) is seeded with the
  return facet alone. It follows binding-definition edges but **not** capture
  edges, since a captured value is not itself *returned*.
- the **containment** set (`binding_escapes_by_containment`) is seeded with the
  stores alone, and follows both edge kinds. It exists because a consumer may
  care *who counts the second holder* rather than *whether one exists*. A
  containment escape hands the value to a holder the frame cannot see, while
  every fiber crossing counts a reference at its seam. The frame-held admission
  is that consumer ([region/held.md](region/held.md)).

A `%pair` does not seed containment, for the same reason. The pair holds each
operand by the counted edge its allocation takes, as a closure holds what it
captures. So an operand is pulled into containment only when a binding that
holds the pair escapes by containment, the way a capture follows its closure.
The other sets still read the pair's operands as stored. A recursion that
threads `(%pair i acc)` into its own tail call therefore keeps `acc` frame-held,
and the frame releases it before the call ([region/relocate.md](region/relocate.md)).

## Interprocedural return transparency

The return facet is **interprocedural** for an *arg-returning* callee. A tail
call `(id y)` to a function that returns its parameter is region-transparent in
that argument. The call yields whatever flowed into the arg, so `y` escapes when
the call's result does. This mirrors the region solver's `try_inline_call`
([inline.rs](../../src/hir/region/infer/walk/inline.rs)), which re-walks an
inlinable callee's body with its params bound to the caller's arg regions.

It is realized as an **arg-return summary** (`compute_arg_return` in
[summary.rs](../../src/hir/escape/flow/summary.rs)). For each inlinable lambda
binding, it records which fixed-param indices flow to its tail, computed to a
fixpoint, so `(fn (z) (id z))` chains through `id`'s summary. "Inlinable"
mirrors the solver's `binding_lambda` exactly: an immutable, unmutated
`Let`/`Letrec`-bound lambda, **never** a top-level `Define`. The solver never
inlines a `Define`, so a `def`-bound callee stays opaque, and an arg returned
through it does not escape via the call.

The whole-program fixpoint can propagate through a deeper arg-return chain than
the solver's inline re-walk, which stops at depth 4. That is a sound
over-approximation: it marks a true escape the solver misses.

## Consumers

Every consumer reads `EscapeInfo`; nothing keeps a parallel escape judgment.

**The region solver** projects the verdict onto regions in
`region::infer::escape`
([region/infer/escape.rs](../../src/hir/region/infer/escape.rs)), through its
own `alloc_region` and `binding_source_regions` maps. Escape never sees a
region. These consumers read the projection or the atom facets directly:

- the ownership **Shared seed** (`compute_shared_seeds`, the return ∪ fiber
  frontier). Those are the regions a value crosses the activation or fiber
  boundary through, which cannot be Owned.
- the builder-idiom **merge** gate's not-returned check. It reads the return
  frontier and `binding_escapes_via_return` per holder, together with the
  region capture graph (`region::infer::escape::captured_bindings`) for its
  sole-held reachability refusal ([region/merging.md](region/merging.md)).
  Storing the child into the parent is the *allowed* escape, so the seed reads
  the return facet, not the full set. The capture refusal is a *reachability*
  question the region forest answers from its own capture graph, never from the
  lexical proxy `is_captured` the solver is locked out of.
- the **closure-cycle merge**'s frontier gate. It reads the fiber frontier to
  refuse, and the return frontier to demand a funding order
  ([region/letrec.md](region/letrec.md)).
- branch **compensation**'s escaping-exclusion, the return frontier. Those
  regions are the caller's to free, so compensating them would double-free. The
  exclusion is **per-path**. Escape marks the whole region returnable as soon as
  one path returns it. On a sibling arm that never uses the value, no mint fires
  and the caller receives nothing, so that arm still owes the release
  ([region/compensate.md](region/compensate.md)).
- the reassign 1-slot-container gate's *not-returned* check
  (`binding_escapes_via_return`, per binding;
  [region/bindings.md](region/bindings.md)). It is read per binding, never by
  projecting a returned region onto a cell, since `binding_source_regions` is
  "where the value points", not "where it lives". Only the **module-scope** half
  asks it. That cell adopts the producer's reference, which the return also
  transfers. A fn-local cell takes a counted reference of its own, and so claims
  nothing the return needs.
- the **frame-held admission** (`region::infer::escape::frame_held_regions`),
  which the branch-arm release window and the lowerer's frame-exit release
  share. It reads `binding_escapes_by_containment` per holder, plus the fiber
  frontier's atomless site half. The return facet and a fiber crossing each
  leave a counted holder, so neither refuses
  ([region/held.md](region/held.md)).

**The lowerer** ([src/lir/lower/](../../src/lir/lower/AGENTS.md)) reads
`lambda_escapes_definition` and `binding_escapes_activation` in
`tail_callee_defers_release`
([defer.rs](../../src/lir/lower/control/call/defer.rs)). That is the escape half
of the per-call adopt decision: a per-call callee closure that dies at the call
lets the runtime supply the stranded decref. Region-locality stays a region
fact, whether the callee has a per-call region demising here or is a
program-root or primitive. `EscapeInfo` cannot express it, and only a per-call
region may be adopted.

The same predicate reads narrower questions for its binding-keyed channels. A
stranded **self-recursive** callee asks escape nothing, since the deferral
supplies exactly the frame's own reference ([selfrec.md](selfrec.md)). A
stranded **cycle member** or **letrec member** asks `escapes_fiber` alone. Its
region has no other release channel at all, so the full activation escape would
re-strand it. The store and capture facets are containment relations, and a
closure a local container holds dies *with* the activation. The return facet is
admitted too, for a reason of its own: this release runs at the recursion's
completion, *after* the callee's return mint, so there is no gap to span. Only
the fiber facet hands the closure to a holder the compiler did not place, so
only it refuses.

Two lowerer/HIR decisions deliberately **do not** read this analysis. The
question they answer is *ownership-location* or *mutation-sharing*, which is
structural lexical capture, not true escape:

- `tail_arg_is_borrowed` ([control.rs](../../src/lir/lower/control.rs)): a
  tail-arg is borrowed when the frame holds no reference of its own to move, by
  the routes that [region/rules.md](region/rules.md) lists under Rule 5. This is
  an ownership-location question, and escape over-approximates it. A born-here
  value that flows to a tail *escapes*, but is *owned*, so escape is the wrong
  input. The predicate reads structural facts: capture, pattern position, and
  the binder's counted reads.
- cell insertion in `functionalize` and the lowerer's closure-env layout: a
  captured *mutable* binding needs a shared cell, so its mutations cross the
  closure boundary whether or not the capturing closure escapes. That is a
  mutation-sharing question, not escape. It reads `needs_capture`. For a local,
  that is `is_captured ∧ (¬immutable ∨ is_prebound)`: a captured *mutable*
  local, **or** a *prebound* immutable one that a **sibling** closure captures
  (mutual recursion, a forward reference). For a param, it is `is_mutated`. A
  binding captured *only* by its own self-edge is **not** `is_captured` (below),
  so it stays cell-free even though it is prebound.

A same-binding self-reference is a binding's own initializer lambda referencing
that binding across the lambda boundary, in the enclosing `letrec` SCC. The
analyzer records it as a first-class **`CaptureKind::Recursive`**, carrying the
SCC-binding identity, distinct from a sibling or foreign capture's
`Local`/`Capture`. Unlike a sibling capture, a self-edge does **not** mark the
binding captured: `mark_captured` in [arena.rs](../../src/hir/arena.rs) is
skipped for it. So a binding captured *only* by itself has
`needs_capture() == false`, and no cell. Its self-reference resolves to the
currently-executing closure: `LoadSelf` in value position, a self-call
re-dispatch in call position. That makes a self-recursive local `loop`
RC-identical to a top-level recursive `defn` ([selfrec.md](selfrec.md)).

The self-edge carries **no** escape authority either. A self-recursive binding's
escape rides its binding-definition edge to its own lambda, so the self-capture
edge only ever self-loops, contributing nothing. `collect_flow` builds
`lambda_captures` by binding, never by kind. A mutual member's *sibling* capture
(`ev` capturing `od`) stays `Local`/`Capture` and DOES mark captured. So a member
a sibling captures keeps its forward cell, which the closure-cycle merge
collapses. Only the self-edge is `Recursive`.

These are the *structural-only* roles of lexical capture, and the **only** roles
left to `is_captured`. It feeds NO escape facet, since the capture facet is
flow-true: transitive `lambda_captures` propagation from real frontier seeds. It
is module-private with no getter, so no consumer can read it as
escape-authority. The escape-authority gap is closed by construction. The
`region-*.lisp` pins in [tests/impl](../../tests/impl/overview.md) are the canonical
reference for what each of these decisions must preserve.

## Precision characteristics

Escape is finer than the structural proxies it replaces. These are the
characterized points where its verdict is the *precise* one; each is pinned by a
unit test asserting escape's own spec.

1. **Stored borrowed param.** A stored borrowed param, `(fn (x) (%pair x x))`,
   or a `chan/send` whose message is a param, escapes. It embeds in a
   longer-lived aggregate or crosses a fiber, so the store or fiber facet marks
   it. A param's region is a runtime placeholder the region projection treats as
   not-ownable, so marking it is sound and costs the consumers nothing.
2. **Store target vs stored value.** `(%put obj k v)` and `(assign acc v)`
   escape the stored *value*, never the *container* `obj`/`acc`. Writing into a
   container is not the container escaping. Escape marks only the value, and the
   projection acts per region, so the value's region is marked exactly once.
3. **Container-read escape.** A value stored into a container, then read back
   OUT (`first`/`rest`/`get`/`pop`) and ESCAPED, must be marked escaping too. The
   ownership forest records `content ⊇ container` at the store (a `%array-push`
   funnel). It would adopt the content into the container's Owned subtree and
   free it at the container's scope-exit subtree drop, under the escaped read
   reference.
   [region-container-read-escape-uaf.lisp](../../tests/impl/region-container-read-escape-uaf.lisp)
   pins that use-after-free.

   A per-container **stored-contents map** (`collect_container_contents`, built
   from the `push`/`put`/`add` store sites) closes it. A **read-result →
   container-contents flow edge** at each element-read makes an escaping read
   result pull the container's stored contents into its own facet through the
   ordinary fixpoint. It is PRECISE, not "every read escapes": the contents are
   marked ONLY when the read result itself reaches a facet. A container merely
   read or indexed, with the result consumed locally, keeps its Owned
   reclamation.
4. **Lexical capture is not escape.** A value captured by a closure that is
   *called in place* (never escapes) does not escape via capture. The capture
   facet marks it only when its capturing closure escapes, where `is_captured`
   marks every captured binding unconditionally. A value the closure *returns*
   still return-escapes through the closure's own tail, a separate facet.
5. **Fiber boundary.** An emitted or sent value crosses to the resumer or
   receiver, so the fiber facet marks it, and `fiber_frontier_sites` catches an
   atomless `(yield (%pair …))`. There is no compile-time RC edge at an `Emit`,
   since the runtime incref in `handle_emit` keeps it alive. The fiber crossing
   is purely escape's.
6. **Native `Mixed`/`Unknown` clique is conservative.** A native declared
   `Mixed` (uncounted store, examined) or `Unknown` (unexamined, the default)
   marks every heap argument escaping. It is as imprecise as the declarations are
   honest, and examining a primitive and declaring a tighter `RegionEffect`
   narrows it. For a single-argument native, whose clique is empty in any case,
   the seed is the whole cost. So a `Mixed` declaration is read here as a store
   claim and nowhere else, which is why a native that stores nothing declares
   `Opaque` ([region/effects.md](region/effects.md)).

## Verification

Escape is the authority, so it is pinned by its **own** spec, not by agreement
with another analysis. Three layers:

- **The unit tests** ([src/hir/escape/tests/](../../src/hir/escape/tests/mod.rs))
  assert each facet's discriminating behaviour directly. A returned value
  return-escapes. A stored value escapes its activation but is not returned. A
  value captured by a non-escaping closure does not escape. An emitted or sent
  value crosses the fiber frontier. An arg-returning callee propagates its arg's
  escape, and a `def`-bound callee does not.
- **The escape golden**
  ([tests/impl/escape-golden.lisp](../../tests/impl/escape-golden.lisp)) pins the
  normalized `escape` dump kind ([src/dump/escape.rs](../../src/dump/escape.rs))
  of a bounded set of real test files, byte-for-byte. Elle reaches the dump
  through `compile/dumps` → `:escape`, and the CLI through `--dump=escape`. The
  dump is id-normalized, so two compiles render identically. Its
  `[return_frontier]` section records escape's verdict projected to regions, and
  its `[region_instrs]` section records the RC instructions that verdict drives.
  A change to escape or its consumers shows up as a snapshot diff to review. The
  emitted RC may *tighten* as escape's precision lands, but it must never coarsen
  or introduce a UAF or leak. The set is bounded, because each `compile/dumps`
  call compiles its source twice and costs regions the run keeps
  ([test-runner.md](../test-runner.md)).
- **The `region-*.lisp` files and the probes in
  [tests/impl/probe/](../../tests/impl/probe/)** prove the projection reclaims
  soundly: no UAF (`--trace=guardfree` under the full stdlib), and no leak
  regression (every closed leak class stays closed).

## Relationship to the region forest

`EscapeInfo` is the durable artifact. It becomes the forest's Owned-vs-Shared
classifier, the hierarchical single-owner region endpoint the region work builds
toward. The lowerer's value-RC predicates `tail_arg_is_borrowed` and
`tail_callee_defers_release` (mint/adopt compensation) are **transitional**. The
forest reclaims an intra-fiber Owned subtree by drop, including any reference
cycle interior to it (no mint, no adopt). Edge-RC scopes reference counting to
cross-fiber Shared edges. So both predicates are subsumed, not preserved.

Lexical capture (`is_captured`/`needs_capture`) persists only as the structural
hint for cell layout, with no escape-authority. The capture *kind* has no
ownership authority either. The forest's capture-adopt emit reloads an adopted
captured value through whichever access path the kind implies: a binding slot
for a direct local, and the constructing function's environment for an upvalue
or transitive capture ([region/adopt.md](region/adopt.md)). So the subtree
admission filters bound the admission of a capture owner-edge, decisively the
lifetime obligation, never how the capture happens to be loaded.
