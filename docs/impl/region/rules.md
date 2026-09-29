# Region rules — the implementor's correctness obligations

<!-- audited: 2026-09-29 -->

The exhaustive correctness contract the compiler and runtime must uphold for
regions.

Read this file before you touch the region code. The RC-instruction mechanism these
rules constrain — value/slot resolution, coalescing, self-edge elimination, and
the equivalence oracle — is in [mechanism.md](mechanism.md).
For the consumer-facing model — how to write Elle that is sympathetic to the
memory system — see [docs/regions.md](../../regions.md) and the
[docs/regions/](../../regions/) series. The semantics (Tofte–Talpin for immutable
values, reference counting for mutation) and the single known leak are in
[regions/semantics.md](../../regions/semantics.md).

## There are exactly two measures: correct, then optimal

A region implementation is **correct** iff it never reads freed memory and
never frees live memory. That means no use-after-free, no double-free, no
dangling pointer, and (the dual we hold ourselves to) no value leaked past the
point its last reference dies. There is no third state and no spectrum. Code is not
"conservative" or "aggressive"; it is correct or it is broken.

Everything else — how many physical regions exist, how aggressively the solver
merges, peak RSS, mmap churn — is **optimization**. Optimization may never buy
performance with correctness. A program that runs with one region per value and
frees each precisely is correct and slow; that is the baseline we must always be
able to fall back to. A program that merges regions to run fast but frees one a
step too early is broken, full stop.

Corollary for how we work: prove the *idea* correct, and then implement it.
Then prove the *implementation* correct with a test written from the idea, not
from what the implementation happens to produce. Greening an individual failing test by
patching its symptom is how a codebase reaches "almost all tests pass" while
never being correct. The tests support the specification; they do not define it.

## The rules

These are exhaustive and each carries its exceptions inline. A violation of any
is a correctness defect, not a tuning knob.

1. **Every allocation has a region.** Region ids are nonzero, with no default
   fallback. An allocation the solver did not assign a region is an
   analysis gap and must panic at allocation — never silently leak. (No
   exceptions: a sentinel region is the bug, not the handling of it.)

2. **Every region corresponds to a real allocation** (the dual of Rule 1). A
   region the solver hands out must have an instruction that raises its RC, or
   its `DecrefRegion` underflows or aliases a neighbour. The test is operational
   — *does the lowerer emit an RC-raising instruction at this HirId?* — not
   syntactic. Exceptions, named here:
   - *Opaque `Call`/`Eval`*: the result is allocated in Rust (or a callee's
     own compilation), in a region the outer compilation did not create. The
     solver assigns a *placeholder* region, and the lowerer releases **by
     value** at the placeholder's `decref_point`. The release reads the actual
     returned value and decrefs the runtime region it lives in
     (`DecrefValueRegion`). It consumes the one owning reference the callee's
     return convention handed back (`IncrefValueRegion` at every `Return`).
     Two shapes:
       - **bound result** (a `let`/`def`/synthetic slot): load the slot,
         release, stamp the slot nil (the branch-result-loop discipline);
       - **discarded result** — the placeholder's `decref_point` is the call
         node itself, and no slot exists. ANF's propagating-tail wrap keys the
         slot on the outer `Let`, not the tail `Call`. Release the
         freshly-lowered result register directly. Skipping the release here
         leaks one object per iteration in loops
         ([tests/elle/arena-count.lisp](../../../tests/elle/arena-count.lisp)).
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
   The `decref_point` is the value's last-use program point. That is often a
   scope exit, but equally a loop back-edge, a tail-call boundary, or a return.
   A *consuming node* is itself a use of its operand's regions. `Return`
   extends a returned region to the Return node, and `Destructure` extends its
   value's regions to the Destructure node. The field extraction reads the
   value AFTER the value expression's own last read. So anchoring the release
   at the inner read frees the source under the extraction: the
   `&named`-param prologue UAF
   ([tests/elle/region-named-param-uaf.lisp](../../../tests/elle/region-named-param-uaf.lisp)).
   With every destructured binding unused, the collected keyword struct's only
   use was the prologue's Var. The lowerer freed it before `StructGetOrNil`
   read the fields.

   Its dual is a *transferring node*: a `Break` is **not** a use of its operand's
   regions. The value becomes the enclosing block's value, and control leaves
   the body before any release placed inside it runs. So the release is anchored
   where the *block's* value is consumed ([anchors.md](anchors.md)). When the
   target block is the function's **tail**, that anchor is the last point before
   the frame is handed back. So the broken value is also the *returned* value
   and takes the return mint. That includes a break through an enclosing
   `Loop`/`While`, which a `break` jumps past.

   The jump moves the anchor of every *other* region in the same window too. A
   release the break passes over is emitted into unreachable code. So a
   `decref_point` at or after a break site and inside its target block is
   re-anchored to the block's. That does not hold across a nested loop or
   lambda, where the release must keep running once per iteration / once per
   activation. Nor does it hold where a frame-replacing exit in the body means
   not every path reaches the block's own exit label.

   A third class is a *borrowing node*: an **uncounted** container element
   read (the `%get`/`%first`/`%rest` opcodes). It hands back a value that
   still lives **inside the container**: its own region for a pair's car, an
   interior member's for an `@array` element. It raises no count on that
   value, so the container's lifetime is the borrow's only protection. The
   container is used for as long as the read's RESULT is. Its regions extend
   to where that result is last used, not to the read.

   Anchored at the read, the container's free-time cascade drops the element's
   last count, and the reader derefs a freed page
   (`region_container_read_borrow_uaf`). The **native** `get`/`first`/`rest`
   call is not this class. Its dispatch takes the Rule 5 pass-through retain,
   so the reader holds its own reference and the container is free to die.
   What that retain cannot survive is adoption freezing the element's RC. The
   ownership cut handles that at admission ([adopt.md](adopt.md)).

   A *remove* is neither: `%pop` extracts the element out of the container (and
   out of its Owned subtree, `extract_owned_region`), so the container keeps its
   own last use. A `Match` arm's pattern binding is a borrowing read of the
   **scrutinee**. So the loop-containment test that every binder's scope node
   feeds decides where its release lands. A **rest** name is the one pattern
   name that is not a borrow. `[a b & r]` and `{:k v & r}` BUILD a fresh
   collection rather than reading one out, so the name owns it and releases it
   ([anchors.md](anchors.md)).

   It is *per activation*: each activation remaps its static region slots to
   fresh physical regions. So the same static `DecrefRegion` frees a different
   physical region each call. Exception, named: across a fiber
   **suspend/resume**, the activation's static→physical map
   (`activation_region_map`) is captured at suspend and restored at resume. So
   the resumed continuation's `DecrefRegion`s resolve to the *same* physical
   regions and still fire once. Re-running a `DecrefRegion` against a region a
   still-live binding holds is a double-free — the canonical fiber-resume
   defect.

   When several releases land on one `decref_point`, their emission order is a
   **topological sort of the region's holders-before-holdee edges**. The edges
   are the single-owner Owned-subtree forest (`owned_adopt_edges` ∪
   `capture_adopt_edges`, member → owner), plus every alias → source edge
   (`counted_read_aliases`, `opaque_result_aliases`,
   `funnel_result_containers`). So a store/capture-adopted **member** is
   released before the release that subtree-drops its **owner**. A member's own
   `DecrefRegion` is a no-op only while the member is still `Owned`. Once the
   owner's drop has reclaimed it, that decref faults, so the member must come
   first ([adopt.md](adopt.md)).

   The same holds for a value that may BE, or live inside, another: a
   borrowing read's result, an opaque call's result, a funnel's pass-through
   result. Each is released `DecrefValueRegion`-style, which resolves its
   runtime region by reading the value's own page. So it must read before the
   other's release can tear that page. The alias edges compose transitively
   through the same sort. That is what orders a read out of a CALL's result
   ahead of the region that frees the page. The read's recorded container is
   the call's placeholder, not that region.

   The forest gives each member exactly one owner, so the adopt half is acyclic
   and a topological order always exists. That includes *nested* subtrees
   (member ⊂ mid ⊂ root release innermost-first), which a flat members-first
   bucket could not order. A read edge is only a *may*-alias: a binding naming
   several alternatives makes two reads each other's container. So a cycle
   there is not an impossible state. It is broken by re-sorting the stalled
   regions on the adopt edges alone and then by tie-break, leaving the order
   deterministic.

   Regions no adopt edge relates are tie-broken by **page-read depth**, so a
   release that *reads* pages precedes a release that *frees* them. A
   value-gated `DecrefValueRegion` sorts first: it loads a slot and unwraps a
   capture cell to reach the inner value's region, the deepest read. Then
   comes a `DecrefCellRegion`, which reads the cell's page header via
   `region_of` and then frees the cell. Then comes a plain `DecrefRegion`,
   which frees and reads nothing. Region id breaks the final tie.

   Freeing a cell's pages before the init value's `DecrefValueRegion` unwraps
   it is the capture-cell over-release UAF. This Shared cell carries no adopt
   edge: RC balances its accounting, but the physical unwrap read must still
   precede the free. The order is deterministic across compiles — it never
   depends on hash-map iteration.

5. **RC tracks every cross-region reference — every escape increfs, every drop
   decrefs.** This is the whole soundness obligation, and it is only as sound as
   this list is complete. The escape sites, exhaustively:
   - *immutable contents* — `alloc_obj` scans the new object and increfs each
     region its fields point into;
   - *mutable store* — `push`/`put`/`add`/`%put`/`insert` incref the stored
     value's region; `pop`/`del`/`remove` decref it. This entry is **statically
     complete**. The raw `RefCell` accessors for the `Value`-bearing mutable
     containers (`@array`, `@struct`, `@set`, box, capture cell) are visible
     only inside `value/` (`as_*_cell`,
     [conversions.rs](../../../src/value/repr/accessors/conversions.rs)). So the
     only way code elsewhere can store into one is through the tracked funnels
     in [value/arena.rs](../../../src/value/arena.rs) (`push_with_incref` and
     friends). An uncounted container store is a compile error, not a review
     item. Read access goes through borrow-guard/copy-out accessors that cannot
     mutate. Membership-neutral mutation (in-place sort/reverse/shuffle — no
     value enters or leaves the container) is region-neutral. It gets its own
     funnel that grants mutable access without RC traffic. Residual channels,
     named: `HeapObject`'s fields are still `pub` for construction and the
     deep-copy machinery. (A direct field match could bypass the seam, so do
     not write one. The accessor channel is the one closed here.) An
     `External`'s `Rc<dyn Any>` payload is opaque to both scan and seam;
   - *native call result pass-through* — `first`/`rest`/`get` and friends return a
     value from another region. The call increfs it (a "new reference" in the
     CPython-C-API sense), and the caller's `DecrefValueRegion` consumes it;
   - *captured closure env* — the closure→env cross-region edge is increfed when
     the closure is built;
   - *borrowed tail-call argument* — a tail-call arg is pure-moved into the
     owned-param callee: the caller's dead post-`TailCall` release *is* the
     transfer. So an arg the frame does NOT own is handed one fresh owning
     reference, consumed by the callee's owned-param release. The transfer is
     what lets that block's deadness carry weight, and it does so for
     **arguments only**. A release landing there for anything the call does
     not name has no such story. It is carried back ahead of the `TailCall`
     ([relocate.md](relocate.md)). Two borrow routes: a captured upvalue (owned
     by the closure env's capture-incref), and a compile-time-constant heap value
     (`immutable_values`). That value is a stdlib export closure or a
     `begin-for-syntax` value, owned by the env that seeded it. It is never
     captured, so the frame holds no reference at all. `tail_arg_is_borrowed`,
     [src/lir/lower/control.rs](../../../src/lir/lower/control.rs).
     The move is **one reference per occurrence, not one per call**. The frame
     holds a single reference to a region, while the callee's owned-param
     releases fire once per parameter. So an argument list naming the same
     region twice hands over one reference against two releases.
     `(concat-seq a rest a false)` does this, and so do two aliased bindings.
     The second release reaches zero under a value the caller is still using.
     Only the FIRST owned occurrence is funded by the move; every later one is
     minted exactly as a borrowed argument is. Repetition is read over the
     arguments' value-producing leaves and the regions they may name, not over
     syntax. That is because two distinct bindings can name one region
     ([region-tail-repeated-arg-uaf.lisp](../../../tests/integration/fixtures/region-tail-repeated-arg-uaf.lisp));
   - *reassigned mutable binding cell* — a reassigned binding is a 1-slot
     mutable container (see [bindings.md](bindings.md)). The store increfs the
     new content's region, and the overwrite decrefs the displaced content's
     region. A binding read out of a reassigned **captured** cell takes a
     counted reference: incref at the bind, value-based release at the reader's
     last use. The cell's overwrite-release cannot see uncounted holders;
   - *suspended frame* — a heap-promoted activation record holds cross-region
     refs (captured env, saved operand stack) and owns its
     `activation_region_map`. These are RC roots, increfed at suspend. They are
     released at resume-consume **and** at squelch/abort discard (an unbalanced
     discard underflows);
   - *sent channel message* — `chan/send` increfs the message's region after a
     successful enqueue (`EscapeSite::ChanSend`). The channel buffer is
     external to the region system, so this retain is the message's only
     reference while it rides the buffer. Each receive
     (`chan/recv`/`chan/try-select`/`chan/wait-ready`) decrefs it as the
     message leaves (`release_received_message`);
   - *submitted I/O operand* — the port, buffer, payload, handle or external a
     pending I/O operation names is increfed when its entry is filed
     (`EscapeSite::IoSubmit`). The backend's pending table is external to the
     region system in the same way a channel buffer is. So this retain is the
     operand's reference while the operation is in flight, and disposing of
     the entry decrefs it (`OperandHold`,
     [io-inflight.md](../io-inflight.md));
   - *retained process root* — a value a host keeps reading past the run that
     produced it. It is registered as a process root while the host holds no
     owning reference to hand over (`EscapeSite::ProcessRoot`). The registry
     is external to the region system in the way a channel buffer is. So this
     retain is the root's reference, and the teardown sweep's decref lowers it;
   - *terminal fiber signal* — a child's set-once return/error/halt result, read
     later via `fiber/value`, is park-retained when the fiber goes terminal and
     released by the signal scan when the fiber is freed.
   Every entry has a matching decrement. Missing an escape site is a
   use-after-free; missing the matching decrement is a leak. The list being
   complete *is* correctness for the RC half.

   A reference from a region to itself is not a cross-region reference. No
   entry counts it: not the allocation scan, not the free cascade, and not
   either half of the mutable-store funnel. A value stored into a container
   that lives in the value's own region raises no count, and removing it lowers
   none. Colocation depends on this symmetry
   ([colocation.md](colocation.md)).

   **The fresh-frame invariant the releases lean on.** A value-based release
   reads its local slot unconditionally. A branch-arm-bound temp is
   NIL-initialized only inside its own arm, yet its scope-end
   `LoadLocal slot; DecrefValueRegion` runs on every path. That is sound only
   because an unwritten slot reads NIL (an immediate the release no-ops on).
   Every activation entry must deliver that. A fresh call does by construction
   (an empty per-activation stack; the prologue's bare-NIL pushes land at the
   slot indices).

   A frame-replacing tail call reuses the caller's operand stack. So the
   trampoline truncates it to the frame base before installing the callee
   (`trampoline_loop`, [src/vm/execute.rs](../../../src/vm/execute.rs)). The
   caller's locals are dead there: every owned value was released at its last
   use or moved into the callee. Any slot left un-truncated would surface as
   the callee's stale read. That turns a scope-end release into an over-free
   of a region the frame owns no reference to. Pinned by
   `runtime::tests::ownership::frame`.

6. **No commingling.** Objects from different regions never share a page —
   otherwise freeing one region cannot munmap its pages while another's objects
   sit on them. (No exceptions.)

7. **The cascade is complete.** Freeing a region decrements every region its
   contents reference. Immutable contents cascade via compiler-emitted decrefs;
   mutable contents cascade via a bounded walk of the container at free time. A
   scan-at-alloc must be symmetric with the scan-at-free — only valid for
   immutable contents. Exception, named: the terminal-signal retain (Rule 5) is
   asymmetric by design. There is no incref at fiber allocation (the signal is
   `None` then). The park-retain supplies the incref, and the free-time signal
   scan supplies the decref. This is balanced only because it is scoped to a
   set-once terminal value.

8. **No leaks.** A heap value whose last reference is dropped is freed at that
   point. The *only* values permitted to outlive the program are true
   process-lifetime roots: the symbol table and imported shared objects. They
   are held for the process by a real reference; those are roots, not leaks. (Native-fns
   also outlive the program, but they are immediate `&'static` `prim_id` values
   that occupy no region at all — there is nothing to leak.) The test for a root
   is **allocated exactly once per process**: a value re-allocated on every
   `(eval …)` or module load is not a root no matter how "compile-time" it looks.
   A scope that drops a value without freeing it is a defect, including the
   mutable-cycle case of the [theory](../../regions/semantics.md). We tolerate
   that case only because it is currently the sole known incompleteness, not
   because leaking is ever correct.

## Soundness checklist

The rules above, as the list to verify against any change:

1. Every allocation has a region (no region 0).
2. Every region has a real allocation (opaque calls use value-gated release).
3. Values are born in their final region (no promotion).
4. `DecrefRegion` fires once per activation, at the point of demise (the
   `activation_region_map` preserves this across resume).
5. Every cross-region escape increfs and every drop decrefs (the escape-site list
   is complete), and a reference within one region is counted on neither side.
6. No two regions share a page.
7. The free cascade is complete and symmetric with alloc-time scanning.
8. Nothing leaks but true process-lifetime roots.

## Teardown — every region frees

The naive user model is `elle foo.lisp` ≡ `(eval (wrap-in-letrec (read-all
(slurp "foo.lisp"))))`. After that `eval` returns and its result is dropped, the
world is back to its pre-`main` state: **every** region the process created is
freed. The only things that persist are true process-lifetime roots and the
native-fn primitives, which are immediate `&'static` values occupying no region.
Even the stdlib, prelude, core env, and trait tables are torn down before the
process exits — they are resident *roots*, not eternal.

One contract drives every entry path: running a file, graceful REPL exit, the
embedding API, and the lint path. The lint path uses one runtime per call. The
resident LSP VM is the deliberate exception, one long-lived runtime for the
server's life. All run through a single `Runtime`
([src/runtime.rs](../../../src/runtime.rs)). `Runtime::new` installs the heap,
registers primitives, and optionally loads the stdlib. It records the
process-resident roots in the process-root registry. `Runtime`'s `Drop` (or an
explicit `Runtime::teardown`) runs the sweep. One teardown routine, so the paths
cannot drift.

Three non-negotiable properties:

1. **RC-driven, never iterate-and-free.** The sweep releases the *roots* —
   decrefs each registered process-root region exactly once — and lets the
   ordinary RC cascade (Rules 5 and 7) reclaim everything reachable. It never
   walks the region table freeing entries. Force-freeing live regions would mask
   the very leaks and missing-decref defects this contract exists to surface:
   freeing-by-iteration always "succeeds" and proves nothing; freeing-by-RC
   succeeds only when the accounting is correct.

2. **Observable, and zero.** The sweep reports the live region census afterward
   (`Runtime::teardown` returns it; `--stats` prints it). **Zero** is the claim
   [tests/region_process_teardown](../../../tests/region_process_teardown.rs)
   gates — not a target the number is allowed to approach. A residue is the
   standing list of open leaks: the number *is* the remaining work, not a
   tuning knob. [tests/elle/oracle.lisp](../../../tests/elle/oracle.lisp)
   measures the same property as a per-op leak rate while a program runs. This
   counts what survives the process. That is the axis that sees a leak whose
   rate is one per PROGRAM rather than one per op.

   Reading the count as a target is what let the last one stand. A single
   reference cycle — the async scheduler's closures, their forward cells, and the
   fiber its tables held — held 100% of the residue of every program ever run.
   Nothing failed on it (elle-lisp/elle#1081).

3. **No unexplained references.** A surviving region's RC is explained by the
   in-edges other survivors point at it, and by nothing else. The remainder is
   RC minus that in-degree, the quantity the macro scope balances
   ([macroscope.md](macroscope.md)). It is a claim held outside the region
   graph, so no release the region system can reach ever frees it. Zero regions
   carrying one is the claim, and
   [tests/region_process_teardown](../../../tests/region_process_teardown.rs)
   gates it.

   The residue count and this one measure different defects. A reference cycle
   keeps its members alive with every reference explained, so the residue stays
   positive while this count is zero. An unexplained reference pins its region
   and everything that region reaches, whatever the rest of the graph does, and
   it survives every fix to the graph.

Because the sweep is RC-driven, the residue equals the set of regions whose RC
never reached zero — the true leaks — rather than being hidden by a blanket free.
As the leaks are fixed the residue falls to zero with no change to the teardown
itself.

### The program value is the host's to release

A run answers with the value of its last form, and the return convention hands
that value to the host with one owning reference (Rule 5, `ReturnValue`). The
host holds a `Copy` `Value`, so dropping it releases nothing. The reference is
the host's to give back, and a host that keeps it leaves the value's region —
and every region that one reaches — standing at teardown.

There are two ways to give it back, and a host picks by how long it reads the
value:

- **Release it now**, with `release_program_value`. The run is over and nothing
  reads the value again. The `elle` binary does this for the value of a file, a
  `--eval:` expression, and stdin.
- **Register it as a process root**, with `register_process_root` and
  `RootRef::Take`. The host reads the value for as long as the runtime lives,
  and the sweep releases it. An embedded host that keeps the value does this.

A host that does neither measures the residue of its own hand-off rather than
the run's. That residue is one region per run, not one per call, so no leak
*rate* ever sees it — the teardown census is the only instrument that does.

A registration takes one owning reference, so a host that registers a value it
holds no reference to must mint one first. `RootRef::Take` hands over the
reference the caller holds; `RootRef::Mint` raises the count, so the root
outlives whatever release the caller still owes. A registration that takes a
reference nobody holds is an over-free, and it stands only while the value that
holds the registered part leaks.

The REPL is the host that needs both halves. It prints each form's value and
releases it, and every binding it keeps mints a reference of its own. That
covers each leaf of a destructuring `def`: the leaves belong to the trailing
tuple, and that tuple is the program value the REPL releases.
