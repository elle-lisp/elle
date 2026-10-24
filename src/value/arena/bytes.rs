// audited: 2026-10-22
//! The byte-store funnels: the one write an `@string` or `@bytes` takes, and an append built on it.
//!
//! docs/impl/mutable.md

use super::*;

/// Replace the bytes in `range` of an `@string` or `@bytes` with `with`, and
/// answer the bytes it removed. A byte store holds no `Value`, so no region
/// tracking runs. The heap is here because a store that grows claims room in
/// the container's region. The caller keeps an `@string` UTF-8. Panics if
/// `store` is neither, or `range` is out of bounds.
pub fn bytes_splice(
    heap: &mut FiberHeap,
    store: Value,
    range: std::ops::Range<usize>,
    with: &[u8],
) -> Vec<u8> {
    let _ = heap;
    store
        .string_store()
        .or_else(|| store.bytes_store())
        .expect("bytes_splice: expected @string or @bytes")
        .splice(range, with)
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
