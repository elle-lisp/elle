//! Chunk-based typed slab allocator with free-list reuse.
//!
//! All slots are `HeapObject`-sized. Pointer stability is guaranteed:
//! a `*mut HeapObject` returned by `alloc()` remains valid until the
//! slot is freed by `dealloc()` or `clear()`.
//!
//! # OS-level memory return
//!
//! Chunks are backed by `mmap` rather than the process heap. When
//! `clear()` or `dealloc()` empties a chunk, its pages are returned to
//! the OS via `munmap`. This bypasses any allocator caching — RSS tracks
//! actual live memory.
//!
//! # Pointer stability guarantee
//!
//! Each chunk is an `mmap`'d region at a fixed virtual address. The outer
//! `Vec<Chunk>` stores chunk metadata; when the Vec grows, only the
//! metadata array reallocates, not the chunks themselves.
//!
//! # Free list storage
//!
//! The free list link (`Option<u32>` flat index) is stored inside the dead
//! slot's bytes. A `HeapObject` slot is at least 48 bytes; a `u32` is 4.
//! The link is written directly into the `MaybeUninit<HeapObject>` bytes.
//! The flat index is `chunk_index * CHUNK_SIZE + offset_within_chunk`.

use std::mem::{size_of, MaybeUninit};

use crate::value::heap::HeapObject;

/// Number of `HeapObject` slots per chunk.
const CHUNK_SIZE: usize = 256;

/// Bytes per chunk: must hold CHUNK_SIZE HeapObject slots.
const CHUNK_BYTES: usize = CHUNK_SIZE * size_of::<HeapObject>();

/// An mmap-backed chunk of `CHUNK_SIZE` HeapObject slots.
struct Chunk {
    ptr: *mut MaybeUninit<HeapObject>,
}

impl Chunk {
    fn new() -> Option<Self> {
        let ptr = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                CHUNK_BYTES,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_PRIVATE | libc::MAP_ANONYMOUS,
                -1,
                0,
            )
        };
        if ptr == libc::MAP_FAILED {
            None
        } else {
            Some(Chunk {
                ptr: ptr as *mut MaybeUninit<HeapObject>,
            })
        }
    }

    fn slot(&mut self, idx: usize) -> *mut MaybeUninit<HeapObject> {
        unsafe { self.ptr.add(idx) }
    }

    fn base(&self) -> *const u8 {
        self.ptr as *const u8
    }

    fn end(&self) -> *const u8 {
        unsafe { self.base().add(CHUNK_BYTES) }
    }
}

impl Drop for Chunk {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.ptr as *mut libc::c_void, CHUNK_BYTES);
        }
    }
}

// SAFETY: Chunk owns its mmap'd memory exclusively.
unsafe impl Send for Chunk {}

pub(crate) struct Slab {
    chunks: Vec<Chunk>,
    /// Head of the intrusive free list, as a flat slot index.
    free_head: Option<u32>,
    /// Next slot index to use in the last chunk (bump cursor).
    bump_cursor: usize,
    live_count: usize,
}

impl Slab {
    pub fn new() -> Self {
        Slab {
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
            let (chunk_idx, slot_idx) = self.split_flat(flat as usize);
            let slot = self.chunks[chunk_idx].slot(slot_idx);
            let next: Option<u32> = unsafe { std::ptr::read(slot as *const Option<u32>) };
            self.free_head = next;
            unsafe { std::ptr::write(slot as *mut HeapObject, obj) };
            slot as *mut HeapObject
        } else {
            if self.chunks.is_empty() || self.bump_cursor >= CHUNK_SIZE {
                self.add_chunk();
            }
            let slot = self.chunks.last_mut().unwrap().slot(self.bump_cursor);
            unsafe { std::ptr::write(slot as *mut HeapObject, obj) };
            self.bump_cursor += 1;
            slot as *mut HeapObject
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
        let (chunk_idx, slot_idx) = self.split_flat(flat);
        let slot = self.chunks[chunk_idx].slot(slot_idx);
        unsafe {
            std::ptr::write(slot as *mut Option<u32>, self.free_head);
        }
        self.free_head = Some(flat as u32);
        self.live_count -= 1;
    }

    /// Reset the slab: discard free list, keep first chunk, drop (munmap) the rest.
    /// The retained chunk gets `madvise(MADV_DONTNEED)` to release physical frames.
    ///
    /// Does NOT run destructors. The caller is responsible for running
    /// `drop_in_place` on all live objects before calling `clear()`.
    pub fn clear(&mut self) {
        self.free_head = None;
        self.bump_cursor = 0;
        self.live_count = 0;
        self.chunks.truncate(1);
        if let Some(chunk) = self.chunks.first() {
            unsafe {
                libc::madvise(
                    chunk.ptr as *mut libc::c_void,
                    CHUNK_BYTES,
                    libc::MADV_DONTNEED,
                );
            }
        }
    }

    /// Total backing bytes committed across all chunks.
    pub fn allocated_bytes(&self) -> usize {
        self.chunks.len() * CHUNK_BYTES
    }

    /// Number of slots currently occupied (live allocations).
    pub fn live_count(&self) -> usize {
        self.live_count
    }

    /// Check if a pointer falls within any of this slab's chunks.
    ///
    /// O(chunks), but chunks are few (typically 1-3). Used by the outbox
    /// safety net to detect pointers into the private heap at yield time.
    pub fn owns(&self, ptr: *const ()) -> bool {
        let addr = ptr as usize;
        for chunk in &self.chunks {
            let base = chunk.base() as usize;
            let end = chunk.end() as usize;
            if addr >= base && addr < end {
                return true;
            }
        }
        false
    }

    // ── Private helpers ──────────────────────────────────────────────

    fn add_chunk(&mut self) {
        let chunk = Chunk::new().expect("slab: mmap chunk failed");
        self.chunks.push(chunk);
        self.bump_cursor = 0;
    }

    /// Convert a flat slot index to `(chunk_index, slot_within_chunk)`.
    fn split_flat(&self, flat: usize) -> (usize, usize) {
        (flat / CHUNK_SIZE, flat % CHUNK_SIZE)
    }

    /// Convert a `*mut HeapObject` back to its flat slot index.
    ///
    /// # Panics
    /// Panics if `ptr` does not point into any chunk (would indicate a bug).
    fn ptr_to_flat(&self, ptr: *mut HeapObject) -> usize {
        let addr = ptr as usize;
        for (chunk_idx, chunk) in self.chunks.iter().enumerate() {
            let base = chunk.base() as usize;
            let end = chunk.end() as usize;
            if addr >= base && addr < end {
                let offset = (addr - base) / size_of::<HeapObject>();
                return chunk_idx * CHUNK_SIZE + offset;
            }
        }
        panic!(
            "Slab::dealloc: pointer {:p} not found in any chunk (use-after-free or foreign pointer)",
            ptr
        );
    }
}

impl Default for Slab {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for Slab {
    fn drop(&mut self) {
        // MaybeUninit slots do not call HeapObject::drop.
        // The caller is responsible for running dtors before dropping the slab.
        // Chunk::drop calls munmap for each chunk.
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
        let mut slab = Slab::new();
        let ptr = slab.alloc(cons_obj());
        assert!(!ptr.is_null());
        assert_eq!(slab.live_count(), 1);
    }

    #[test]
    fn test_slab_alloc_multiple() {
        let mut slab = Slab::new();
        let mut ptrs = vec![];
        for _ in 0..5 {
            ptrs.push(slab.alloc(cons_obj()));
        }
        assert_eq!(slab.live_count(), 5);
        for ptr in &ptrs {
            assert!(!ptr.is_null());
        }
        let unique: std::collections::HashSet<usize> = ptrs.iter().map(|p| *p as usize).collect();
        assert_eq!(unique.len(), 5);
    }

    #[test]
    fn test_slab_dealloc_returns_to_free_list() {
        let mut slab = Slab::new();
        let ptr1 = slab.alloc(cons_obj());
        slab.dealloc(ptr1);
        assert_eq!(slab.live_count(), 0);
        let ptr2 = slab.alloc(cons_obj());
        assert_eq!(slab.live_count(), 1);
        assert_eq!(ptr1, ptr2, "slot must be reused after dealloc");
    }

    #[test]
    fn test_slab_pointer_stability() {
        let mut slab = Slab::new();
        let mut ptrs = vec![];
        for i in 0u32..300 {
            let ptr = slab.alloc(HeapObject::Pair(Pair::new(
                Value::int(i as i64),
                Value::NIL,
            )));
            ptrs.push((ptr, i as i64));
        }
        assert_eq!(slab.live_count(), 300);
        for (ptr, expected) in &ptrs {
            let obj = unsafe { &**ptr };
            match obj {
                HeapObject::Pair(c) => {
                    assert_eq!(c.first.as_int().unwrap(), *expected)
                }
                _ => panic!("unexpected variant"),
            }
        }
    }

    #[test]
    fn test_slab_clear_resets() {
        let mut slab = Slab::new();
        slab.alloc(cons_obj());
        slab.alloc(cons_obj());
        slab.alloc(cons_obj());
        assert_eq!(slab.live_count(), 3);
        slab.clear();
        assert_eq!(slab.live_count(), 0);
        let p1 = slab.alloc(cons_obj());
        let p2 = slab.alloc(cons_obj());
        assert_eq!(slab.live_count(), 2);
        assert_ne!(p1, p2);
    }

    #[test]
    fn test_slab_allocated_bytes() {
        let mut slab = Slab::new();
        assert_eq!(slab.allocated_bytes(), 0, "no bytes before first alloc");
        slab.alloc(cons_obj());
        assert_eq!(
            slab.allocated_bytes(),
            CHUNK_BYTES,
            "one full chunk after first alloc"
        );
    }

    #[test]
    fn test_slab_owns() {
        let mut slab = Slab::new();
        let ptr = slab.alloc(cons_obj()) as *const ();
        assert!(slab.owns(ptr));
        let x: i64 = 42;
        assert!(!slab.owns(&x as *const _ as *const ()));
    }
}
