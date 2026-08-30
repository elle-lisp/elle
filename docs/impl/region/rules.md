# Region rules — the implementor's correctness obligations

This is implementation-facing: the exhaustive correctness contract the
compiler and runtime must uphold. Read it before touching the region code. The RC-instruction mechanism these
rules constrain — value/slot resolution, coalescing, self-edge elimination, and
the equivalence oracle — is in [mechanism.md](mechanism.md).
For the consumer-facing model — how to write Elle that is sympathetic to the
memory system — see [docs/regions.md](../../regions.md) and the
[docs/regions/](../../regions/) series; the semantics (Tofte–Talpin for immutable
values, reference counting for mutation) and the single known leak are in
[regions/semantics.md](../../regions/semantics.md).

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
   tests/elle/region-named-param-uaf.lisp). Its dual is a *transferring node*:
   a `Break` is **not** a use of its operand's regions — the value becomes the
   enclosing block's value, and control leaves the body before any release
   placed inside it runs, so the release is anchored where the *block's* value
   is consumed ([mechanism.md](mechanism.md) § "`break` transfers its value").
   When the target block is the function's **tail**, that anchor is the last
   point before the frame is handed back, so the broken value is also the
   *returned* value and takes the return mint — including through an enclosing
   `Loop`/`While`, which a `break` jumps past (mechanism.md § "A break out of a
   TAIL block carries the return mint"). The jump moves the anchor of every
   *other* region in the same window too: a release the break passes over is
   emitted into unreachable code, so a `decref_point` at or after a break site
   and inside its target block is re-anchored to the block's — except across a
   nested loop or lambda, where the release must keep running once per iteration
   / once per activation, and except where a frame-replacing exit in the body
   means the block's own exit label is not a point every path reaches
   (mechanism.md § "A release the break jumps over is not a release").
   A third class is a *borrowing node*: an **uncounted** container element read —
   the `%get`/`%first`/`%rest` opcodes — hands back a value that still lives
   **inside the container** (its own region for a pair's car, an interior member's
   for an `@array` element) and raises no count on it, so the container's lifetime
   is the borrow's only protection: the container is used for as long as the read's
   RESULT is, and its regions extend to where that result is last used, not to the
   read. Anchored at the read, the container's free-time cascade drops the element's
   last count and the reader derefs a freed page
   (`region_container_read_borrow_uaf`). The **native** `get`/`first`/`rest` call is
   not this class: its dispatch takes the Rule 5 pass-through retain, so the reader
   holds its own reference and the container is free to die — what that retain
   cannot survive is adoption freezing the element's RC, which the ownership cut
   handles at admission (adopt.md § "The lifetime obligation the root carries"). A
   *remove* is neither: `%pop` extracts the element out of the container (and out of
   its Owned subtree, `extract_owned_region`), so the container keeps its own last
   use. A `Match` arm's pattern binding is a borrowing read of the **scrutinee**, so
   where its release lands is decided by the loop-containment test every binder's
   scope node feeds (mechanism.md § "Every binder records its scope").
   It is *per
   activation*: each activation remaps its static region slots to fresh physical
   regions, so the same static `DecrefRegion` frees a different physical region
   each call. Exception, named: across a fiber **suspend/resume** the activation's
   static→physical map (`activation_region_map`) is captured at suspend and restored at
   resume, so the resumed continuation's `DecrefRegion`s resolve to the *same*
   physical regions and still fire once. Re-running a `DecrefRegion` against a
   region a still-live binding holds is a double-free — the canonical fiber-resume
   defect.

   When several releases land on one `decref_point`, their emission order is a
   **topological sort of the region's holders-before-holdee edges** — the
   single-owner Owned-subtree forest (`owned_adopt_edges` ∪ `capture_adopt_edges`,
   member → owner) plus every alias → source edge (`counted_read_aliases`,
   `opaque_result_aliases`, `funnel_result_containers`) — so a store/capture-adopted
   **member** is released
   before the release
   that subtree-drops its **owner**. A member's own `DecrefRegion` is a no-op only
   while the member is still `Owned`; once the owner's drop has reclaimed it that
   decref faults, so the member must come first (adopt.md § "The lifetime
   obligation the root carries"). The same holds for a value that may BE, or live
   inside, another — a borrowing read's result, an opaque call's result, a funnel's
   pass-through result: each is released `DecrefValueRegion`-style, which resolves
   its runtime region by reading the value's own page, so it must read before the
   other's release can tear that page. The alias edges compose transitively through
   the same sort, which is what orders a read out of a CALL's result — whose
   recorded container is the call's placeholder, not the region that frees the page
   — ahead of that region. The forest gives each member exactly one owner, so the adopt half
   is acyclic and a topological order always exists — including
   for *nested* subtrees (member ⊂ mid ⊂ root release innermost-first), which a
   flat members-first bucket could not order. A read edge is only a *may*-alias (a
   binding naming several alternatives makes two reads each other's container), so a
   cycle there is not an impossible state: it is broken by re-sorting the stalled
   regions on the adopt edges alone and then by tie-break, leaving the order
   deterministic.

   Regions no adopt edge relates are tie-broken by **page-read depth** so a
   release that *reads* pages precedes a release that *frees* them: a value-gated
   `DecrefValueRegion` (loads a slot and unwraps a capture cell to reach the inner
   value's region — the deepest read) sorts first, then a `DecrefCellRegion`
   (reads the cell's page header via `region_of`, then frees the cell), then a
   plain `DecrefRegion` (frees, reads nothing); region id breaks the final tie.
   Freeing a cell's pages before the init value's `DecrefValueRegion` unwraps it is
   the capture-cell over-release UAF (this Shared cell carries no adopt edge — RC
   balances its accounting, but the physical unwrap read must still precede the
   free). The order is deterministic across compiles — it never depends on
   hash-map iteration.

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
   - *borrowed tail-call argument* — a tail-call arg is pure-moved into the
     owned-param callee (the caller's dead post-`TailCall` release *is* the
     transfer), so an arg the frame does NOT own is handed one fresh owning
     reference, consumed by the callee's owned-param release. The transfer is
     what makes that block's deadness load-bearing for **arguments only**: a
     release landing there for anything the call does not name has no such
     story and is carried back ahead of the `TailCall`
     ([mechanism.md](mechanism.md) § "A release past a frame-replacing tail
     call is not a release"). Two borrow
     routes: a captured upvalue (owned by the closure env's capture-incref)
     and a compile-time-constant heap value (`immutable_values` — a stdlib
     export closure, a `begin-for-syntax` value — owned by the env that
     seeded it; never captured, so the frame holds no reference at all).
     `tail_arg_is_borrowed`, src/lir/lower/control.rs.
     The move is **one reference per occurrence, not one per call**. The frame
     holds a single reference to a region, while the callee's owned-param
     releases fire once per parameter, so an argument list naming the same
     region twice — `(concat-seq a rest a false)`, or two aliased bindings —
     hands over one reference against two releases, and the second reaches
     zero under a value the caller is still using. Only the FIRST owned
     occurrence is funded by the move; every later one is minted exactly as a
     borrowed argument is. Repetition is read over the arguments'
     value-producing leaves and the regions they may name, not over syntax,
     because two distinct bindings can name one region
     (`region-tail-repeated-arg-uaf.lisp`);
   - *reassigned mutable binding cell* — a reassigned binding is a 1-slot
     mutable container (see
     [bindings.md](bindings.md)): the store increfs the new
     content's region, the overwrite decrefs the displaced content's region,
     and a binding read out of a reassigned **captured** cell takes a counted
     reference (incref at the bind, value-based release at the reader's last
     use) — the cell's overwrite-release cannot see uncounted holders;
   - *suspended frame* — a heap-promoted activation record holds cross-region refs
     (captured env, saved operand stack) and owns its `activation_region_map`; these are RC
     roots, increfed at suspend and released at resume-consume **and** at
     squelch/abort discard (an unbalanced discard underflows);
   - *sent channel message* — `chan/send` increfs the message's region after a
     successful enqueue (`EscapeSite::ChanSend`); the channel buffer is external
     to the region system, so this retain is the message's only reference while
     it rides the buffer, and each receive (`chan/recv`/`chan/try-select`/
     `chan/wait-ready`) decrefs it as the message leaves
     (`release_received_message`);
   - *submitted I/O operand* — the port, buffer, payload, handle or external a
     pending I/O operation names is increfed when its entry is filed
     (`EscapeSite::IoSubmit`); the backend's pending table is external to the
     region system in the same way a channel buffer is, so this retain is the
     operand's reference while the operation is in flight, and disposing of the
     entry decrefs it (`OperandHold`, src/io/AGENTS.md § "A submitted operation
     holds the values its completion reads");
   - *terminal fiber signal* — a child's set-once return/error/halt result, read
     later via `fiber/value`, is park-retained when the fiber goes terminal and
     released by the signal scan when the fiber is freed.
   Every entry has a matching decrement. Missing an escape site is a
   use-after-free; missing the matching decrement is a leak. The list being
   complete *is* correctness for the RC half.

   **The fresh-frame invariant the releases lean on.** A value-based release
   reads its local slot unconditionally — a branch-arm-bound temp is
   NIL-initialized only inside its own arm, yet its scope-end
   `LoadLocal slot; DecrefValueRegion` runs on every path — sound only because
   an unwritten slot reads NIL (an immediate the release no-ops on). Every
   activation entry must deliver that. A fresh call does by construction (an
   empty per-activation stack; the prologue's bare-NIL pushes land at the slot
   indices). A frame-replacing tail call reuses the caller's operand stack, so
   the trampoline truncates it to the frame base before installing the callee
   (`trampoline_loop`, src/vm/execute.rs): the caller's locals are dead there —
   every owned value was released at its last use or moved into the callee —
   and any slot left un-truncated would surface as the callee's stale read,
   turning a scope-end release into an over-free of a region the frame owns no
   reference to. Pinned by `runtime::tests::ownership::frame`.

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
   mutable-cycle case of the [theory](../../regions/semantics.md) (which we tolerate
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
   regions remaining. A non-zero residue is the standing list of open leaks: the
   number *is* the remaining work, not a tuning knob. `tests/elle/oracle.lisp`
   measures the same property as a per-op leak rate while a program runs;
   `tests/region_process_teardown` counts what survives the process.

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
