// audited: 2026-10-22
// The byte store of an @string or @bytes: an extent in its container's region,
// grown by the splice and owning nothing outside the region's pages.
//
// docs/impl/mutable.md
//
// A store on the Rust heap passes every read and write pin in
// mutable_store.rs, so those cannot tell where the bytes live. These pins can:
// each one fails while the store is a `Vec`, and holds once its bytes are an
// extent of the container's region.

use elle::primitives::ctx::TestHeap;
use elle::value::arena::{bytes_append, bytes_splice, region_of};
use elle::value::repr::TAG_BYTES;
use elle::value::store::ByteStore;
use elle::Value;

/// A heap value at `ptr`, for the region store's ownership check, which reads
/// nothing but the address.
fn address(ptr: *const u8) -> Value {
    Value::from_heap_ptr(ptr as *const (), TAG_BYTES)
}

// A `Vec`'s buffer is a `malloc` the region store does not own. So the
// ownership check fails first, and the page-header read beside it never runs
// against a pointer with no header.
#[test]
fn a_grown_byte_store_lies_in_its_containers_region() {
    let h = TestHeap::new();
    let b = h.ctx().bytes_mut(vec![1, 2, 3]);
    let s = h.ctx().string_mut(b"abc".to_vec());
    for _ in 0..4 {
        bytes_append(h.heap(), b, &[0; 1000]);
        bytes_append(h.heap(), s, &[b'x'; 1000]);
    }
    for v in [b, s] {
        let read = v
            .bytes_mut_read()
            .or_else(|| v.string_mut_read())
            .expect("a byte store");
        assert!(
            h.heap().value_in_region_store(address(read.as_ptr())),
            "a grown {} keeps its bytes outside every region of its heap",
            v.type_name()
        );
        assert_eq!(
            h.heap().region_of_ptr(read.as_ptr() as *const ()),
            region_of(h.heap(), v).expect("a heap value").get(),
            "a grown {} keeps its bytes in a region other than its own",
            v.type_name()
        );
    }
}

#[test]
fn a_byte_store_owns_nothing_to_drop() {
    assert!(
        !std::mem::needs_drop::<ByteStore>(),
        "a byte store owns memory outside its region's pages"
    );
}

#[test]
fn a_string_store_grows_past_three_doublings_and_reads_back() {
    let h = TestHeap::new();
    let s = h.ctx().string_mut(Vec::new());
    let mut want = Vec::new();
    for n in 0..100u8 {
        let piece = [b'a' + n % 26];
        bytes_append(h.heap(), s, &piece);
        want.extend_from_slice(&piece);
    }
    assert_eq!(&s.string_mut_read().unwrap()[..], &want[..]);
}

// A cross-thread channel delivers a pointer into the sender's heap, so a
// receiver can hold a store its own heap does not. Growing one would claim an
// extent in a region the receiver cannot name.
#[test]
#[should_panic(expected = "no region of this heap")]
fn a_growing_splice_on_another_heaps_store_panics() {
    let ours = TestHeap::new();
    let theirs = TestHeap::new();
    let b = theirs.ctx().bytes_mut(vec![1, 2, 3]);
    bytes_append(ours.heap(), b, &[4]);
}

// The counterpart of the panic above: a write that fits the capacity claims
// nothing, so it needs no region and stays legal on a foreign store.
#[test]
fn a_splice_within_capacity_on_another_heaps_store_edits_it() {
    let ours = TestHeap::new();
    let theirs = TestHeap::new();
    let b = theirs.ctx().bytes_mut(vec![1, 2, 3]);
    assert_eq!(bytes_splice(ours.heap(), b, 0..2, &[9, 8]), vec![1, 2]);
    assert_eq!(bytes_splice(ours.heap(), b, 2..3, &[]), vec![3]);
    assert_eq!(&b.bytes_mut_read().unwrap()[..], &[9, 8]);
}
