// audited: 2026-10-23
// An @set's funnels and reads agree with a BTreeSet through every shape its store takes.
//
// docs/impl/mutable.md
//
// The counter-factual: a tree edit that drops, repeats or misplaces an entry
// passes a small test, because a set of a few dozen members fits one or two
// nodes. These runs reach a few thousand members, so the store splits and
// merges at every level, borrows from both siblings, and collapses its root.
// Every answer, every length and every state is checked against a `BTreeSet`
// after each step, so the first edit that goes wrong names its step.

use std::collections::BTreeSet;

use elle::primitives::ctx::TestHeap;
use elle::value::arena::{set_add_with_incref, set_del_with_decref};
use elle::Value;

/// The `i`th key in a scattered order. 7919 is prime to the modulus, so the
/// first million keys are distinct.
fn scattered(i: i64) -> i64 {
    i * 7919 % 1_000_003
}

/// The member for `key`: a string for every third key, built afresh on each
/// call, so a delete finds it by value, not by identity.
fn member(h: &TestHeap, key: i64) -> Value {
    if key % 3 == 0 {
        h.ctx().string(format!("k{key}"))
    } else {
        Value::int(key)
    }
}

/// An @set beside the `BTreeSet` it must agree with.
struct Twin<'h> {
    h: &'h TestHeap,
    set: Value,
    model: BTreeSet<Value>,
}

impl<'h> Twin<'h> {
    fn new(h: &'h TestHeap) -> Self {
        Twin {
            h,
            set: h.ctx().set_mut(BTreeSet::new()),
            model: BTreeSet::new(),
        }
    }

    fn add(&mut self, key: i64) {
        let m = member(self.h, key);
        let stored = set_add_with_incref(self.h.heap(), self.set, m);
        assert_eq!(stored, self.model.insert(m), "the add of {key}");
        self.agree(key);
    }

    fn del(&mut self, key: i64) {
        let m = member(self.h, key);
        let removed = set_del_with_decref(self.h.heap(), self.set, &m);
        assert_eq!(removed, self.model.remove(&m), "the delete of {key}");
        self.agree(key);
    }

    /// The store's length and members in both directions, after the step
    /// that wrote `key`.
    fn agree(&self, key: i64) {
        let read = self.set.set_mut_read().expect("an @set");
        assert_eq!(read.len(), self.model.len(), "the length after {key}");
        assert_eq!(read.is_empty(), self.model.is_empty());
        assert!(
            read.iter().eq(self.model.iter()),
            "the members ascending after {key}"
        );
        assert!(
            read.iter().rev().eq(self.model.iter().rev()),
            "the members descending after {key}"
        );
    }

    /// Membership of every key below `n`, present or not.
    fn answers_membership_below(&self, n: i64) {
        let read = self.set.set_mut_read().expect("an @set");
        for key in 0..n {
            let m = member(self.h, key);
            assert_eq!(read.contains(&m), self.model.contains(&m), "{key}");
        }
    }
}

#[test]
fn scattered_adds_and_heavy_deletes_agree_with_a_btreeset() {
    let h = TestHeap::new();
    let mut twin = Twin::new(&h);
    let keys: Vec<i64> = (0..2000).map(|i| scattered(i) % 3000).collect();
    for (i, &key) in keys.iter().enumerate() {
        twin.add(key);
        if i % 7 == 6 {
            twin.add(keys[i / 2]);
        }
    }
    twin.answers_membership_below(3000);
    for i in 0..1900 {
        twin.del(keys[i * 1237 % 2000]);
        if i % 11 == 10 {
            twin.del(3000 + i as i64);
        }
    }
    twin.answers_membership_below(3000);
    for &key in &keys[..1000] {
        twin.add(key);
    }
    for &key in &keys {
        twin.del(key);
    }
    assert!(twin.model.is_empty(), "every key is deleted");
    twin.add(7);
    twin.answers_membership_below(3000);
}

// A run in order splits the rightmost or the leftmost node every time, and a
// delete in order empties one end of the store, so each end borrows and
// merges at every level.
#[test]
fn runs_in_order_from_either_end_agree_with_a_btreeset() {
    let h = TestHeap::new();
    let mut twin = Twin::new(&h);
    for key in 0..1500 {
        twin.add(key);
    }
    for key in (750..1500).rev() {
        twin.del(key);
    }
    for key in (-750..0).rev() {
        twin.add(key);
    }
    for key in -750..750 {
        twin.del(key);
    }
    assert!(twin.model.is_empty(), "every key is deleted");
}

// A cursor at each end walks toward the other. Each answer comes from its own
// end, the count left shrinks by one per step, and once the cursors meet both
// ends answer nothing, so no member comes back twice.
#[test]
fn a_read_iterates_from_both_ends_to_the_same_members() {
    let h = TestHeap::new();
    let mut twin = Twin::new(&h);
    for i in 0..700 {
        twin.add(scattered(i) % 1000);
    }
    let read = twin.set.set_mut_read().expect("an @set");
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
