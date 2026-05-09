# DropSlot plan: challenged assumptions

## The pool.allocs problem

The plan says "DropSlot is O(1). dealloc_slot is O(1). No scanning."

This is wrong. Here's why.

`pool.allocs` is a `Vec<*mut HeapObject>` that tracks every live
allocation in insertion order. `release(mark)` truncates it to
`mark.allocs_len`, freeing every slot added after the mark. This is
the core of scope reclamation — O(1) bulk truncation.

DropSlot calls `slab.dealloc(ptr)` to return the slot to the free
list. But it does NOT remove the pointer from `pool.allocs`. The
pointer remains, marking a freed slot as "live."

What happens next:

1. The slab's free list now contains the slot.
2. A later `alloc()` reuses the slot, pushes the same pointer to
   `pool.allocs` again. Now `pool.allocs` has TWO entries for the
   same slot.
3. On fiber death, `teardown()` iterates `pool.dtors` and runs
   destructors. If the DropSlot'd object was in `pool.dtors`, its
   destructor runs again on a recycled slot. Double-free of inner
   data (Rc, Vec, etc.).
4. If a scope-mark `release()` fires that covers the DropSlot'd
   range, it iterates `allocs[mark..end]` and calls `slab.dealloc`
   for the stale pointer. The slot was already freed and possibly
   reused. Double-free.

This is the exact same bug that plagues `dealloc_ptrs` in the
pointer-snapshot approach. Fine-grained deallocation within a
bulk-truncation data structure is fundamentally incompatible.

**To make DropSlot correct, it must either:**

(a) Remove the pointer from `pool.allocs` and `pool.dtors`. This is
    an O(n) scan per DropSlot. For k dead bindings in a pool of n
    entries, that's O(k·n) — same cost as `dealloc_ptrs`.

(b) Change `pool.allocs` to a data structure that supports O(1)
    removal AND O(1) bulk truncation.

(c) Separate allocations into two categories: scope-managed (tracked
    in pool.allocs, freed by release) and drop-managed (NOT tracked
    in pool.allocs, freed by DropSlot only).

### Option (a): accept the O(n) scan

This concedes that DropSlot is NOT O(1). The compile-time analysis
is O(1) per binding, but the runtime execution is O(n) per drop.
The advantage over pointer-snapshots is: the set of things to drop
is determined at compile time, not by runtime scanning. The pool.allocs
scan is bookkeeping overhead, not a liveness decision.

This is honest but deflating. The plan promised no scanning.

### Option (b): change the data structure

Replace `pool.allocs: Vec<*mut HeapObject>` with a doubly-linked
intrusive list through the slab slots. Each slot stores prev/next
flat indices. `alloc()` appends to the list. `release(mark)` truncates
by unlinking from mark to tail. `DropSlot` unlinks a single node in
O(1).

Cost: 8 bytes per slot (two u32 indices). At 48+ bytes per HeapObject,
this is ~17% overhead. The slab's free list already uses intrusive
storage (the free-list link is written into dead slots' bytes), so
the pattern is established.

But `release(mark)` currently uses `mark.allocs_len` (an index into
the Vec) to truncate. With an intrusive list, the mark must store a
node pointer instead. And `RegionExitCall`'s drain-from-middle must
unlink a range of nodes, adjusting prev/next pointers for the
surviving nodes. This is doable but changes the ArenaMark structure.

### Option (c): two allocation categories

The compiler knows at allocation time whether a value will be
DropSlot'd: it's in a let scope whose body is a tail call, the
binding will be dead at the tail call, and the function is
rotation_safe.

A new allocation variant `alloc_untracked()` skips `pool.allocs` and
`pool.dtors`. DropSlot frees the slot directly. No stale pointers.

`release()` never sees untracked allocations. `teardown()` doesn't
know about them — but `slab.clear()` resets the entire slab regardless,
so they're cleaned up on fiber death.

Problem: the compiler must propagate "this allocation will be
untracked" through the bytecode to the runtime. A new bytecode
`AllocUntracked` or a flag on existing allocation instructions. The
slab doesn't need to change — only pool tracking changes.

Problem: what if a value that was supposed to be DropSlot'd escapes
(the analysis was wrong)? The value is in the slab but not in
pool.allocs. It won't be freed by any scope exit. It won't be freed
by teardown's dtor traversal. Its destructor never runs. Rc inner
data leaks.

Fix: `slab.clear()` could run destructors for ALL occupied slots, not
just those in pool.dtors. But the slab doesn't know which slots are
occupied and which are free-list entries (it uses intrusive free-list
storage, so "empty" slots contain link pointers, not dead HeapObjects).

This option is fragile. If the escape analysis is ever wrong, we get
silent destructor leaks (Rc, Arc, Box inner data never freed). The
current approach (track everything in pool.allocs/dtors, free via
release) is always correct even when escape analysis is wrong — it just
doesn't free *promptly*. Category (c) trades correctness for O(1).

## Scoping: RegionEnter without RegionExit

The plan says: "for scopes whose body IS a tail call, skip
RegionEnter." But this needs careful examination.

Without RegionEnter, there's no scope mark. Good — no orphaned mark
on the stack. But also: no call-scoped reclamation tracking for inner
calls, because RegionExitCall reads marks from the scope_marks stack.

Wait — that's wrong. Call-scoped reclamation pushes its OWN marks
(mark1 before args, mark2 before call) and pops them with
RegionExitCall. These are independent of the let scope's mark. They
work regardless of whether the let scope has RegionEnter.

So skipping RegionEnter for the let scope is fine. Inner call-scoped
reclamation still works. The let scope's bindings are freed by
DropSlot. Intermediates within inner calls are freed by
RegionExitCall.

What about intermediates that are NOT in a call-scoped region?
Specifically, discarded expressions in `begin` before a tail call:

```lisp
(begin
  {:x n}        ; heap alloc, result discarded
  (f (- n 1)))  ; tail call
```

The struct `{:x n}` is evaluated, its result is on the operand stack
briefly, then popped (discarded). The slab slot is allocated but
nobody holds a reference after the Pop. Without RegionExit, this slot
is never freed.

The fix: emit DropSlot for discarded heap-allocating expressions in
begin-before-tail-call. The lowerer already knows it's discarding the
expression (it emits Pop). Replace Pop with DropSlot for heap-
allocating expressions in tail position.

But this means EVERY discarded expression in a tail-position begin
gets a DropSlot. For non-heap expressions (immediates, void-returning
calls), DropSlot is a no-op (tag check). Acceptable.

## compute_return_params vs. local reference analysis

The plan cites compute_return_params for determining which params are
dead at the tail-call point. This is overkill.

compute_return_params is an interprocedural analysis: it computes a
bitmask of which function parameters transitively flow to the function's
return value, through nested calls, lambdas, and control flow. It
handles cases like "parameter flows through a closure capture into a
returned struct field."

DropSlot needs something simpler: "does this tail-call arg expression
reference this binding?" This is a local syntactic query — walk the
arg's HIR tree for Var nodes. No interprocedural reasoning. No
fixpoint. O(args × bindings_in_scope) at each tail-call site.

compute_return_params is still useful for its original purpose (scope
allocation analysis for call-scoped reclamation). DropSlot doesn't
need it and shouldn't depend on it.

## rotation_safe: per-function vs. per-binding

rotation_safe gates ALL DropSlot emissions for a function. If any
binding in the function body escapes to external mutable state, no
bindings get DropSlot'd.

This is correct but coarse. A function that does `(push acc x)` in
one branch is marked rotation_safe=false, even though other bindings
in other branches are perfectly safe to drop.

Per-binding analysis would ask: "does THIS specific binding's value
escape to external mutable state?" The existing walk_for_outward_set
analysis checks the entire body. A refinement would track WHICH
bindings' values flow into external stores.

This is a precision improvement, not a correctness issue. The
coarse-grained gate is conservative: it may miss DropSlot
opportunities, but it won't DropSlot a value that's still referenced.

For the initial implementation: keep per-function rotation_safe.
For future precision: per-binding escape tracking.

## SIG_FUEL and idempotence

If fuel runs out between two DropSlot instructions, the VM suspends.
On resume, it re-executes from the saved IP. If the first DropSlot
already freed a slot, the second execution would try to read a register
that was already cleared (if we null it) or free a stale pointer (if we
don't).

Fix: DropSlot must null the register after freeing. On replay, it
reads NIL (immediate) and is a no-op. This adds one write per
DropSlot.

## What should be REMOVED outright

These are dead or superseded by the DropSlot approach:

| Code | File | Reason |
|------|------|--------|
| `rotation_log: Vec<*mut HeapObject>` | fiberheap/mod.rs | Runtime tracking structure. Pushed to on every allocation. |
| `drain_rotation_log()` | fiberheap/mod.rs | Drains rotation_log for pointer-snapshot approach. |
| `dealloc_ptrs()` | fiberheap/mod.rs | O(n²) scan+free. The heart of the snapshot approach. |
| `RotationState` struct | vm/execute.rs | prev_ptrs, curr_ptrs, prev_safe. All runtime state. |
| `rotation.advance()` calls | vm/execute.rs, vm/mod.rs | Trampoline rotation invocations. |
| `rotation_safe` on `TailCallInfo` | vm/core.rs | Runtime doesn't need the flag; compiler already emitted DropSlots. |
| `rotation_safe` on `PendingTailCall` | vm/mod.rs | Same — the flag is consumed at compile time, not runtime. |
| `jit_prev_mark`, `jit_curr_mark` | fiberheap/mod.rs | JIT rotation marks. Replaced by JIT DropSlot emission (deferred). |
| `save/restore_jit_rotation_base` | fiberheap/mod.rs | JIT rotation save/restore. |
| `rotate_pools_jit()` | fiberheap/mod.rs | JIT mark/release rotation. |
| `release_between()` | fiberheap/mod.rs | Range-based release from the committed-but-abandoned approach. |
| `adjust_after_drain()` | arena.rs | Index adjustment for release_between. |
| `reset_alloc_count()` | fiberheap/mod.rs | Manual alloc_count reset (already removed in uncommitted). |
| `last_alloc_ptr()` | pool.rs | Only used by rotation_log.push(). |

## What needs REWORK

Existing mechanisms that must change for DropSlot to work:

### pool.allocs and pool.dtors (the hard problem)

The Vec-based tracking is incompatible with fine-grained deallocation.
Either:

**(b) Intrusive doubly-linked list** — replaces pool.allocs Vec.
Each slab slot gets prev/next u32 indices. O(1) insert, O(1) removal,
O(1) truncation-to-mark (if mark stores a node reference).

Rework scope: SlabPool, SlabMark, ArenaMark, release(), all code that
reads pool.allocs.len() or iterates pool.allocs. This is substantial
but localized to the fiberheap module.

OR

**(a) Accept O(n) removal** in DropSlot by scanning pool.allocs.
Rework: add `remove_from_allocs(ptr)` and `remove_from_dtors(ptr)` to
SlabPool. Called by DropSlot. Same cost as dealloc_ptrs but with
compile-time-determined targets.

The choice between (a) and (b) determines the performance ceiling.
Option (a) is simpler to implement but limits DropSlot's advantage
over pointer-snapshots. Option (b) is the right long-term design but
is a larger change.

### Lowerer tail-call path

Currently: evaluate args → emit TailCall.

Must become: evaluate args → compute dead set → emit DropSlots → emit
TailCall.

The dead set computation is new code but uses existing infrastructure:
- Scope binding tracking (already in the lowerer)
- HIR Var reference walking (straightforward)
- rotation_safe check (already computed)

### begin-in-tail-position lowering

Currently: evaluate each expression, Pop intermediate results.

Must become: evaluate each expression, DropSlot intermediate results
(instead of Pop) when the begin is in tail position.

This requires threading "are we in tail position?" through the begin
lowering path. The lowerer already knows this (it's how tail calls are
detected).

### Scope emission for let-with-tail-call-body

Currently: always emit RegionEnter/RegionExit if escape analysis
passes.

Must become: suppress RegionEnter/RegionExit when the let body is
a tail call. Emit DropSlot for dead bindings instead.

Rework: the `can_scope_allocate_let` decision must be augmented with
"is the body a tail call?" If so, skip scope marks, use DropSlot
instead.

### --checked-intrinsics parity

The `checked?` gate in leak.lisp must be removed. The %-prefixed
IMMEDIATE_PRIMITIVES fix (already on this branch) should make all
tests pass with --checked-intrinsics. Any remaining failures are
escape analysis gaps to fix.

## What is NOVEL contribution

New code that doesn't exist in the codebase today:

### DropSlot bytecode instruction

A new bytecode opcode. Semantics: read register, check heap tag,
run destructor if needed, return slab slot to free list, null the
register (for SIG_FUEL idempotence), update tracking.

~20 lines in dispatch.rs. The instruction itself is simple; the
correctness depends on the pool.allocs solution above.

### Dead-set computation at tail-call sites

A new analysis pass in the lowerer: at each TailCall emission point,
compute the set of in-scope bindings NOT referenced by any tail-call
arg. This is:

```rust
fn dead_at_tailcall(
    scope_bindings: &[Binding],
    tail_args: &[Hir],
) -> Vec<Binding> {
    let referenced: HashSet<Binding> = tail_args.iter()
        .flat_map(|arg| collect_var_refs(arg))
        .collect();
    scope_bindings.iter()
        .filter(|b| !referenced.contains(b))
        .copied()
        .collect()
}
```

~30 lines. Straightforward HIR walk. No fixpoint. No interprocedural
reasoning.

### DropSlot emission in the lowerer

For each dead binding at a tail-call site: load the binding into a
register (if not already loaded), emit DropSlot. For dead params:
load from capture, emit DropSlot.

~40 lines in the tail-call lowering path.

### DropSlot-instead-of-Pop for discarded expressions in tail begin

When lowering a `begin` form in tail position: for each non-tail
expression, emit DropSlot after evaluation instead of Pop (if the
expression may be heap-allocating).

~10 lines in begin lowering.

## Summary of the assessment

The DropSlot approach is fundamentally correct: compile-time liveness
determines what to free, explicit instructions free it, no runtime
scanning of liveness. This is Perceus applied to Elle.

The complication is pool.allocs — a data structure designed for bulk
truncation that resists fine-grained removal. This is an infrastructure
problem, not an analysis problem. The analysis is right; the runtime
bookkeeping needs to accommodate it.

Two paths forward:

**Path A (incremental):** implement DropSlot with O(n) pool.allocs
removal. Same asymptotic cost as the current pointer-snapshot approach,
but the liveness decisions are compile-time. Ship it, measure, then
consider the data structure change if the O(n) is measurable.

**Path B (structural):** replace pool.allocs Vec with an intrusive
linked list through slab slots. O(1) everything. Larger change but
eliminates the tension between scope reclamation and fine-grained
freeing permanently.

Path A has the virtue of being shippable quickly and provably correct
(same mechanism as dealloc_ptrs, just with compile-time target
selection). Path B is the right long-term design but should be
validated with benchmarks showing the pool.allocs scan is actually a
bottleneck.
