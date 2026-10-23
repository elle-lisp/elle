// audited: 2026-10-22
// The mutable-store seam: its read guards, the byte splice, and the borrow rule
// that refuses a write under a live read.
//
// docs/impl/mutable.md
//
// The borrow pins hold through every step that moves a store into region
// pages. A write that runs while a read guard is live can shift or regrow the
// contents under the reader, so it must panic rather than proceed. Without the
// borrow count, each pin's write would succeed, and its guard would read stale
// or moved contents.

use elle::primitives::ctx::TestHeap;
use elle::value::arena::{
    bytes_splice, capture_store_with_rebind, lbox_store_with_rebind, push_with_incref,
    set_add_with_incref, struct_put_with_rebind,
};
use elle::value::heap::CellOrigin;
use elle::value::types::TableKey;
use elle::Value;
use std::collections::{BTreeMap, BTreeSet};

fn ints(ns: &[i64]) -> Vec<Value> {
    ns.iter().map(|&n| Value::int(n)).collect()
}

#[test]
fn an_array_read_shows_the_elements_in_order() {
    let h = TestHeap::new();
    let arr = h.ctx().array_mut(ints(&[3, 1, 2]));
    push_with_incref(h.heap(), arr, Value::int(9));
    let read = arr.array_mut_read().expect("an @array has an array read");
    assert_eq!(&read[..], &ints(&[3, 1, 2, 9])[..]);
    assert!(Value::int(1).array_mut_read().is_none());
}

#[test]
fn a_byte_read_shows_the_bytes_of_its_own_kind() {
    let h = TestHeap::new();
    let s = h.ctx().string_mut(b"hello".to_vec());
    let b = h.ctx().bytes_mut(vec![1, 2, 3]);
    assert_eq!(&s.string_mut_read().unwrap()[..], b"hello");
    assert_eq!(&b.bytes_mut_read().unwrap()[..], &[1, 2, 3]);
    assert!(s.bytes_mut_read().is_none(), "an @string is not @bytes");
    assert!(b.string_mut_read().is_none(), "@bytes is not an @string");
}

#[test]
fn a_struct_read_answers_lookups_in_key_order() {
    let h = TestHeap::new();
    let st = h.ctx().struct_mut();
    for k in [3, 1, 2] {
        struct_put_with_rebind(h.heap(), st, TableKey::Int(k), Value::int(k * 10));
    }
    let read = st.struct_mut_read().expect("an @struct has a struct read");
    assert_eq!(read.len(), 3);
    assert!(!read.is_empty());
    assert_eq!(read.get(&TableKey::Int(2)), Some(Value::int(20)));
    assert_eq!(read.get(&TableKey::Int(4)), None);
    let order: Vec<(TableKey, Value)> = read.iter().map(|(k, v)| (*k, *v)).collect();
    assert_eq!(
        order,
        vec![
            (TableKey::Int(1), Value::int(10)),
            (TableKey::Int(2), Value::int(20)),
            (TableKey::Int(3), Value::int(30)),
        ]
    );
}

#[test]
fn a_set_read_answers_membership_in_order() {
    let h = TestHeap::new();
    let set = h.ctx().set_mut(BTreeSet::new());
    for n in [5, 4, 6, 4] {
        set_add_with_incref(h.heap(), set, Value::int(n));
    }
    let read = set.set_mut_read().expect("an @set has a set read");
    assert_eq!(read.len(), 3);
    assert!(read.contains(&Value::int(6)));
    assert!(!read.contains(&Value::int(7)));
    let order: Vec<Value> = read.iter().copied().collect();
    assert_eq!(order, ints(&[4, 5, 6]));
}

#[test]
fn a_splice_appends_inserts_removes_and_overwrites() {
    let h = TestHeap::new();
    let b = h.ctx().bytes_mut(vec![1, 2, 3]);
    assert!(bytes_splice(h.heap(), b, 3..3, &[4, 5]).is_empty());
    assert!(bytes_splice(h.heap(), b, 0..0, &[0]).is_empty());
    assert_eq!(bytes_splice(h.heap(), b, 2..4, &[]), vec![2, 3]);
    assert_eq!(bytes_splice(h.heap(), b, 1..2, &[7, 7, 7]), vec![1]);
    assert_eq!(&b.bytes_mut_read().unwrap()[..], &[0, 7, 7, 7, 4, 5]);
}

#[test]
fn a_splice_edits_a_string_store() {
    let h = TestHeap::new();
    let s = h.ctx().string_mut(b"hello".to_vec());
    bytes_splice(h.heap(), s, 5..5, b", world");
    assert_eq!(bytes_splice(h.heap(), s, 0..1, b"j"), b"h".to_vec());
    assert_eq!(&s.string_mut_read().unwrap()[..], b"jello, world");
}

#[test]
fn a_store_grows_past_three_doublings_and_reads_back() {
    let h = TestHeap::new();
    let arr = h.ctx().array_mut(Vec::new());
    let b = h.ctx().bytes_mut(Vec::new());
    let set = h.ctx().set_mut(BTreeSet::new());
    for n in 0..100 {
        push_with_incref(h.heap(), arr, Value::int(n));
        let end = b.bytes_mut_read().unwrap().len();
        bytes_splice(h.heap(), b, end..end, &[n as u8]);
        set_add_with_incref(h.heap(), set, Value::int(99 - n));
    }
    let want: Vec<i64> = (0..100).collect();
    assert_eq!(&arr.array_mut_read().unwrap()[..], &ints(&want)[..]);
    let bytes: Vec<u8> = (0..100).collect();
    assert_eq!(&b.bytes_mut_read().unwrap()[..], &bytes[..]);
    let members: Vec<Value> = set.set_mut_read().unwrap().iter().copied().collect();
    assert_eq!(members, ints(&want));
}

#[test]
fn a_box_and_a_capture_cell_copy_out_the_stored_value() {
    let h = TestHeap::new();
    let bx = h.ctx().lbox(Value::int(1));
    let cell = h.ctx().capture_cell(Value::int(1), CellOrigin::Runtime);
    lbox_store_with_rebind(h.heap(), bx, Value::int(2));
    capture_store_with_rebind(h.heap(), cell, Value::int(3));
    assert_eq!(bx.lbox_get(), Some(Value::int(2)));
    assert_eq!(cell.capture_cell_get(), Some(Value::int(3)));
}

#[test]
#[should_panic(expected = "already borrowed")]
fn a_push_under_an_array_read_panics() {
    let h = TestHeap::new();
    let arr = h.ctx().array_mut(ints(&[1]));
    let _read = arr.array_mut_read().unwrap();
    push_with_incref(h.heap(), arr, Value::int(2));
}

#[test]
#[should_panic(expected = "already borrowed")]
fn a_splice_under_a_byte_read_panics() {
    let h = TestHeap::new();
    let b = h.ctx().bytes_mut(vec![1]);
    let _read = b.bytes_mut_read().unwrap();
    bytes_splice(h.heap(), b, 1..1, &[2]);
}

#[test]
#[should_panic(expected = "already borrowed")]
fn a_put_under_a_struct_read_panics() {
    let h = TestHeap::new();
    let st = h.ctx().struct_mut_from(BTreeMap::new());
    let _read = st.struct_mut_read().unwrap();
    struct_put_with_rebind(h.heap(), st, TableKey::Int(1), Value::int(1));
}

#[test]
#[should_panic(expected = "already borrowed")]
fn an_add_under_a_set_read_panics() {
    let h = TestHeap::new();
    let set = h.ctx().set_mut(BTreeSet::new());
    let _read = set.set_mut_read().unwrap();
    set_add_with_incref(h.heap(), set, Value::int(1));
}
