// audited: 2026-10-22
//! The byte-store funnels: the one write an `@string` or `@bytes` takes, and an append built on it.
//!
//! docs/impl/mutable.md

use super::*;

/// Replace the bytes in `range` of an `@string` or `@bytes` with `with`, and
/// answer the bytes it removed. A byte store holds no `Value`, so no region
/// tracking runs. A store that must grow claims its new extent in the region
/// of `heap` whose pages hold the shell. The caller keeps an `@string` UTF-8.
///
/// Panics if `store` is neither, if `range` is out of bounds, or if the store
/// must grow and no region of `heap` holds it: a shell another thread's heap
/// owns, which a cross-thread channel delivers.
pub fn bytes_splice(
    heap: &mut FiberHeap,
    store: Value,
    range: std::ops::Range<usize>,
    with: &[u8],
) -> Vec<u8> {
    let bytes = store
        .string_store()
        .or_else(|| store.bytes_store())
        .expect("bytes_splice: expected @string or @bytes");
    bytes.splice(range, with, |n| {
        let region = heap.owning_region_of(store).unwrap_or_else(|| {
            panic!(
                "bytes_splice: the {} lies in no region of this heap, so it cannot \
                 grow (docs/impl/mutable.md)",
                store.type_name()
            )
        });
        let room = heap.alloc_room_in_region::<u8>(n, region);
        std::ptr::NonNull::new(room).expect("a region allocation is never null")
    })
}

/// Append `with` to an `@string` or `@bytes`: a [`bytes_splice`] at the end.
pub fn bytes_append(heap: &mut FiberHeap, store: Value, with: &[u8]) {
    let end = store
        .string_mut_read()
        .or_else(|| store.bytes_mut_read())
        .expect("bytes_append: expected @string or @bytes")
        .len();
    bytes_splice(heap, store, end..end, with);
}
