# Region-Based Memory Management

## What this document is

The target architecture for Elle's memory system. Not a history of what
was tried (see `/dev/shm/memory-take-five.md` for that). Not a description
of the current code (which is mid-transition and broken). This is the
destination.

## Goals

1. **O(1) fiber death.** munmap the fiber's pages. Done.
2. **Zero-copy yield.** A yielded value is already on pages the parent
   can reach. No deep copy.
3. **Bounded RSS in loops.** Any loop that doesn't accumulate live data
   runs in bounded memory without programmer intervention.
4. **Deterministic reclamation.** Memory is freed at known program points.
   No GC pauses, no stop-the-world, no write barriers.
5. **Soundness.** No use-after-free, no double-free, no dangling pointers.

## Non-goals

1. No tracing garbage collector.
2. No runtime liveness scanning.
3. No Rc<Value>. Value is Copy (16-byte tag + pointer).

## The core idea

Every value logically lives in its own region. A region is a physical
allocation unit — it owns pages of memory. A region has a reference
count (RC) that starts at 1 when created (the initial reference, owned
by the compiler) and tracks cross-region references beyond that.

`DecrefRegion` decrements RC; when RC reaches 0, the region's pages
are freed (returned to a page pool) and cascade decrefs fire for any
cross-region references found in the region's contents. `IncrefRegion`
explicitly increments RC. The runtime also auto-increfs cross-region
refs at allocation time (`alloc_obj`) and at mutable-collection
push/put time. `DecrefRegion` is the only region-demise bytecode —
there is no separate `FreeRegion`.

The compiler may merge regions whose lifetimes provably coincide
(identical `free_at` and identical incref topology). This is an
optimization that reduces the number of physical regions (and thus
mmap/munmap overhead). The system must be correct without it, but it
is required for acceptable performance.

## Unique regions are the default; merging is the optimization

Every value starts in its own unique region. This is the base
mechanism, not a fallback. The solver's only job is to *merge* unique
regions when it can prove their points of demise coincide — pure
optimization, governed by point-of-demise analysis.

If the solver fails to merge anything, the program is still correct:
it just runs with more regions than necessary, paying RC bookkeeping
costs for every cross-region reference. Failure to merge is a
performance concern, never a correctness concern.

The inverse mistake — starting allocations pre-merged into a "current
scope region" and trying to *un-merge* values that need to escape — is
backwards. There is no un-merge primitive. The solver cannot pull a
value out of a region it was born into. Allocations must start unique
and stay unique unless the solver actively proves a safe merge.

The Tofte-Talpin region calculus operates on lifetimes, not on
syntactic scopes. The solver's data structures may use a tree to
compute lifetime relations, but every allocation site emits a fresh
region by default, and constraints exist solely to discover which
regions can be safely identified with which other regions.

## Merge rule: identical free_at AND identical incref topology

Two regions may collapse into one when both hold:

- Identical `free_at` — the same HirId is where the compiler emits
  their `DecrefRegion`.
- Identical incref topology — the same set of cross-region edges
  flows in *and* the same set of runtime-incref sites (push, put,
  capture, emit) acts on them at the same program points.

Different incref topologies mean the merged region's RC would track
the union of references, extending one value's lifetime to the
other's. Even though the user-visible bytecode would still be sound,
this defeats the point of regions (values stay alive longer than
necessary).

The conservative starting policy is to merge only regions whose
`free_at` matches *and* which have no cross-region edges incident to
either. Tighter conditions await profiling. Failure to merge is a
performance concern, not a correctness one.

Merging is monotonic and iterative. A region that already contains
several values may merge again with a third region if the condition
holds. Once merged, always merged; there is no un-merge.

## DecrefRegion fires at points of demise, not at "scope exits"

A region's compiler-emitted `DecrefRegion` is placed at the program
point where the value's last use ends — the region's `free_at` HirId.
That point happens to often *be* a lexical scope exit, but it can
equally be a loop back-edge, a tail-call boundary, a `break`, a
function return, or any other program point where the compiler can
name a moment after which the initial reference is no longer needed.

Saying "the scope emits a `DecrefRegion` for the region" is a
category error. Scopes don't own regions. The compiler emits
`DecrefRegion` at program points; the solver merges allocations into
shared regions only when their `free_at` HirIds and incref
topologies coincide, so that one `DecrefRegion` suffices for several
allocations whose initial references are dropped at the same program
point.

If a value escapes — flowing into a container, captured by a closure,
yielded to a scheduler — its region's RC has already been incremented
elsewhere by the time `DecrefRegion` fires. The decref drops the
compiler's initial reference but the region survives because RC > 0.
There is no "widening" pass that moves a value to a longer-lived
region; the region's lifetime is whatever the RC says it is at
runtime.

## Region IDs must be valid

There is no "region 0", no "default region", no "global region", no
"unstamped region." Every allocation must have a valid region
assignment from the solver. An allocation without a region assignment
is a defect in the analysis — it must panic, not silently leak.

The representation should make the invalid state unrepresentable:
`Option<RegionId>` where `None` panics at allocation time, or a
newtype that cannot be constructed with a sentinel value. Any code
path that produces an unassigned region is a bug to be fixed in the
solver, not a condition to be handled gracefully at runtime.

The current code uses region_id 0 as a catch-all for unassigned
allocations and silently never frees them. This is wrong. It allows
analysis gaps to hide as leaks instead of surfacing as crashes. Every
occurrence of region 0 in the codebase is a defect to be eliminated.

## Every region must correspond to a real allocation

The dual of the above: every region the solver hands out must
correspond to an `alloc_in_region` instruction the lowerer actually
emits. A region with no allocation is just as broken as an allocation
with no region: its `DecrefRegion` decrements a counter that no
`IncrefRegion` ever raised, producing underflows or — worse — silent
conflations with neighbouring region IDs reused by other allocations.

The criterion for "this HIR node allocates" is therefore not
syntactic ("DerefCell looks like it produces a value") but
operational: **does the lowerer emit an instruction at this HirId
that increments a region's RC?** If yes, the node allocates and the
solver must assign it a region. If no, the node is transparent at
this layer and the solver must not manufacture a region for it — pass
the child's regions through instead.

Concretely:

- `Call`, `Eval`: opaque. The result lives in a region the outer
  compilation didn't allocate. The solver hands out a placeholder
  region and registers it in `call_result_regions`; the lowerer
  emits `ReleaseValueRegion(expected)` at the free_at, value-gated so
  the runtime skips the decref when `region_of(value)` doesn't match
  the placeholder.
- `Lambda`, allocating `Intrinsic` (`%pair`, `%freeze`, `%thaw`):
  real allocation. Solver assigns a region; lowerer emits
  `MakeClosure`/`List`/`Freeze`/`Thaw` via `emit_alloc`, stamping
  the bytecode with the region.
- `MakeCell`, `DerefCell`, `SetCell`: **transparent** under the
  current double-handling contract (see `src/lir/lower/binding.rs`
  lines 698-718). The actual cell allocation happens implicitly in
  `lower_let`/`lower_letrec` at the binding site (`MakeCaptureCell`
  emitted via `emit_alloc` at the *Let's* HirId); the unwrap at
  reads happens implicitly in `lower_var` via `LoadCapture`. The
  `MakeCell`/`DerefCell`/`SetCell` HIR markers exist only for the
  analyses between functionalize and the lowerer; the lowerer emits
  zero instructions for them. The solver must not call `alloc_here`
  at these nodes.
- `Intrinsic` ops that do not allocate (`%get`, `%put`, `%del`,
  `%length`, `%type-of`): the result is an existing value (or an
  immediate). `%get`/`%put`/`%del` pass through `arg_regions[0]` —
  the result lives in the input collection's region. `%length`/
  `%type-of` return no region.

The same rule will eventually retire the `MakeCell`/`DerefCell`/
`SetCell` HIR markers entirely (Phase 3 of the binding-lowerer
work) — they're scaffolding that survives only as long as the
lowerer's "implicit cell creation" path coexists with
functionalize's "explicit cell ops" path.

## Region assignment is compile-time

The solver analyzes data flow in the functionalized HIR and assigns
each allocation site to a region. The assignment is an annotation on
the allocation instruction in the bytecode. At runtime, the allocation
routes to that region's physical pages.

There are no hidden region parameters passed through Elle function
calls. The compiler can see through Elle function bodies — it has the
HIR and traces data flow across call boundaries within a compilation
unit. If `f` calls `g` which allocates a value that flows back as
`f`'s result, the solver sees the full path and assigns the allocation
to a region whose lifetime matches the result's actual point of demise.

The only case requiring runtime region routing is **opaque calls** —
NativeFn primitives implemented in Rust, where the compiler cannot see
the function body. For these, the VM sets a TLS opaque-call-region variable
before the call so the primitive's internal `alloc()` calls pick up
the correct region automatically.

## Regions are not fibers

A region is not owned by a fiber. A region is an independent entity.

A value allocated by a child fiber and yielded to the parent lives in
its own region. That region has RC=1 (initial reference) plus
additional increfs for cross-region references. When all references
are released — including the compiler-emitted `DecrefRegion` at the
region's `free_at` — RC reaches 0 and the region's pages are freed.

No copying happened. The parent received a Value (a 16-byte tag+pointer)
pointing into the region's pages. The region outlived the child fiber
because its RC was > 0.

A fiber's "own" allocations are just allocations whose regions happen to
have RC=0 when the fiber dies. They are freed in bulk. But there is no
"fiber-wide region" introduced automatically — regions exist to hold
actual values, not speculatively.

## Regions are not scopes

A scope may contain values from multiple regions, and a value's region
is not determined by the scope it's syntactically inside — it's
determined by the solver based on where the value actually dies.

```lisp
(let [x (make-thing)        ; x allocated in ρ1
      y (make-other-thing)]  ; y allocated in ρ2
  (push acc x)               ; acc's region now references ρ1; ρ1.rc += 1
  y)                         ; y is the result
```

The solver sees that `y` escapes this let — it flows to whatever
expression contains the let. So the solver assigns `y`'s allocation
to a region (ρ2) whose lifetime extends beyond this scope. ρ2 might
be introduced at the grandparent scope, or wherever the value
ultimately dies. The allocation of `y` inside `make-other-thing`
targets ρ2 directly — `y` is born in the right region. No rescue, no
promotion, no RC dance for the result path.

At scope exit:
- ρ1 has RC > 0 (acc references it). ρ1 is NOT freed.
- ρ2 was introduced outside this scope (by the solver). This scope
  exit doesn't touch ρ2.
- Regions for temporaries that don't escape have RC=0 and ARE freed.

The scope doesn't own the regions. The scope is a program point where
the compiler knows certain references are dropped. Dropping a reference
decrements the referenced region's RC. If RC hits 0, the region is freed.

## Regions are flat

Regions are not nested, not hierarchical, not tree-structured at
runtime — neither logically nor physically. A region is an opaque ID.
The solver may use internal data structures (trees, lattices) to
compute assignments, but the output is a flat set of region IDs. No
region is "inside" another region. No region is "parent" or "child"
of another. The only relationship between regions at runtime is
cross-region references tracked by RC.

A scope is not a region. A region is not a scope. The solver assigns
allocations to regions based on data flow — where values actually die.
A scope exit can be the `free_at` for some region (so a `DecrefRegion`
fires there), but the scope doesn't "own" the region. Multiple scopes
might share a region (after merging), or a scope might have no region
(no local allocations). The solver links HirIds to regions; the
runtime sees only flat IDs.

## Regions in loops

A loop body creates values. Some are temporaries (dead at the back-edge).
Some are passed to the next iteration via recur. Some escape into
accumulators via push/put.

Each of these gets its own region (logically). At the back-edge:
- Temporary regions have RC=0 (nothing references them after this
  iteration). Freed.
- Recur-arg regions have RC > 0 (the next iteration references them).
  Not freed. They persist until the next iteration drops the reference
  (e.g., by overwriting the loop parameter), at which point RC may
  hit 0 and the region is freed.
- Escaped regions (push'd into an accumulator) have RC > 0. Not freed.
  They persist until the accumulator is freed.

There is no double-buffering. There is no "current iteration arena" vs
"next iteration arena." Each value has its own region with its own RC.
Some regions live for one iteration. Some live for two. Some live
forever. The RC tracks this precisely.

## Data flow through calls

For `(f (g (h x)))`:
- `h`'s result is allocated in its own region (logically). It becomes
  `g`'s input and may die inside `g`.
- `g`'s result is in its own region. It becomes `f`'s input.
- `f`'s result is in its own region. It flows to whatever contains
  this expression.

The solver sees through all three calls (within the compilation unit)
and assigns each allocation to a region based on its actual point of
demise. Each intermediate result gets a different region. The lowerer
annotates each allocation instruction with its solver-assigned
region ID.

## The solver's role

The solver has two jobs:

1. **Correctness**: assign each allocation to a region whose lifetime
   is at least as long as the value's last use. This is mandatory.

2. **Optimization**: merge regions with identical lifetimes to reduce
   the number of physical regions. This is important for performance
   (fewer mmap/munmap calls, less internal fragmentation) but does not
   affect correctness.

When the solver cannot prove that two regions share a point of demise,
it leaves them separate. The RC system handles this correctly — each
region is freed independently when its RC hits 0. The solver's
inability to optimize a particular case costs performance (more
physical regions) but never causes unsoundness.

## Physical representation

Per-thread page pool with multiple size classes (a combination of
pool-based and segregated approaches).

The constraint: objects from different regions must NOT share physical
pages. Otherwise, freeing a region (RC=0) cannot return memory to the
OS — other regions' objects on the same pages prevent munmap.

The current code violates this: all regions share one slab per fiber.
`free_region()` returns individual slots to a free list but cannot
munmap pages. This reduces region freeing to internal recycling, not
OS-level memory return. This is the fundamental problem with the
current implementation.

### Design

A per-thread pool manages pages in multiple size classes. When a
region needs space, it claims a page of the appropriate size from the
pool. When a region is freed (RC=0), its pages return to the pool.
The pool can munmap excess pages under memory pressure.

Pages are never shared between regions. A region that needs 80 bytes
claims the smallest available page size. Internal fragmentation within
a page is the cost of non-commingling. Region merging amortizes this
cost by consolidating many logical regions into fewer physical ones.

Region merging is required for acceptable performance. Without it,
the un-optimized case (one region per value) claims one page per
allocation, which is far too expensive.

## Reference counting

RC is per-region, not per-object. One counter per region.

Increment when: a value in region A is stored into a data structure
whose backing storage is in region B. Region A's RC increases by 1,
because region B now references region A.

Decrement when: the reference from region B to region A is removed
(overwrite, scope exit, region B freed).

When RC hits 0: free the region's pages. No traversal of contents
(for the immutable case — see cascading frees below).

### Two paths for RC management

**Compile-time path (immutable data):** The compiler knows the
contents of immutable structs, arrays, closures, pairs at every
program point. It emits IncrefRegion/DecrefRegion instructions at
the exact points where cross-region references are created or
destroyed. No runtime inspection needed.

**Runtime path (mutable collections):** Mutable collections (`@[]`,
`@{}`, `box`) can have their contents changed by `push`, `pop`,
`put`, `del` at runtime. The compiler cannot predict the contents
at every program point. So:

- `push val into collection`: inspect val's region, incref it
  (collection's region now references val's region)
- `pop from collection`: inspect the popped value's region, decref it
- `put key val into collection`: decref old value's region, incref
  new value's region
- `del key from collection`: decref removed value's region

When a mutable collection's region is freed, the runtime walks its
contents and decrements each referenced region. This is a bounded
scan proportional to the collection's size, not a heap-wide scan.

This is a clean split: mutable collections are the only things whose
contents the compiler can't fully predict, and they're the only things
that need runtime region inspection. Everything else is compiler-
generated.

### Cascading frees

When a region is freed, its contents may reference other regions.
The referenced regions get their RC decremented. If any reach 0,
they are freed (recursively).

For immutable contents, the cascade decrements are compiler-generated.
For mutable contents, the cascade requires walking the collection at
free time.

## The opaque call problem

The compiler assigns regions by tracing data flow through Elle function
bodies. But NativeFn primitives are Rust code — the compiler can't see
their allocations.

`(string "hello" " " "world")` calls a Rust primitive that internally
calls `FiberHeap::alloc()`. The compiler can't annotate that allocation
with a region ID because the allocation happens in Rust, not in bytecode.

Fix: `alloc()` samples a thread-local (TLS) variable to find the
active region. The VM sets this variable before calling the primitive
(based on the solver's region assignment for the Call node) and clears
it after. The primitive's internal `alloc()` calls pick up the region
automatically — no API change to NativeFn, no region handle threaded
through Call arguments. This keeps the primitive interface simple and
the region plumbing invisible to Rust code.

This TLS approach is temporary. The NativeFn signature will eventually
change to pass an allocator (like Zig), eliminating TLS entirely.

## Current status

| Component | Notes |
|-----------|-------|
| Region solver | ~1500 lines, 56 tests. Generates per-scope region assignments. Populates `cross_region_refs` for IncrefRegion emission. |
| Physical region allocator | PagePool + RegionPool + RegionStore. Per-region pages, dual-ended layout. `--region-page-size`, `--page-pool-max`. |
| Per-region RC | RegionStore tracks u32 RC per region. RC starts at 1 (the compiler's initial reference). `DecrefRegion` decrements; region freed when RC reaches 0. No deferred_free. |
| DecrefRegion | Dispatch uses `self.heap.free_region_physical()` which calls `decref()`. If RC > 0, decrements; frees when 0. Single region-demise bytecode — `FreeRegion` is gone. |
| IncrefRegion | Solver populates `cross_region_refs`; lowerer emits IncrefRegion at constraint sites via `emit_increfs_for` and for cross-region closure captures in `lower_lambda_expr`. |
| Auto-incref at alloc | `alloc_obj` scans HeapObject for cross-region Value refs and increfs each referenced region. Balances cascade decrefs at free time. |
| Bytecode handlers | Pair, MakeCapture, MakeClosure, MakeArrayMut use `vm.heap.alloc_in_region()` directly. No TLS. |
| populate_env | Capture cells, rest-arg cons, local cells use `heap.alloc_in_region()` (heap passed separately from fiber). |
| FiberHeap location | Lives on VM, not on Fiber. All fibers share one heap; isolation is per-region. TLS (`CURRENT_FIBER_HEAP`) points to this same VM-owned heap instance. |
| TLS alloc region | NativeFn calls and macro expansion only. `with_alloc_region!` / `with_transient_region!` macros. `get_alloc_region()` panics on 0. |
| Lambda body escape | Solver constrains body results to escape body region. |

## Remaining work

1. ~~**Wire mutable collection runtime RC.**~~ Done. `push`, `put`,
   `del`, `pop`, `rebox`, `insert`, `remove` on `@[]`, `@{}`, `box`,
   `@set` use `region_of()` (page header, O(1)) and incref/decref
   the referenced region. Cascade on free walks dtor and ref_objs
   lists to decref cross-region references.

2. ~~**Remove legacy slab region machinery.**~~ Done. Per-slot region
   arrays (region_ids, region_next, region_heads, region_bump_marks)
   and the old `free_region()` slab walk have been removed.

3. **Region merging.** Land the unmerged baseline first (every
   allocation in its own region; correct but slow), then enable the
   conservative merge condition (identical `free_at` and no
   cross-region edges incident to either) iteratively to fixpoint.
   Required for acceptable performance, but never for correctness.

## Invariants

These must hold for soundness:

1. **No freeing while RC > 0.** If a region has RC > 0, its pages
   must not be freed.

2. **RC accurately tracks cross-region references.** Every store of
   a value from region A into a data structure in region B must
   increment A's RC. Every removal must decrement. For immutable data,
   this is compile-time. For mutable collections, this is runtime.

3. **No commingling.** Objects from different regions must not share
   physical pages.

4. **Cascade is complete.** When a region is freed, all regions it
   references must be decremented. If any reach 0, they must be freed
   (recursively).

5. **The compiler's region assignments are sound.** If the solver says
   "allocate X in region ρ", then ρ must outlive all uses of X. A
   region freed too early is a use-after-free.

6. **Values are born in the right region.** The allocation instruction
   targets the solver-assigned region directly. Values are never
   allocated in a short-lived region and then "promoted" to a longer-
   lived one.

7. **Every allocation has a region.** There is no default, fallback,
   or "region 0." An allocation without a solver-assigned region is a
   bug that must panic.

## Validation

`tests/elle/leak.lisp` contains 14 tiers of allocation patterns.
Each measures allocation growth over 100 and 10000 iterations.
A pattern is "bounded" when the 10000-iteration count is not
significantly larger than the 100-iteration count.

When the system is working, every test in leak.lisp that is marked
`bounded?` must pass. Tests marked as "known leaks" represent patterns
that genuinely escape values — they are correct to leak.
