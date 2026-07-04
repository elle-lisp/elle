# Region rules — the implementor's correctness obligations

This is implementation-facing: the exhaustive correctness contract the
compiler and runtime must uphold. Read it before touching the region code.
For the consumer-facing model — how to write Elle that is sympathetic to the
memory system — see [docs/regions.md](../regions.md) and the
[docs/regions/](../regions/) series; the semantics (Tofte–Talpin for immutable
values, reference counting for mutation) and the single known leak are in
[regions/semantics.md](../regions/semantics.md).

## There are exactly two measures: correct, then optimal

A region implementation is **correct** iff it never reads freed memory and
never frees live memory — no use-after-free, no double-free, no dangling
pointer, and (the dual we hold ourselves to) no value leaked past the point
its last reference dies. There is no third state and no spectrum. Code is not
"conservative" or "aggressive"; it is correct or it is broken.

Everything else — how many physical regions exist, how aggressively the solver
merges, peak RSS, mmap churn — is **optimization**. Optimization may never buy
performance with correctness. A program that runs with one region per value and
frees each precisely is correct and slow; that is the baseline we must always be
able to fall back to. A program that merges regions to run fast but frees one a
step too early is broken, full stop.

Corollary for how we work: prove the *idea* correct, implement it, then prove
the *implementation* correct with a test written from the idea — not from what
the implementation happens to produce. Greening an individual failing test by
patching its symptom is how a codebase reaches "almost all tests pass" while
never being correct. The tests support the specification; they do not define it.

## The mechanism

A region owns pages and carries one `u32` reference count. RC starts at **1** —
the compiler's initial reference, i.e. the TT `letregion` owner. Cross-region
references raise it above 1.

- `IncrefRegion` raises RC. The runtime also auto-increfs at two points: scanning
  an immutable object's contents at allocation (`alloc_obj`), and storing into a
  mutable container at runtime.
- `DecrefRegion` lowers RC. At 0 the region's pages return to the pool **and**
  every region its contents reference is decremented (the cascade), recursively.
- `DecrefRegion` is the only demise instruction. There is no `FreeRegion`.

If a value escapes — into a container, a closure, a yielded signal — its region's
RC was already raised at the escape site, so the `DecrefRegion` at its `decref_point`
drops the initial reference without freeing. The region lives exactly as long as
RC says. There is no promotion pass that moves a value to a longer-lived region;
the value is born in the right region (Rule 3) and RC tracks the rest.

### Two resolutions: value-resolved and slot-resolved

Each region-RC instruction names its region in one of two ways:

- **value-resolved** (`IncrefValueRegion`/`DecrefValueRegion`): the operand is a
  *register*; the runtime reads the value and asks `region_of` for its physical
  region. This is the honest encoding whenever the region cannot be named at
  compile time — a passed-through arg, a branch-dependent mix, an opaque call
  result, a runtime mutable store. The prediction-free calling convention is
  built on it: a callee hands its caller one owning reference to the result's
  *runtime* region (`IncrefValueRegion` at every tail) and the caller consumes it
  (`DecrefValueRegion` at the result's `decref_point`), neither side naming the
  other's region statically.
- **slot-resolved** (`IncrefRegion`/`DecrefRegion`): the operand is a
  *`StaticRegion` slot*; the runtime resolves it through the current activation's
  `activation_region_map` to the physical region this execution minted for that
  slot. Usable only where the region is statically known.

### Compile-time region selection (coalescing)

Where the compiler can prove a value is a **fresh local allocation whose region
is a known slot** — the value was allocated in this function (`alloc_region` has
an entry, or for a returned binding `binding_source_regions` resolves to one such
region), that region is `live`, and it is none of the dynamic classes below — it
substitutes the slot-resolved `IncrefRegion` for the value-resolved
`IncrefValueRegion` (and likewise on the decref side). This is **instruction
selection, not a change of RC unit**: the slot resolves — through the activation
map — to the *same physical region* `region_of(value)` would return, because the
allocation stamped that slot to that region and a value never moves regions. So
every region's RC trajectory is bit-identical and leak counts and teardown
residue are unchanged *by construction*. The win is one fewer runtime deref per
coalesced site, and the slot-resolved form touches no operand stack (the value
register stays on top as the return value — stack-neutral).

The substitution is **purely callee-mint-side** for the return convention: the
caller still cannot name the callee's region, so the caller's balancing
`DecrefValueRegion` stays value-resolved. The pervasive coalescible site is the
prediction-free return mint at every function tail. The two narrower sites are
both reassigned-binding traffic over a value the lowerer just allocated locally:
the **reassign incref-on-store** (`lower_assign`'s drop-on-overwrite — pinning a
1-slot container's new content; coalesces only for a *fn-local* container, since a
*module-scope* container's value is in `mutated_binding_value_regions` and stays
value-resolved), and the decref-side **captured-reassign init-drop**
(`store_captured_cell_init` — dropping the producer's reference to a captured
binding's fresh init value, `DecrefValueRegion` → `DecrefRegion`). Both reach the
same `coalescible_region` predicate, so the runtime-population guard (the region's
slot must be stamped by an allocation emitted in this function) refuses any
captured/cross-thread value at all three sites alike.

The reduction this buys — coalesced (slot-resolved) versus value-resolved mints at
the candidate sites, plus the self-edges eliminated below — is *measured, not
asserted*: the lowerer records each decision in the thread-local instrument
`lir::lower::rcstats` (the choice is not recoverable from the final LIR — a
coalesced mint's `IncrefRegion` is indistinguishable from a store-edge's, and an
eliminated self-edge leaves no instruction), and `benches/regionrc.rs` reports the
totals across the stdlib load and the `tests/elle` corpus.

### The dynamic boundary (stays value-resolved)

These sites are genuinely runtime facts and must **never** coalesce — the region
is not knowable at compile time:

| Site / class | Why it stays value-resolved |
|---|---|
| caller-side `DecrefValueRegion` of a call result | the caller cannot name the callee's region — prediction-free by design |
| tail borrowed-arg incref (`tail_arg_is_borrowed`) | a captured upvalue / env **cell** region — dynamic |
| tail native-result retain | pass-through native result; region named only at runtime |
| reassign drop-old (`DecrefValueRegion{old_reg}`) | the displaced 1-slot-container content — the runtime fact the container tracks |
| `Mixed`/`Unknown` `RegionEffect` results | region unknown; the clique is a may-store over-keep |
| pass-through natives (`first`/`rest`/`get`) | result lives in an arg's region, named only at runtime |
| capture cells (`cell_release_regions`, `DecrefCellRegion`) | release frees the *cell's* own region, not the inner value's |
| phantom param regions | no `alloc_here`, filtered from `live_regions` — runtime-counted |
| suspended frames | `activation_region_map` captured/restored across resume; the slot is per-activation |
| terminal fiber signals | set-once park-retain, no compile-time edge |
| runtime mutable-store traffic | the `push_with_incref` funnel counts at the store site (the TT gap is dynamic) |
| ownership-forest ops (`AdoptRegion`, `FreeRegionGroup`, `AdoptIntoActivation`) | a forest member is a runtime fact (a call-result / cross-activation region); `AdoptIntoActivation`'s parent — the activation's pages-less owner node — has no slot at all (docs/impl/region-model.md § "Owner nodes") |

### Self-edge elimination

`emit_increfs_for` emits one `IncrefRegion(source)` per cross-region store edge
`(site, source, target)` — a value in `source` stored into a structure in
`target` — balanced by `target`'s free-time cascade at `DecrefRegion(target)`.
The cascade **skips self-references** (`regionpool/introspect.rs` decrefs a
referenced region only when `rid != own_id`). So a `source == target` self-edge
`R→R` has no balancing decref: keeping its `IncrefRegion(R)` **leaks** `R`.
Eliminating a self-edge is therefore the sound transform — the compiler-side
mirror of the cascade's own `own_id` self-skip. It is the *only* redundant case:

- **alias edges** — `(%pair x x)` and repeated-arg shapes emit N edges into a
  *distinct* target; the cascade finds N references and decrefs N times, so all N
  increfs are required. Collapsing them is an over-collapse UAF.
- **may-store clique edges** — over-approximations whose balancing decref is the
  target's runtime content scan (per *actual* store, not per emitted edge).
  Eliminating them trades a known leak for a possible UAF.

A self-edge appears only when a region **merge** collapses a store edge's source
and target into one region (a value merged into the aggregate it is stored into);
see [region-model.md](region-model.md) § Merging. The compiler detects one with
`RegionInfo::is_merge_self_edge` — `merged_root(source) == merged_root(target)`
over the merge forest — which is exactly the slot coincidence the merged
allocation resolves to (`static_slot` canonicalizes through that forest). When the
predicate fires, `emit_increfs_for` **drops** the `IncrefRegion` rather than
emitting it; the detection isolates the redundant self-edge from the two must-keep
classes above by construction, because the merge seed never collapses an escaping
alias (it is not sole-held) nor a clique edge (it is not a `%pair` immutable
store), so their endpoints keep distinct merge roots.

This elimination is half of one mechanism with the merge's allocation
canonicalization and child-decref suppression (region-model.md § "Emission: one
slot per merge tree, one demise at the root"): a self-edge dropped without the
merge frees early, and a merge without the drop leaks, so neither side is emitted
without the other. Its correctness net is not a per-edge runtime assert — once both
endpoints share a slot, a slot-vs-slot check is a tautology — but the compile-time
decref-dominance assertion (exactly one `DecrefRegion` per merged slot,
`record_merged_slots`) together with `--trace=guardfree` over the builder corpus
(an over-collapse surfaces as a UAF; a self-edge left in place grows the live
region count). The pinning test is the canonical reference
(`tests/elle/region-merge-builder-loop.lisp`).

### The equivalence oracle

A mis-coalesce is a use-after-free: a slot resolved to the wrong physical region
makes its cascade free a live region. The net for a coalesced *mint* (the
value→slot substitution, § "Compile-time region selection") is the debug-only
`AssertRegionMatches { region_id, src }`, emitted immediately before every
coalesced `IncrefRegion`. (Self-edge *elimination* carries no coalesced incref to
guard — its net is the decref-dominance assertion and guardfree, § "Self-edge
elimination".) In the bytecode interpreter it panics when
`activation_region_map.resolve(region_id) != region_of(src)` — turning an
inference bug into a deterministic panic at the exact instruction, under the
trustworthy guardfree oracle, instead of a later heap corruption (the mirror of
the native-effect declaration oracle, [region-effects.md](region-effects.md)).
Release builds and the JIT/WASM tiers treat it as a no-op (the GPU tiers exclude
any function carrying it via the `is_gpu_instruction` whitelist); their coalesced
sites are covered instead by the runner's cross-tier divergence detection and the
escape golden. The instruction renders into no `[region_instrs]` golden line — it
is scaffolding, not part of the semantic RC stream.

## The rules

These are exhaustive and each carries its exceptions inline. A violation of any
is a correctness defect, not a tuning knob.

1. **Every allocation has a region.** Region ids are nonzero, with no default
   fallback: an allocation the solver did not assign a region is an
   analysis gap and must panic at allocation — never silently leak. (No
   exceptions: a sentinel region is the bug, not the handling of it.)

2. **Every region corresponds to a real allocation** (the dual of Rule 1). A
   region the solver hands out must have an instruction that raises its RC, or
   its `DecrefRegion` underflows or aliases a neighbour. The test is operational
   — *does the lowerer emit an RC-raising instruction at this HirId?* — not
   syntactic. Exceptions, named here:
   - *Opaque `Call`/`Eval`*: the result is allocated in Rust (or a callee's
     own compilation), in a region the outer compilation did not create. The
     solver assigns a *placeholder* region and the lowerer releases **by
     value** at the placeholder's `decref_point`: read the actual returned
     value and decref the runtime region it lives in (`DecrefValueRegion`),
     consuming the one owning reference the callee's return convention handed
     back (`IncrefValueRegion` at every `Return`). Two shapes:
       - **bound result** (a `let`/`def`/synthetic slot): load the slot,
         release, stamp the slot nil (the branch-result-loop discipline);
       - **discarded result** — the placeholder's `decref_point` is the call
         node itself and no slot exists (ANF's propagating-tail wrap keys the
         slot on the outer `Let`, not the tail `Call`): release the
         freshly-lowered result register directly. Skipping the release here
         leaks one object per iteration in loops (tests/elle/arena-count.lisp).
     A branch-union region whose `decref_point` lands on a call node is NOT
     that call's own result region (`alloc_region[hir] ≠ r`) and keeps the
     slot path.
   - *Transparent nodes* (`MakeCell`/`DerefCell`/`SetCell`, and non-allocating
     intrinsics `%get`/`%put`/`%length`/`%type-of`): emit no RC-raising
     instruction, so the solver must assign no region and pass the child's
     regions through instead.

3. **Values are born in the right region.** The allocation instruction targets
   the solver-assigned region directly. Never allocate into a short-lived region
   and promote — there is no un-merge and no promotion primitive. (No exceptions.)

4. **`DecrefRegion` fires at the point of demise, exactly once per activation.**
   The `decref_point` is the value's last-use program point — often a scope exit, but
   equally a loop back-edge, a tail-call boundary, a return. A *consuming
   node* is itself a use of its operand's regions: `Return` extends a
   returned region to the Return node, and `Destructure` extends its
   value's regions to the Destructure node — the field extraction reads
   the value AFTER the value expression's own last read, so anchoring the
   release at the inner read frees the source under the extraction (the
   `&named`-param prologue UAF: with every destructured binding unused,
   the collected keyword struct's only use was the prologue's Var, and the
   lowerer freed it before `StructGetOrNil` read the fields —
   tests/elle/region-named-param-uaf.lisp). It is *per
   activation*: each activation remaps its static region slots to fresh physical
   regions, so the same static `DecrefRegion` frees a different physical region
   each call. Exception, named: across a fiber **suspend/resume** the activation's
   static→physical map (`activation_region_map`) is captured at suspend and restored at
   resume, so the resumed continuation's `DecrefRegion`s resolve to the *same*
   physical regions and still fire once. Re-running a `DecrefRegion` against a
   region a still-live binding holds is a double-free — the canonical fiber-resume
   defect.

   When several releases land on one `decref_point`, their emission order is
   **deterministic and dependency-safe**: releases that *read* pages come before
   releases that *free* pages. Concretely — `DecrefValueRegion` (loads a slot and
   derefs the value, unwrapping a capture cell to reach the inner value's region)
   is emitted first; then `DecrefCellRegion` (reads the cell's page header via
   `region_of`); then plain `DecrefRegion` (no page reads). A page-freeing
   release ordered before a page-reading release of the same point is the
   capture-cell over-release UAF: the cell's `DecrefRegion` frees the cell's
   pages, then the init value's `DecrefValueRegion` unwraps the freed cell. The
   order must also be deterministic across compiles — release order may never
   depend on hash-map iteration.

5. **RC tracks every cross-region reference — every escape increfs, every drop
   decrefs.** This is the whole soundness obligation, and it is only as sound as
   this list is complete. The escape sites, exhaustively:
   - *immutable contents* — `alloc_obj` scans the new object and increfs each
     region its fields point into;
   - *mutable store* — `push`/`put`/`add`/`%put`/`insert` incref the stored
     value's region; `pop`/`del`/`remove` decref it. This entry is **statically
     complete**: the raw `RefCell` accessors for the `Value`-bearing mutable
     containers (`@array`, `@struct`, `@set`, box, capture cell) are visible
     only inside `value/` (`as_*_cell`, conversions.rs), so the only way code
     elsewhere can store into one is through the tracked funnels in
     `value/arena.rs` (`push_with_incref` and friends) — an uncounted
     container store is a compile error, not a review item. Read access goes
     through borrow-guard/copy-out accessors that cannot mutate.
     Membership-neutral mutation (in-place sort/reverse/shuffle — no value
     enters or leaves the container) is region-neutral and gets its own
     funnel that grants mutable access without RC traffic. Residual channels,
     named: `HeapObject`'s fields are still `pub` for construction and the
     deep-copy machinery (a direct field match could bypass the seam — don't;
     the accessor channel is the one closed here), and an `External`'s
     `Rc<dyn Any>` payload is opaque to both scan and seam;
   - *native call result pass-through* — `first`/`rest`/`get` and friends return a
     value from another region; the call increfs it (a "new reference" in the
     CPython-C-API sense), and the caller's `DecrefValueRegion` consumes it;
   - *captured closure env* — the closure→env cross-region edge is increfed when
     the closure is built;
   - *reassigned mutable binding cell* — a reassigned binding is a 1-slot
     mutable container (see
     [region-bindings.md](region-bindings.md)): the store increfs the new
     content's region, the overwrite decrefs the displaced content's region,
     and a binding read out of a reassigned **captured** cell takes a counted
     reference (incref at the bind, value-based release at the reader's last
     use) — the cell's overwrite-release cannot see uncounted holders;
   - *suspended frame* — a heap-promoted activation record holds cross-region refs
     (captured env, saved operand stack) and owns its `activation_region_map`; these are RC
     roots, increfed at suspend and released at resume-consume **and** at
     squelch/abort discard (an unbalanced discard underflows);
   - *terminal fiber signal* — a child's set-once return/error/halt result, read
     later via `fiber/value`, is park-retained when the fiber goes terminal and
     released by the signal scan when the fiber is freed.
   Every entry has a matching decrement. Missing an escape site is a
   use-after-free; missing the matching decrement is a leak. The list being
   complete *is* correctness for the RC half.

6. **No commingling.** Objects from different regions never share a page —
   otherwise freeing one region cannot munmap its pages while another's objects
   sit on them. (No exceptions.)

7. **The cascade is complete.** Freeing a region decrements every region its
   contents reference. Immutable contents cascade via compiler-emitted decrefs;
   mutable contents cascade via a bounded walk of the container at free time. A
   scan-at-alloc must be symmetric with the scan-at-free — only valid for
   immutable contents. Exception, named: the terminal-signal retain (Rule 5) is
   asymmetric by design — no incref at fiber allocation (the signal is `None`
   then), the park-retain supplies the incref and the free-time signal scan
   supplies the decref; this is balanced only because it is scoped to a set-once
   terminal value.

8. **No leaks.** A heap value whose last reference is dropped is freed at that
   point. The *only* values permitted to outlive the program are true
   process-lifetime roots — the symbol table and imported shared objects — held
   for the process by a real reference; those are roots, not leaks. (Native-fns
   also outlive the program, but they are immediate `&'static` `prim_id` values
   that occupy no region at all — there is nothing to leak.) The test for a root
   is **allocated exactly once per process**: a value re-allocated on every
   `(eval …)` or module load is not a root no matter how "compile-time" it looks.
   A scope that drops a value without freeing it is a defect, including the
   mutable-cycle case of the [theory](../regions/semantics.md) (which we tolerate
   only because it is currently the sole known incompleteness, not because
   leaking is ever correct).

## Soundness checklist

The rules above, as the list to verify against any change:

1. Every allocation has a region (no region 0).
2. Every region has a real allocation (opaque calls use value-gated release).
3. Values are born in their final region (no promotion).
4. `DecrefRegion` fires once per activation, at the point of demise (the
   `activation_region_map` preserves this across resume).
5. Every cross-region escape increfs and every drop decrefs (the escape-site list
   is complete).
6. No two regions share a page.
7. The free cascade is complete and symmetric with alloc-time scanning.
8. Nothing leaks but true process-lifetime roots.

## Teardown — every region frees

The naive user model is `elle foo.lisp` ≡ `(eval (wrap-in-letrec (read-all
(slurp "foo.lisp"))))`: after that `eval` returns and its result is dropped, the
world is back to its pre-`main` state — **every** region the process created is
freed. The only things that persist are true process-lifetime roots and the
native-fn primitives, which are immediate `&'static` values occupying no region.
Even the stdlib, prelude, core env, and trait tables are torn down before the
process exits — they are resident *roots*, not eternal.

One contract drives every entry path — running a file, graceful REPL exit, the
embedding API, and the lint path (one runtime per call; the resident LSP VM is
the deliberate exception, one long-lived runtime for the server's life). All run
through a single `Runtime` (`src/runtime.rs`): `Runtime::new` installs the heap,
registers primitives, and optionally loads the stdlib, recording the
process-resident roots in the process-root registry; `Runtime`'s `Drop` (or an
explicit `Runtime::teardown`) runs the sweep. One teardown routine, so the paths
cannot drift.

Two non-negotiable properties:

1. **RC-driven, never iterate-and-free.** The sweep releases the *roots* —
   decrefs each registered process-root region exactly once — and lets the
   ordinary RC cascade (Rules 5 and 7) reclaim everything reachable. It never
   walks the region table freeing entries. Force-freeing live regions would mask
   the very leaks and missing-decref defects this contract exists to surface:
   freeing-by-iteration always "succeeds" and proves nothing; freeing-by-RC
   succeeds only when the accounting is correct.

2. **Observable.** The sweep reports the live region census afterward
   (`Runtime::teardown` returns it; `--stats` prints it). The target is **zero**
   regions remaining. A non-zero residue is the standing list of open leaks (the
   leak-suite suspects, `tests/elle/leak*.lisp`): the number *is* the remaining
   work, not a tuning knob.

Because the sweep is RC-driven, the residue equals the set of regions whose RC
never reached zero — the true leaks — rather than being hidden by a blanket free.
As the leaks are fixed the residue falls to zero with no change to the teardown
itself.

## Macro expansion — a closed allocation scope

A macro transformer runs at compile time and builds its expansion as a tree of
runtime `Value`s: the quasiquote template lowers to `list` / `append` / `array`
constructor calls (`quasiquote_to_code`), and the transformer body executes them
to produce the output. The expander then **deep-copies that result into owned
`Syntax`** (`Syntax::from_value`, whose contract forbids any surviving arena
pointer — the `contains_syntax_literal` debug assert), after which *every region
the transformer minted is dead*: the returned tree, its interior nodes, and the
scratch a constructor discards internally (an `append`-copied segment list)
alike.

The transformer body is ordinary compiled code, so the region solver gives it
the ordinary tail-return treatment: the result region's decref is **suppressed**
(it is in the return frontier — escape's return facet) because a function's caller
releases its result via the return convention (Rule 5, `ReturnValue`), and
tail-flowing native call-results inherit the same suppression. For an ordinary
call that is exactly right — the caller's `DecrefValueRegion` consumes the one
returned reference and the cascade reclaims the rest. But the macro caller is
Rust code that keeps only a *deep copy*; releasing solely the single result-root
region would leave every other suppressed/escaped scratch region holding one
unbalanced owner reference. At stdlib scale (thousands of expansions, each with
several scratch `Pair`s) that residue dominates teardown.

So macro expansion is treated as a **closed allocation scope**. `expand_macro_call`
records the regions minted across the transformer call (`begin_mint_log` →
per-call `(id, generation)` log on the heap, generation-stamped so a recycled id
names the right incarnation) and, after `from_value`, reclaims the scope by
balancing each surviving region's **unexplained** references — its RC minus the
in-degree it gets from other live regions (the same quantity the residue census
reports). That is exactly the owner references the transformer never released:
balancing them lets the ordinary cascade (Rule 7) reclaim the whole immutable
scratch DAG. This is RC-driven, not a blanket free — a region kept alive by a
real edge (its in-degree covers its RC) is left untouched, so an edge from a
persistent cell into freshly-built scratch survives intact. (The boundary
documented here is the inverse of force-freeing the teardown sweep forbids: there
the whole heap must reclaim by RC so leaks stay visible; here a *provably closed*
scope balances precisely its own unreleased references and nothing else.)
