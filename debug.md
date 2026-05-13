# Bug: Heap-Use-After-Free in HTTP Test During Scope Cleanup

## Status: ROOT CAUSE FOUND, FIX IMPLEMENTED

## Root Cause

**`release_refcounted` frees slab slots whose dtor entries it skipped.**

The three phases in `release_refcounted` execute in this order:

1. **Dtor loop** — iterates `dtors[mark.dtor_len()..]`, checks `refcount == 0`,
   calls `drop_in_place` and nulls the entry for eligible objects.
2. **Children decref** — decrefs heap children of rc=0 objects.
3. **Dealloc loop** — iterates `scope_ptrs`, frees slab slots where
   `refcount == 0`.

The dtor loop checks refcount BEFORE children are decref'd. A parent object
with rc > 0 (pinned by a child's incref) is skipped — its dtor entry stays
**non-null**. Then the children decref phase decrefs those children, which can
bring the parent's refcount to 0. The dealloc loop sees rc == 0 and frees the
parent's slab slot. The non-null dtor entry now points to a freed slot.

When that slab slot is reused by a new allocation (also `needs_drop`), `alloc`
pushes the same pointer to `dtors` again. The list now has two non-null entries
for the same pointer. When `run_dtors` eventually processes the list,
`drop_in_place` runs twice on the same pointer → double-free → UAF.

### Instrumented trace confirming the root cause

```
[release_refcounted] SKIPPED dtor[1804] flat 20994 ptr 0x… tag=LString:
    in_scope=true rc_zero=false first_flat=true. Entry stays non-null.
[release_refcounted] DEALLOC slot 0x… (flat 20994) still has non-null
    dtor entry at index 1804. This entry will become STALE.
```

Flat 20994 had `rc_zero=false` at dtor-loop time (child held an incref on it).
After children decref brought rc to 0, the dealloc loop freed the slot. The
dtor entry at index 1804 was never nulled.

### Why the old Approach B hypothesis (Rc sharing) was wrong

The ASAN trace showed two `Rc<RefCell<Value>>` drops on the same `RcInner`.
This was NOT two different slab slots sharing an `Rc`. It was the SAME slab
slot being dropped twice — because the duplicate dtor entry caused
`drop_in_place` to run twice on the same `HeapObject`.

## The Fix

In the dealloc loop, after freeing a slab slot, null its dtor entry. This
ensures no stale dtor entry survives after dealloc, regardless of why the
dtor loop skipped it.

```rust
// Dealloc rc=0 slab slots, keep pinned.
for i in 0..scope_ptrs.len() {
    let ptr = scope_ptrs[i];
    let flat = scope_flats[i];
    if self.pool.refcount(ptr as *const HeapObject) == 0 {
        self.pool.unlink_alloc(flat);
        // Null any surviving dtor entry for this slot. The dtor loop
        // above may have skipped it (rc > 0 at dtor time), but children
        // decref can bring rc to 0 before we reach here.
        for di in mark.dtor_len()..self.pool.dtors.len() {
            if self.pool.dtors[di] == ptr {
                self.pool.dtors[di] = std::ptr::null_mut();
                break; // at most one non-null entry per pointer
            }
        }
        unsafe { self.pool.dealloc_slot(ptr) };
    }
}
```

This is safe because:
- The dtor loop already ran `drop_in_place` for entries it nulled. We're only
  nulling entries it SKIPPED (which were not dropped).
- The skipped objects had rc > 0 at dtor time, so they were NOT collected as
  children-to-decref. Their inner data is still valid (not dropped).
- We null the entry before `dealloc_slot` returns the slot to the free list.

## What Has Been Eliminated

- **I/O buffer double-truncate**: benign — first truncate sets length,
  second adjusts for newline position
- **bytes_to_string_in_place corruption**: safe for I/O buffers
  (traits=NIL, no Rc sharing)
- **io_uring vs thread pool**: bug reproduces without io_uring
- **Simple TCP echo test**: passes with ASAN — only leaks, no UAF.
  Bug requires the full HTTP module's loop pattern
- **Latest I/O buffer commit**: bug is pre-existing, confirmed on commits
  5+ back
- **Approach B (Rc sharing)**: the duplicate RcInner drops were a symptom
  of double-dropping the same slab slot, not two slots sharing an Rc
- **remove_from_dtors retain shifting scope marks**: not the cause;
  `drop_slot_value` was not in the failing path

## Reproduction

```bash
make smoke  # fails on http.lisp
```

## Instrumentation Added (debug_assertions only)

All instrumentation is gated behind `#[cfg(debug_assertions)]` and will be
removed after the fix is confirmed stable:

- **`SlabPool::alloc`**: panics if pushing a pointer already present in dtors
- **`SlabPool::dealloc_slot`**: panics if deallocating a slot with a
  non-null dtor entry
- **`SlabPool::remove_from_dtors`**: logs when `retain` shifts entries
  (potential scope-mark staleness)
- **`SlabPool::run_dtors`**: Approach A (duplicate pointer detection with
  flat/tag diagnostics), Approach B (RcInner sharing detection across
  different slab slots), flat dedup tracking
- **`release_refcounted` dtor loop**: logs which condition caused each skip
  (in_scope, rc_zero, first_flat) with tag and pointer info
- **`release_refcounted` dealloc loop**: logs when deallocating a slot with
  a surviving non-null dtor entry (the smoking gun)
- **`push_scope_mark`**: validates no duplicates exist in dtors at scope entry
- **`drop_slot_value`**: logs dtor indices and scope mark dtor_lens before
  `remove_from_dtors` to detect scope-mark invalidation
- **`FiberHeap::Drop`**: cross-pool Rc sharing assertion moved BEFORE any
  teardown (was previously after outbox/shared teardown, making it a no-op);
  now also checks shared allocator pools

## Reference Documents

- `/dev/shm/memory-take-five.md` — memory system design doc
- `docs/io-completion-heap.md` — I/O completion heap routing spec
