# Bug: Heap-Use-After-Free in HTTP Test During Fiber Abort

## Status: ROOT CAUSE CONFIRMED, FIX PENDING

## Summary

The http.lisp test crashes with a SEGV in `prim_get` → `as_struct` when
accessing a `:status` key on a freed immutable struct. The struct was allocated
on a shared allocator owned by a child fiber. When the child fiber's
`FiberHandle` Rc dropped to 0 during a scope exit, the Fiber was dropped,
its `FiberHeap` was torn down (including the shared allocator), and the struct
was freed while the parent still held a `Value` pointing to it.

## Root Cause

### The crash sequence

```
do_fiber_abort
  → with_child_fiber {                        # swap: parent runs as self
      resume_suspended                         # resume parent's suspended frames
        → dispatch loop                        # parent executes its bytecode
          → RegionExit                         # parent exits a scope
            → pop_scope_mark_and_release
              → release_refcounted             # free rc=0 objects in scope
                → drop_in_place(Fiber)         # FiberHandle Rc hits 0
                  → Rc::drop_slow              # Rc deallocates the Fiber
                    → FiberHeap::drop          # child's heap teardown
                      → teardown shared alloc  # frees shared allocator objects
                        → SlabPool::teardown   # frees the struct with :status

        → dispatch loop (continued)            # parent's NEXT instruction
          → prim_get response :status          # dereferences freed struct → SEGV
```

The struct at the crash address was allocated on the child's shared allocator
pool. The shared allocator is a `SlabPool` inside a `SharedAllocator` owned by
the parent's `FiberHeap.owned_shared`. When the child `FiberHandle` (an
`Rc<RefCell<Option<Fiber>>>`) is dropped, the `Rc` deallocates the `Fiber`,
which drops its `Box<FiberHeap>`, which calls `FiberHeap::drop()`, which tears
down owned shared allocators. The 5 objects on the shared allocator are freed.

But the parent is still executing — its suspended frames hold `Value`s pointing
to those objects. The next instruction after the scope exit accesses the freed
struct.

### Why the FiberHandle's Rc drops to 0

The branch introduced `StoreLocalRefcounted`, `DropSlot`, and `DecrefLocal`
bytecodes that changed the refcount model. The FiberHandle was stored in a
local variable via `StoreLocalRefcounted` (incref to 1). When the scope exits,
the `release_refcounted` worklist sees the FiberHandle at rc=1 (pinned), but
it's a child Fiber that the parent is aborting. The parent no longer needs
the Fiber — the abort path already extracted the error value. But the
FiberHandle's incref from `StoreLocalRefcounted` keeps it alive until the
scope exits. When the scope exits, the worklist doesn't protect the Fiber
(it has no children that are also scope allocs), and it ends up at rc=0 after
some decref path, so it gets dropped.

### Why origin/main doesn't crash

Origin/main's `release_refcounted` uses the same worklist approach. The
difference is the branch's refcounting changes:
- New `StoreLocalRefcounted` bytecode increfs values stored in local bindings
- New `DecrefLocal` bytecode decrefs before scope exit
- New `DropSlot` bytecode decrefs and frees immediately

These changed WHEN and HOW FiberHandle refcounts are managed. At origin/main,
the FiberHandle either had rc>0 throughout the scope or was never stored via
`StoreLocalRefcounted`. On the branch, the decref path brings it to rc=0
during the scope exit, triggering the drop cascade.

### Previous (incorrect) hypothesis: three-phase dtor/dealloc ordering

The original hypothesis was that `release_refcounted`'s three-phase approach
(dtor loop → children decref → dealloc) caused the bug because objects skipped
in the dtor loop (rc > 0) could reach rc=0 after children decref, and the
dealloc loop would free them while dtor entries survived. This was a REAL bug
(slots were freed with stale dtor entries), but fixing it did not resolve the
crash because the actual crash has a different root cause (Fiber Drop cascade).

The three-phase code was replaced with the origin/main worklist approach
(adapted to the linked-list data structures), which is correct and does not
have the stale-dtor-entry problem.

## What Has Been Eliminated

- **I/O buffer double-truncate**: benign — first truncate sets length,
  second adjusts for newline position
- **bytes_to_string_in_place corruption**: safe for I/O buffers
  (traits=NIL, no Rc sharing)
- **io_uring vs thread pool**: bug reproduces without io_uring
- **Simple TCP echo test**: passes with ASAN — only leaks, no UAF.
  Bug requires the full HTTP module's fiber abort pattern
- **Approach B (Rc sharing)**: the duplicate RcInner drops were a symptom
  of double-dropping the same slab slot, not two slots sharing an Rc
- **remove_from_dtors retain shifting scope marks**: not the cause;
  `drop_slot_value` was not in the failing path
- **Three-phase dtor/dealloc ordering**: was a real bug (stale dtor entries)
  but not the cause of the SEGV; fixed by reverting to worklist approach
- **Second dtor pass after children decref**: segfaults because it drops
  objects still referenced by pinned closures (closure envs are not tracked
  by slab refcounts)
- **Deferred dealloc (skip slots with non-null dtors)**: prevents stale
  entries but the crash happens because the Fiber Drop cascade frees objects
  on a different pool entirely (shared allocator)

## Reproduction

```bash
cargo build --release && ./target/release/elle tests/elle/http.lisp  # SEGV
```

ASAN trace:
```
SEGV on unknown address in <Value>::as_struct (accessors.rs:376)
  prim_get (access.rs:265)
  call_inner → handle_call → dispatch → trampoline_loop
  execute_bytecode_from_ip → resume_suspended
  do_fiber_abort::{closure#1} → with_child_fiber
  do_fiber_abort → handle_fiber_abort_signal_jit
```

## Instrumentation Added (debug_assertions only)

All instrumentation is gated behind `#[cfg(debug_assertions)]`:

- **`release_refcounted`**: logs ENTER (mark, dtor_len, n_allocs, n_pinned,
  scope_depth), PHASE1 (children protected via worklist), each DROP/KEEP dtor
  entry with tag/flat/rc, PHASE2 summary (dtors run, kept), each FREE/PIN
  slot with tag/flat/rc, DONE summary (slots freed, pinned)
- **`SlabPool::alloc`**: panics on duplicate dtor push
- **`SlabPool::dealloc_slot`**: panics on dealloc with non-null dtor entry
- **`SlabPool::teardown`**: logs dtors.len, alloc_count, and full backtrace
  to identify which teardown frees the victim object
- **`SlabPool::release`**: logs dtor_len, dtors.len, number of slots freed
- **`SharedAllocator::teardown`**: logs pool.alloc_count, marks.len
- **`FiberHeap::Drop`**: cross-pool Rc sharing assertion BEFORE any teardown;
  checks shared allocator pools; full backtrace would show Fiber drop cascade
- **`drop_slot_value`**: logs dtor indices and scope mark dtor_lens before
  `remove_from_dtors`; logs FREEING with flat/tag/rc
- **`push_scope_mark`**: validates no duplicates in dtors at scope entry
- **`prim_get`** (access.rs): logs heap pointer and key for struct accesses

## Fix Options

### Option A: Don't drop the Fiber during release_refcounted

Prevent `drop_in_place` from running on Fiber/FiberHandle objects during
`release_refcounted`. Instead, queue them for deferred drop after the scope
exit completes and the parent has finished using the shared allocator contents.

Pros: Minimal change; isolates the fix to `release_refcounted`.
Cons: Fibers still need to be dropped eventually; deferred drops accumulate
if the FiberHandle Rc is shared.

### Option B: Detach shared allocator before Fiber Drop

When a child fiber's FiberHandle Rc drops to 0, before dropping the Fiber,
unlink the shared allocator from the child's FiberHeap so it survives the
drop. The shared allocator is already owned by the PARENT's `owned_shared`,
so the child's FiberHeap shouldn't be tearing it down at all.

Wait — the backtrace shows `FiberHeap::drop` at line 1744, which tears
down the outbox, not `owned_shared`. The child's `owned_shared` is empty
(the parent owns the shared allocator). The 5 freed objects were on the
child's **outbox** pool — a `Box<SlabPool>` installed by the parent for
yield-bound allocations. When the child Fiber's Rc drops to 0, the child's
FiberHeap tears down the outbox, freeing the struct that the parent still
references. This is an outbox lifecycle bug, not a shared allocator bug.

If the child's `owned_shared` IS empty, then the 5-object teardown comes
from the child's outbox. The outbox is a `Box<SlabPool>` installed by the
parent during `with_child_fiber`. Values yielded to the parent are allocated
on this outbox so they survive the child's private pool. But the outbox is
owned by the child's FiberHeap and torn down when the child dies — which
is too early if the parent hasn't finished using the values.

### Option C: Deep-copy values during fiber abort

During `do_fiber_abort`, before resuming the parent's suspended frames,
deep-copy all values that might reference the child's pools. The outbox
mechanism already does this for yielded values, but the abort path doesn't
go through the outbox.

Pros: Defensive; catches all cases regardless of refcount state.
Cons: Expensive (deep copy on every abort); may not be needed if the real
issue is that values escape to the parent without going through the outbox.

### Option D: Fix the refcount so the Fiber stays alive

Ensure the FiberHandle's slab refcount stays >0 until the parent is done
with the shared allocator contents. This would be an incref at the right
place (e.g., when the parent stores the response struct from the child).

Pros: Correct by design — the refcount accurately reflects liveness.
Cons: Requires identifying the exact point where the incref should happen,
which depends on the fiber abort lifecycle.

## Reference Documents

- `/dev/shm/memory-take-five.md` — memory system design doc
- `docs/io-completion-heap.md` — I/O completion heap routing spec
- `src/value/fiberheap/AGENTS.md` — FiberHeap design and invariants
- `src/vm/AGENTS.md` — Fiber swap protocol and shared allocator provisioning
