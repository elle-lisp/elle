//! Unsafe in-place buffer fill helpers for io completions.
//!
//! Each helper mutates a pre-allocated value that was born on the requesting
//! fiber's heap. They exist so an io completion can stamp kernel-derived data
//! into an already-allocated value without re-composing it or allocating on the
//! scheduler's heap (which would create a cross-heap reference). Every helper
//! is `unsafe` on the same contract: the owning fiber must be parked, so no
//! mutator can observe the write.

use crate::value::Value;

/// Extract a writeable pointer and length from a pre-allocated LBytes buffer.
///
/// # Safety
///
/// The caller must ensure that:
/// - The fiber that owns this buffer is parked (no mutator can read the data).
/// - The pointer is used only for a single write (kernel SQE or thread pool copy).
/// - The pointer is not used after the fiber is un-parked or torn down.
///
/// The `LBytes` variant stores an `RegionSlice<u8>` pointing into the fiber's
/// bump arena. We cast away `const` to write through it. This is safe because:
///
/// - Bump arena pages are mmap'd with `PROT_READ | PROT_WRITE`.
/// - The fiber is parked — no mutator can observe the write.
/// - The pointer escapes to C (io_uring SQE) — the optimizer cannot assume
///   the pointee is unchanged.
pub(crate) unsafe fn writeable_buffer_ptr(buffer: &Value) -> (*mut u8, usize) {
    use crate::value::heap::HeapObject;
    let ext = buffer.as_heap_ptr().expect("buffer must be heap value");
    let obj = ext as *const HeapObject;
    match &*obj {
        HeapObject::LBytes { data, .. } => (data.as_ptr() as *mut u8, data.len()),
        _ => panic!("IoOp buffer must be LBytes, got {}", buffer.type_name()),
    }
}

/// Truncate a pre-allocated LBytes buffer to the actual number of bytes written.
///
/// The pre-allocated buffer has capacity `N` but only `new_len` bytes are valid.
/// This modifies the RegionSlice's `len` field in place so that `as_bytes()`
/// returns a slice of the correct length.
///
/// # Safety
///
/// Same requirements as `writeable_buffer_ptr`: the owning fiber must be parked.
/// `new_len` must be <= the buffer's current length.
pub(crate) unsafe fn truncate_buffer(buffer: &Value, new_len: usize) {
    use crate::value::heap::HeapObject;
    use crate::value::region_slice::RegionSlice;
    let ext = buffer.as_heap_ptr().expect("buffer must be heap value");
    let obj = ext as *mut HeapObject;
    match &mut *obj {
        HeapObject::LBytes { data, .. } => {
            assert!(
                new_len <= data.len(),
                "truncate_buffer: new_len {} > buffer len {}",
                new_len,
                data.len()
            );
            *data = RegionSlice::from_raw(data.as_ptr(), new_len as u32);
        }
        _ => panic!("IoOp buffer must be LBytes, got {}", buffer.type_name()),
    }
}

/// Transmute a pre-allocated LBytes buffer into an LString in place.
///
/// After `truncate_buffer` has set the correct length, this validates the
/// buffer content as UTF-8 and transmutes the HeapObject from `LBytes` to
/// `LString` without copying data. The returned Value has `TAG_STRING` and
/// points to the same heap allocation.
///
/// This works because `LBytes` and `LString` have identical field layouts:
///
/// ```text
/// LBytes  { data: RegionSlice<u8>, traits: Value }   // HeapTag 22
/// LString { s:    RegionSlice<u8>, traits: Value }   // HeapTag 0
/// ```
///
/// Only the HeapTag discriminant and Value TAG differ — we overwrite both
/// in place via `ptr::write` (which does NOT drop the old value).
///
/// # Safety
///
/// Same requirements as `truncate_buffer`: the owning fiber must be parked.
/// The buffer must be an `LBytes` value (not already transmuted).
///
/// # Returns
///
/// `Ok(Value)` with `TAG_STRING` on valid UTF-8.
/// `Err(Value)` with an encoding error on invalid UTF-8.
pub(crate) unsafe fn bytes_to_string_in_place(
    buffer: Value,
    origin_heap: *mut crate::value::fiberheap::FiberHeap,
) -> Result<Value, Value> {
    use crate::value::heap::HeapObject;
    use crate::value::region_slice::RegionSlice;
    use crate::value::repr::TAG_STRING;

    let ptr = buffer.as_heap_ptr().expect("buffer must be heap value") as *mut HeapObject;

    let (slice_ptr, slice_len, traits) = match &*ptr {
        HeapObject::LBytes { data, traits } => (data.as_ptr(), data.len(), *traits),
        _ => panic!(
            "bytes_to_string_in_place: expected LBytes, got {}",
            buffer.type_name()
        ),
    };

    // Validate UTF-8
    let bytes = std::slice::from_raw_parts(slice_ptr, slice_len);
    if std::str::from_utf8(bytes).is_err() {
        // The error value is built on the requesting instance's heap, threaded
        // in by the caller (the same `origin_heap` every io completion uses).
        return Err(crate::io::io_error(
            "encoding-error",
            format!("port/read-line: invalid UTF-8 in {} bytes", slice_len),
            origin_heap,
        ));
    }

    // Transmute: overwrite HeapObject in place (LBytes → LString).
    // ptr::write does NOT drop the old value — safe because neither
    // RegionSlice<u8> nor Value has a Drop impl.
    std::ptr::write(
        ptr,
        HeapObject::LString {
            s: RegionSlice::from_raw(slice_ptr, slice_len as u32),
            traits,
        },
    );

    // Return new Value with TAG_STRING, same heap pointer
    Ok(Value::from_heap_ptr(ptr as *const (), TAG_STRING))
}

/// Overwrite one field of a pre-allocated **immutable** struct in place.
///
/// Used by the `RecvFrom` completion to stamp the kernel-derived `:addr`
/// (an `LString` re-tagged from the pre-allocated `:addr` buffer) and `:port`
/// (an int) into the result struct that was pre-allocated on the requesting
/// fiber's heap — without re-composing the struct or allocating a new one.
///
/// `LStruct.data` is a `RegionSlice` over the struct's own region pages, which
/// the region owns and may write; the slice hands out only shared borrows, so
/// the write goes through the backing pointer. The sorted key order is
/// preserved because we never change a key.
///
/// # Safety
///
/// Same contract as the other in-place fill helpers: the owning fiber must be
/// parked (exclusive access). `key` must already be present. The replaced and
/// replacement values must not require RC fixups beyond what the caller manages
/// — in practice the old slot holds a placeholder (`int 0` / pre-alloc buffer)
/// and the new value is owned by / lives on the same fiber heap.
pub(crate) unsafe fn set_struct_field_in_place(
    result: &Value,
    key: &crate::value::heap::TableKey,
    val: Value,
) {
    use crate::value::heap::HeapObject;
    let ptr = result
        .as_heap_ptr()
        .expect("recv result must be a heap value") as *mut HeapObject;
    match &mut *ptr {
        HeapObject::LStruct { data, .. } => {
            let entries = data.as_ptr() as *mut (crate::value::heap::TableKey, Value);
            for i in 0..data.len() {
                let entry = &mut *entries.add(i);
                if &entry.0 == key {
                    entry.1 = val;
                    return;
                }
            }
            panic!("set_struct_field_in_place: key {:?} not present", key);
        }
        _ => panic!(
            "set_struct_field_in_place: expected immutable struct, got {}",
            result.type_name()
        ),
    }
}
