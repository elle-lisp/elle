// audited: 2026-10-23
// A @struct's funnels and reads agree with a BTreeMap through every shape its store takes.
//
// docs/impl/mutable.md
//
// The counter-factual: a tree edit that drops, repeats or misplaces an entry
// passes a small test, and so does a rebind that adds an entry where it should
// replace a value, because a few dozen entries fit one or two nodes. These runs
// reach a few thousand entries, so the store splits and merges at every level,
// borrows from both siblings, and collapses its root. Every answer, every
// length and every state is checked against a `BTreeMap` after each step, so
// the first edit that goes wrong names its step.

use std::collections::BTreeMap;

use elle::primitives::ctx::TestHeap;
use elle::value::arena::{struct_put_with_rebind, struct_remove_with_decref};
use elle::value::types::TableKey;
use elle::Value;

/// The `i`th key in a scattered order. 7919 is prime to the modulus, so the
/// first million keys are distinct.
fn scattered(i: i64) -> i64 {
    i * 7919 % 1_000_003
}

/// The key for `n`: a string for every third, built afresh on each call, so a
/// rebind and a delete find it by content, not by identity.
fn key(h: &TestHeap, n: i64) -> TableKey {
    if n % 3 == 0 {
        TableKey::from_value(&h.ctx().string(format!("k{n}"))).expect("a string key")
    } else {
        TableKey::Int(n)
    }
}

/// A @struct beside the `BTreeMap` it must agree with.
struct Twin<'h> {
    h: &'h TestHeap,
    st: Value,
    model: BTreeMap<TableKey, Value>,
}

impl<'h> Twin<'h> {
    fn new(h: &'h TestHeap) -> Self {
        Twin {
            h,
            st: h.ctx().struct_mut(),
            model: BTreeMap::new(),
        }
    }

    /// Store `v` under the key for `n`, which may already be present.
    fn put(&mut self, n: i64, v: i64) {
        let k = key(self.h, n);
        let old = struct_put_with_rebind(self.h.heap(), self.st, k, Value::int(v));
        assert_eq!(old, self.model.insert(k, Value::int(v)), "the put of {n}");
        self.agree(n);
    }

    fn del(&mut self, n: i64) {
        let k = key(self.h, n);
        let removed = struct_remove_with_decref(self.h.heap(), self.st, &k);
        assert_eq!(removed, self.model.remove(&k), "the delete of {n}");
        self.agree(n);
    }

    /// The store's length and entries in both directions, after the step
    /// that wrote the key for `n`.
    fn agree(&self, n: i64) {
        let read = self.st.struct_mut_read().expect("an @struct");
        assert_eq!(read.len(), self.model.len(), "the length after {n}");
        assert_eq!(read.is_empty(), self.model.is_empty());
        assert!(
            read.iter().eq(self.model.iter()),
            "the entries ascending after {n}"
        );
        assert!(
            read.iter().rev().eq(self.model.iter().rev()),
            "the entries descending after {n}"
        );
    }

    /// The lookup of every key below `n`, present or not.
    fn answers_lookups_below(&self, n: i64) {
        let read = self.st.struct_mut_read().expect("an @struct");
        for i in 0..n {
            let k = key(self.h, i);
            assert_eq!(read.get(&k), self.model.get(&k).copied(), "{i}");
            assert_eq!(read.contains_key(&k), self.model.contains_key(&k), "{i}");
        }
    }
}

#[test]
fn scattered_puts_rebinds_and_heavy_deletes_agree_with_a_btreemap() {
    let h = TestHeap::new();
    let mut twin = Twin::new(&h);
    let keys: Vec<i64> = (0..2000).map(|i| scattered(i) % 3000).collect();
    for (i, &n) in keys.iter().enumerate() {
        twin.put(n, i as i64);
        if i % 7 == 6 {
            twin.put(keys[i / 2], -(i as i64));
        }
    }
    twin.answers_lookups_below(3000);
    for i in 0..1900 {
        twin.del(keys[i * 1237 % 2000]);
        if i % 11 == 10 {
            twin.del(3000 + i as i64);
        }
    }
    twin.answers_lookups_below(3000);
    for &n in &keys[..1000] {
        twin.put(n, n);
    }
    for &n in &keys {
        twin.del(n);
    }
    assert!(twin.model.is_empty(), "every key is deleted");
    twin.put(7, 7);
    twin.answers_lookups_below(3000);
}

// A run in order splits the rightmost or the leftmost node every time, and a
// delete in order empties one end of the store, so each end borrows and
// merges at every level. A rebind of every key between the runs edits every
// node and must change no shape.
#[test]
fn runs_in_order_from_either_end_agree_with_a_btreemap() {
    let h = TestHeap::new();
    let mut twin = Twin::new(&h);
    for n in 0..1500 {
        twin.put(n, n);
    }
    for n in 0..1500 {
        twin.put(n, -n);
    }
    for n in (750..1500).rev() {
        twin.del(n);
    }
    for n in (-750..0).rev() {
        twin.put(n, n);
    }
    for n in -750..750 {
        twin.del(n);
    }
    assert!(twin.model.is_empty(), "every key is deleted");
}

// A cursor at each end walks toward the other. Each answer comes from its own
// end, the count left shrinks by one per step, and once the cursors meet both
// ends answer nothing, so no entry comes back twice.
#[test]
fn a_read_iterates_from_both_ends_to_the_same_entries() {
    let h = TestHeap::new();
    let mut twin = Twin::new(&h);
    for i in 0..700 {
        twin.put(scattered(i) % 1000, i);
    }
    let read = twin.st.struct_mut_read().expect("an @struct");
    let mut it = read.iter();
    let mut front = twin.model.iter();
    let mut back = twin.model.iter().rev();
    let len = twin.model.len();
    for step in 0..len {
        assert_eq!(it.len(), len - step, "the count left at step {step}");
        if step % 3 == 0 {
            assert_eq!(it.next_back(), back.next(), "from the back at {step}");
        } else {
            assert_eq!(it.next(), front.next(), "from the front at {step}");
        }
    }
    assert_eq!(it.len(), 0);
    assert_eq!(it.next(), None);
    assert_eq!(it.next_back(), None);
}
