// audited: 2026-10-22
// The store of an @array: an extent of Value in its container's region, grown
// by the funnels, owning nothing outside it.
//
// docs/impl/mutable.md
//
// A store on the Rust heap passes every read, write and borrow pin, so those
// cannot tell where the elements live. The placement, drop and foreign-shell
// pins can: each fails while the store is a `Vec`, and holds once its elements
// are an extent of the container's region. The model and borrow pins hold on
// both, and guard what the move must keep.

use elle::primitives::ctx::TestHeap;
use elle::value::arena::{
    drain_tail_with_decref, extend_with_incref, insert_with_incref, pop_with_decref,
    push_with_incref, region_of, remove_at_with_decref, set_at_with_rebind, with_array_mut_neutral,
};
use elle::value::repr::TAG_ARRAY;
use elle::value::store::ArrayStore;
use elle::Value;

fn ints(ns: &[i64]) -> Vec<Value> {
    ns.iter().map(|&n| Value::int(n)).collect()
}

/// A heap value at `ptr`, for the region store's ownership check, which reads
/// nothing but the address.
fn address(ptr: *const Value) -> Value {
    Value::from_heap_ptr(ptr as *const (), TAG_ARRAY)
}

fn elements(arr: Value) -> Vec<Value> {
    arr.array_mut_read().expect("an @array").to_vec()
}

// A `Vec`'s buffer is a `malloc` the region store does not own. So the
// ownership check fails first, and the page-header read beside it never runs
// against a pointer with no header.
#[test]
fn a_grown_array_store_lies_in_its_containers_region() {
    let h = TestHeap::new();
    let arr = h.ctx().array_mut(ints(&[1, 2, 3]));
    for n in 0..1000 {
        push_with_incref(h.heap(), arr, Value::int(n));
    }
    let read = arr.array_mut_read().expect("an @array");
    assert!(
        h.heap().value_in_region_store(address(read.as_ptr())),
        "a grown @array keeps its elements outside every region of its heap"
    );
    assert_eq!(
        h.heap().region_of_ptr(read.as_ptr() as *const ()),
        region_of(h.heap(), arr).expect("a heap value").get(),
        "a grown @array keeps its elements in a region other than its own"
    );
}

#[test]
fn an_array_store_owns_nothing_to_drop() {
    assert!(
        !std::mem::needs_drop::<ArrayStore>(),
        "an array store owns memory outside its region's pages"
    );
}

// A cross-thread channel delivers a pointer into the sender's heap, so a
// receiver can hold a store its own heap does not. Growing one would claim an
// extent in a region the receiver cannot name.
#[test]
#[should_panic(expected = "no region of this heap")]
fn a_growing_push_on_another_heaps_array_panics() {
    let ours = TestHeap::new();
    let theirs = TestHeap::new();
    let arr = theirs.ctx().array_mut(ints(&[1, 2, 3]));
    push_with_incref(ours.heap(), arr, Value::int(4));
}

// The counterpart of the panic above: a write that fits the capacity claims
// nothing, so it needs no region and stays legal on a foreign store. The
// elements are immediates, so no write here resolves a region either.
#[test]
fn a_write_within_capacity_on_another_heaps_array_edits_it() {
    let ours = TestHeap::new();
    let theirs = TestHeap::new();
    let arr = theirs.ctx().array_mut(ints(&[1, 2, 3]));
    assert_eq!(
        set_at_with_rebind(ours.heap(), arr, 0, Value::int(9)),
        Value::int(1)
    );
    assert_eq!(pop_with_decref(ours.heap(), arr), Value::int(3));
    assert_eq!(remove_at_with_decref(ours.heap(), arr, 0), Value::int(9));
    insert_with_incref(ours.heap(), arr, 0, Value::int(7));
    assert_eq!(elements(arr), ints(&[7, 2]));
}

// The counter-factual for the move: a splice that grows and shifts in one
// write is where a copy loses or duplicates an element. A front insert moves
// the whole tail, and a growth moves the whole store, so the sequence below
// does both many times over and checks every funnel against a `Vec`.
#[test]
fn every_array_funnel_agrees_with_a_vec_through_growth_and_shifts() {
    let h = TestHeap::new();
    let arr = h.ctx().array_mut(Vec::new());
    let mut model: Vec<Value> = Vec::new();
    for n in 0..300i64 {
        let v = Value::int(n);
        match n % 7 {
            0 | 1 => {
                insert_with_incref(h.heap(), arr, 0, v);
                model.insert(0, v);
            }
            2 => {
                push_with_incref(h.heap(), arr, v);
                model.push(v);
            }
            3 => {
                let more = ints(&[n, n + 1000, n + 2000]);
                extend_with_incref(h.heap(), arr, &more);
                model.extend_from_slice(&more);
            }
            4 => {
                let at = model.len() / 2;
                assert_eq!(remove_at_with_decref(h.heap(), arr, at), model.remove(at));
            }
            5 => {
                let at = model.len() / 3;
                let old = std::mem::replace(&mut model[at], v);
                assert_eq!(set_at_with_rebind(h.heap(), arr, at, v), old);
            }
            _ => {
                let len = model.len();
                let tail = model.split_off(len - 2.min(len));
                assert_eq!(drain_tail_with_decref(h.heap(), arr, 2), tail);
                if let Some(last) = model.pop() {
                    assert_eq!(pop_with_decref(h.heap(), arr), last);
                }
            }
        }
        assert_eq!(elements(arr), model, "after step {n}");
    }
    with_array_mut_neutral(arr, |s| s.reverse());
    model.reverse();
    assert_eq!(elements(arr), model, "after a neutral reverse");
}

#[test]
#[should_panic(expected = "already borrowed")]
fn a_set_under_an_array_read_panics() {
    let h = TestHeap::new();
    let arr = h.ctx().array_mut(ints(&[1]));
    let _read = arr.array_mut_read().unwrap();
    set_at_with_rebind(h.heap(), arr, 0, Value::int(2));
}

#[test]
#[should_panic(expected = "already borrowed")]
fn a_pop_under_an_array_read_panics() {
    let h = TestHeap::new();
    let arr = h.ctx().array_mut(ints(&[1]));
    let _read = arr.array_mut_read().unwrap();
    pop_with_decref(h.heap(), arr);
}

#[test]
#[should_panic(expected = "already borrowed")]
fn a_neutral_write_under_an_array_read_panics() {
    let h = TestHeap::new();
    let arr = h.ctx().array_mut(ints(&[2, 1]));
    let _read = arr.array_mut_read().unwrap();
    with_array_mut_neutral(arr, |s| s.sort());
}

// A read while a write runs would see a store mid-change, so the read panics
// rather than answer.
#[test]
#[should_panic(expected = "already mutably borrowed")]
fn an_array_read_during_a_neutral_write_panics() {
    let h = TestHeap::new();
    let arr = h.ctx().array_mut(ints(&[2, 1]));
    with_array_mut_neutral(arr, |_| {
        let _ = arr.array_mut_read();
    });
}

#[test]
fn a_neutral_write_gives_the_store_back_to_readers() {
    let h = TestHeap::new();
    let arr = h.ctx().array_mut(ints(&[2, 1]));
    with_array_mut_neutral(arr, |s| s.sort());
    assert_eq!(elements(arr), ints(&[1, 2]));
}
