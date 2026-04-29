//! Chunk-based typed slab allocator with free-list reuse.
//!
//! All slots are `HeapObject`-sized. Pointer stability is guaranteed:
//! a `*mut HeapObject` returned by `alloc()` remains valid until the
//! slot is freed by `dealloc()` or `clear()`.
//!
//! # Pointer stability guarantee
//!
//! Each chunk is a `Box<[MaybeUninit<HeapObject>]>` — heap-allocated,
//! fixed-address. The outer `Vec<Box<...>>` stores Box pointers; when
//! the Vec grows, only the pointer array reallocates, not the chunks.
//!
//! # Free list storage
//!
//! The free list link (`Option<u32>` flat index) is stored inside the dead
//! slot's bytes. A `HeapObject` slot is at least 48 bytes; a `u32` is 4.
//! The link is written directly into the `MaybeUninit<HeapObject>` bytes.
//! The flat index is `chunk_index * chunk_size + offset_within_chunk`.

use std::mem::{size_of, MaybeUninit};

use crate::value::heap::HeapObject;

/// Number of `HeapObject` slots per chunk.
// Used in tests and will be used by Chunk 2 wiring.
#[allow(dead_code)]
const CHUNK_SIZE: usize = 256;

#[allow(dead_code)]
pub(crate) struct RootSlab {
    chunks: Vec<Box<[MaybeUninit<HeapObject>]>>,
    /// Head of the intrusive free list, as a flat slot index.
    free_head: Option<u32>,
    /// Next slot index to use in the last chunk (bump cursor within last chunk).
    bump_cursor: usize,
    live_count: usize,
}

#[allow(dead_code)]
impl RootSlab {
    pub fn new() -> Self {
        RootSlab {
            chunks: Vec::new(),
            free_head: None,
            bump_cursor: 0,
            live_count: 0,
        }
    }

    /// Allocate one slot, write `obj` into it, and return a pointer to it.
    ///
    /// The returned pointer is stable until `dealloc()` or `clear()` is called.
    pub fn alloc(&mut self, obj: HeapObject) -> *mut HeapObject {
        let ptr = if let Some(flat) = self.free_head {
            // Reuse a freed slot: read the next-link from its bytes,
            // then overwrite with the new object.
            let (chunk_idx, slot_idx) = self.split_flat(flat as usize);
            let slot = &mut self.chunks[chunk_idx][slot_idx];
            // Read the free-list next link before overwriting.
            let next: Option<u32> = unsafe { std::ptr::read(slot.as_ptr() as *const Option<u32>) };
            self.free_head = next;
            unsafe { std::ptr::write(slot.as_mut_ptr(), obj) };
            slot.as_mut_ptr()
        } else {
            // Bump path: use the next slot in the last chunk.
            if self.chunks.is_empty() || self.bump_cursor >= CHUNK_SIZE {
                self.add_chunk();
            }
            let chunk = self.chunks.last_mut().unwrap();
            let slot = &mut chunk[self.bump_cursor];
            unsafe { std::ptr::write(slot.as_mut_ptr(), obj) };
            let ptr = slot.as_mut_ptr();
            self.bump_cursor += 1;
            ptr
        };
        self.live_count += 1;
        ptr
    }

    /// Return a slot to the free list.
    ///
    /// # Safety
    /// The caller must have already called `drop_in_place(ptr)` before this.
    /// `ptr` must have been returned by a prior `alloc()` call on this slab
    /// and must not have been deallocated since.
    pub fn dealloc(&mut self, ptr: *mut HeapObject) {
        let flat = self.ptr_to_flat(ptr);
        // Write the current free_head into the dead slot's bytes as the
        // next-link in the intrusive free list.
        let (chunk_idx, slot_idx) = self.split_flat(flat);
        let slot = &mut self.chunks[chunk_idx][slot_idx];
        unsafe {
            std::ptr::write(slot.as_mut_ptr() as *mut Option<u32>, self.free_head);
        }
        self.free_head = Some(flat as u32);
        self.live_count -= 1;
    }

    /// Reset the slab: discard free list, keep first chunk, drop rest.
    ///
    /// Does NOT run destructors. The caller is responsible for running
    /// `drop_in_place` on all live objects before calling `clear()`.
    pub fn clear(&mut self) {
        self.free_head = None;
        self.bump_cursor = 0;
        self.live_count = 0;
        self.chunks.truncate(1);
    }

    /// Total backing bytes committed across all chunks.
    pub fn allocated_bytes(&self) -> usize {
        self.chunks.len() * CHUNK_SIZE * size_of::<HeapObject>()
    }

    /// Synonym for `allocated_bytes` (used by `FiberHeap::capacity()`).
    pub fn capacity_bytes(&self) -> usize {
        self.allocated_bytes()
    }

    /// Number of slots currently occupied (live allocations).
    pub fn live_count(&self) -> usize {
        self.live_count
    }

    /// Check if a pointer falls within any of this slab's chunks.
    ///
    /// O(chunks), but chunks are few (typically 1-2). Used by the outbox
    /// safety net to detect pointers into the private heap at yield time.
    pub fn owns(&self, ptr: *const ()) -> bool {
        let addr = ptr as usize;
        for chunk in &self.chunks {
            let base = chunk.as_ptr() as usize;
            let end = base + CHUNK_SIZE * size_of::<HeapObject>();
            if addr >= base && addr < end {
                return true;
            }
        }
        false
    }

    // ── Private helpers ──────────────────────────────────────────────

    fn add_chunk(&mut self) {
        let chunk: Box<[MaybeUninit<HeapObject>]> = std::iter::repeat_with(MaybeUninit::uninit)
            .take(CHUNK_SIZE)
            .collect::<Vec<_>>()
            .into_boxed_slice();
        self.chunks.push(chunk);
        self.bump_cursor = 0;
    }

    /// Convert a flat slot index to `(chunk_index, slot_within_chunk)`.
    fn split_flat(&self, flat: usize) -> (usize, usize) {
        (flat / CHUNK_SIZE, flat % CHUNK_SIZE)
    }

    /// Convert a `*mut HeapObject` back to its flat slot index.
    ///
    /// Iterates all chunks and uses pointer arithmetic to locate the slot.
    /// Called only on the dealloc path, so O(chunks) is acceptable.
    ///
    /// # Panics
    /// Panics if `ptr` does not point into any chunk (would indicate a bug).
    fn ptr_to_flat(&self, ptr: *mut HeapObject) -> usize {
        let addr = ptr as usize;
        for (chunk_idx, chunk) in self.chunks.iter().enumerate() {
            let base = chunk.as_ptr() as usize;
            let end = base + CHUNK_SIZE * size_of::<HeapObject>();
            if addr >= base && addr < end {
                let offset = (addr - base) / size_of::<HeapObject>();
                return chunk_idx * CHUNK_SIZE + offset;
            }
        }
        panic!(
            "RootSlab::dealloc: pointer {:p} not found in any chunk (use-after-free or foreign pointer)",
            ptr
        );
    }
}

impl Drop for RootSlab {
    fn drop(&mut self) {
        // MaybeUninit slots do not call HeapObject::drop.
        // The caller is responsible for running dtors before dropping the slab.
        // The Vec<Box<...>> and the Box slices themselves are freed here.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::heap::{HeapObject, Pair};
    use crate::value::Value;

    fn cons_obj() -> HeapObject {
        HeapObject::Pair(Pair::new(Value::NIL, Value::NIL))
    }

    #[test]
    fn test_slab_alloc_basic() {
        let mut slab = RootSlab::new();
        let ptr = slab.alloc(cons_obj());
        assert!(!ptr.is_null());
        assert_eq!(slab.live_count(), 1);
    }

    #[test]
    fn test_slab_alloc_multiple() {
        let mut slab = RootSlab::new();
        let mut ptrs = vec![];
        for _ in 0..5 {
            ptrs.push(slab.alloc(cons_obj()));
        }
        assert_eq!(slab.live_count(), 5);
        // All pointers must be non-null and distinct.
        for ptr in &ptrs {
            assert!(!ptr.is_null());
        }
        let unique: std::collections::HashSet<usize> = ptrs.iter().map(|p| *p as usize).collect();
        assert_eq!(unique.len(), 5);
    }

    #[test]
    fn test_slab_dealloc_returns_to_free_list() {
        let mut slab = RootSlab::new();
        let ptr1 = slab.alloc(cons_obj());
        // Caller runs drop_in_place before dealloc (Pair needs no drop, but follow the contract).
        slab.dealloc(ptr1);
        assert_eq!(slab.live_count(), 0);
        // Next alloc should reuse the same slot.
        let ptr2 = slab.alloc(cons_obj());
        assert_eq!(slab.live_count(), 1);
        assert_eq!(ptr1, ptr2, "slot must be reused after dealloc");
    }

    #[test]
    fn test_slab_pointer_stability() {
        let mut slab = RootSlab::new();
        // Allocate 300 objects (forcing chunk growth beyond 256).
        // Use Pair cells with distinguishable integer payloads.
        let mut ptrs = vec![];
        for i in 0u32..300 {
            let ptr = slab.alloc(HeapObject::Pair(Pair::new(
                Value::int(i as i64),
                Value::NIL,
            )));
            ptrs.push((ptr, i as i64));
        }
        assert_eq!(slab.live_count(), 300);
        // Verify each pointer still holds its original value after chunk growth.
        for (ptr, expected) in &ptrs {
            let obj = unsafe { &**ptr };
            match obj {
                HeapObject::Pair(c) => {
                    assert_eq!(
                        c.first.as_int().unwrap(),
                        *expected,
                        "pointer stability violated at {:p}",
                        ptr
                    )
                }
                _ => panic!("unexpected variant"),
            }
        }
    }

    #[test]
    fn test_slab_clear_resets() {
        let mut slab = RootSlab::new();
        slab.alloc(cons_obj());
        slab.alloc(cons_obj());
        slab.alloc(cons_obj());
        assert_eq!(slab.live_count(), 3);
        slab.clear();
        assert_eq!(slab.live_count(), 0);
        // Allocations work after clear.
        let p1 = slab.alloc(cons_obj());
        let p2 = slab.alloc(cons_obj());
        assert_eq!(slab.live_count(), 2);
        assert_ne!(p1, p2);
    }

    #[test]
    fn test_slab_allocated_bytes() {
        let mut slab = RootSlab::new();
        assert_eq!(slab.allocated_bytes(), 0, "no bytes before first alloc");
        slab.alloc(cons_obj()); // triggers first chunk
        assert!(
            slab.allocated_bytes() >= std::mem::size_of::<HeapObject>(),
            "at least one slot worth of bytes after first alloc"
        );
        // Exactly one chunk.
        assert_eq!(
            slab.allocated_bytes(),
            CHUNK_SIZE * std::mem::size_of::<HeapObject>()
        );
    }
}
