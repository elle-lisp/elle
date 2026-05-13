# Bug: Heap-Use-After-Free in HTTP Test During Fiber Abort

## Status: ROOT CAUSE CONFIRMED, FIX PENDING

## Summary

The http.lisp test crashes with a SEGV in `prim_get` → `as_struct` when
accessing a `:status` key on a freed immutable struct. The struct was allocated
on the child fiber's **outbox** pool. When the child `FiberHandle`'s `Rc`
dropped to 0 during the parent's scope exit, the child `Fiber` was dropped,
its `FiberHeap::drop` tore down the outbox, and the struct was freed while
the parent still held a `Value` pointing to it.

## Root Cause

### The crash sequence

```
do_fiber_abort
  → with_child_fiber {                        # parent runs as self.fiber
      resume_suspended                         # resume parent's suspended frames
        → dispatch loop                        # parent executes its bytecode
          → RegionExit                         # parent exits a scope
            → pop_scope_mark_and_release
              → release_refcounted             # free rc=0 objects in scope
                → drop_in_place(Fiber)         # child FiberHandle Rc hits 0
                  → Rc::drop_slow              # Rc deallocates the child Fiber
                    → FiberHeap::drop          # child's heap teardown
                      → outbox.teardown()      # frees outbox (5 objects)
                        → SlabPool::teardown   # frees the struct with :status

        → dispatch loop (continued)            # parent's NEXT instruction
          → prim_get response :status          # dereferences freed struct → SEGV
```

### The victim objects

The 5 objects freed by the outbox teardown are:
- 2 Closures
- 2 Fibers
- 1 LArray

These were on the child's outbox `SlabPool`, not the private pool or shared
allocator. The outbox is a `Box<SlabPool>` installed by the parent during
`with_child_fiber` for zero-copy yield exchange. It is owned by the child's
`FiberHeap` and torn down in `FiberHeap::drop`.

### Why the FiberHandle's Rc drops to 0

The branch added `StoreLocalRefcounted` (incref on store) and `DecrefLocal`
(decref before scope exit). The FiberHandle was stored via
`StoreLocalRefcounted` (rc goes to 1). `DecrefLocal` decrements it back to 0
before the scope exits. `release_refcounted` sees rc=0 and drops it.

### Why origin/main doesn't crash

Origin/main has the same worklist-based `release_refcounted` and the same
refcount infrastructure. The difference: origin/main does NOT emit `DecrefLocal`
for the FiberHandle (the `DecrefLocal` bytecode was added on this branch).
Without the premature decref, the FiberHandle stays at rc=1, the worklist
protects it, and it survives the scope exit. The child Fiber stays alive, the
outbox is not torn down, and the parent can safely access outbox values.

The child Fiber is eventually cleaned up when the parent's next scope mark
release or clear runs — at a point where the parent no longer references
outbox objects.

## Fix Plan

### Problem: `drop_in_place(Fiber)` during `release_refcounted` has cascading effects

When `release_refcounted` calls `drop_in_place` on a Fiber/FiberHandle
object, the Fiber's `Rc` deallocates the `Fiber`, which drops its
`Box<FiberHeap>`, which runs `FiberHeap::drop`, which tears down the outbox.
The outbox contains objects the PARENT is still using.

This is not a refcount accuracy issue — the FiberHandle genuinely has rc=0
because `DecrefLocal` ran. The issue is that dropping a Fiber inline during
scope cleanup has side effects that span beyond the scope.

### Approach: Defer Fiber drops out of `release_refcounted`

In `release_refcounted`, when the dtor pass encounters a `HeapObject::Fiber`
at rc=0, skip `drop_in_place` for it. Instead, collect these Fiber pointers
into a `deferred_drops` vec. After the scope exit is fully complete (dtors
compacted, slots freed), drop the deferred Fibers in a separate pass.

This is safe because:
- The Fiber's slab slot is NOT freed (it stays allocated, just at rc=0)
- The Fiber's dtor entry is compacted out of the dtor vec normally
- The Fiber object remains valid in memory until the deferred drop runs
- The deferred drop happens immediately after the scope exit, before control
  returns to the caller — no window where the Fiber "leaks"

The change is confined to `release_refcounted` in `mod.rs`. No compiler
changes, no refcount model changes, no new bytecodes.

### Implementation

1. In `release_refcounted`, during the dtor pass (Phase 2), check if the
   object is a `HeapObject::Fiber`. If so, skip `drop_in_place` and push the
   pointer to a `deferred_drops` vec instead.

2. After the dealloc loop completes, iterate `deferred_drops` and call
   `drop_in_place` on each. At this point the scope exit is done — no more
   Values from this scope are being accessed.

3. After `drop_in_place`, dealloc the slab slot (unlink + dealloc_slot).

### Why not fix the refcount or remove the rc system

**Fixing the refcount** would require ensuring that FiberHandles never reach
rc=0 while their outbox contents are still referenced. This means either:
- Not emitting `DecrefLocal` for FiberHandles (special-casing in the compiler)
- Adding extra increfs for outbox-dependent values (complex, error-prone)
Both approaches are fragile — the refcount model is subtle and adding
special cases invites future bugs of the same kind.

**Removing the rc system** would mean removing `StoreLocalRefcounted`,
`DecrefLocal`, `DropSlot` and going back to the worklist-only approach. But
the worklist already exists in `release_refcounted` and the refcount system
provides value elsewhere (while-loop rotation, mutable collection tracking).
Removing it is a large change with its own risk of regressions.

**Deferring Fiber drops** is the smallest, most targeted fix. It addresses
the specific issue (cascading teardown during scope cleanup) without
changing the refcount model or the compiler.

## What Has Been Eliminated

- **I/O buffer double-truncate**: benign
- **bytes_to_string_in_place corruption**: safe for I/O buffers
- **io_uring vs thread pool**: bug reproduces without io_uring
- **Approach B (Rc sharing)**: symptom of double-dropping the same slab slot
- **remove_from_dtors retain shifting**: not in the failing path
- **Three-phase dtor/dealloc ordering**: was a real bug (stale dtor entries)
  but not the cause of the SEGV; fixed by reverting to worklist approach
- **Second dtor pass after children decref**: segfaults because closure env
  references aren't tracked by slab refcounts
- **Deferred dealloc (skip slots with non-null dtors)**: prevents stale
  entries but the crash is caused by Fiber Drop cascade, not stale entries
- **Shared allocator teardown**: the freed objects were on the outbox, not
  the shared allocator. `SharedAllocator::teardown` was never called.

## Reproduction

```bash
cargo build --release && ./target/release/elle tests/elle/http.lisp  # SEGV
```

## Instrumentation Added (debug_assertions only)

All instrumentation is gated behind `#[cfg(debug_assertions)]`:

- **`release_refcounted`**: logs ENTER, PHASE1, each DROP/KEEP dtor, PHASE2
  summary, each FREE/PIN slot, DONE summary
- **`SlabPool::alloc`**: panics on duplicate dtor push
- **`SlabPool::dealloc_slot`**: panics on dealloc with non-null dtor entry
- **`SlabPool::teardown`**: logs dtors.len, alloc_count, full backtrace
- **`SlabPool::release`**: logs dtor_len, dtors.len, slot count
- **`SharedAllocator::teardown`**: logs pool.alloc_count, marks.len
- **`FiberHeap::Drop`**: cross-pool Rc sharing assertion before teardown
- **`drop_slot_value`**: logs dtor indices, scope marks, FREEING with flat/tag/rc
- **`push_scope_mark`**: validates no dtor duplicates at scope entry
- **`prim_get`** (access.rs): logs heap pointer and key for struct accesses

## Reference Documents

- `/dev/shm/memory-take-five.md` — memory system design doc
- `docs/io-completion-heap.md` — I/O completion heap routing spec
- `src/value/fiberheap/AGENTS.md` — FiberHeap design and invariants
- `src/vm/AGENTS.md` — Fiber swap protocol and shared allocator provisioning
