// audited: 2026-11-18
// The one order of all values: `Ord` and `Hash` on `Value` agree on kinds, mutable twins, NaN and sets.
//
// docs/types.md
// docs/impl/values.md
//
// tests/lang/order.lisp pins the order a program sees. These tests reach what
// a program cannot build without FFI, and the two sign bits of NaN.

use elle::ffi::types::TypeDesc;
use elle::primitives::ctx::TestHeap;
use elle::value::types::TableKey;
use elle::value::{SymbolId, Value};
use std::cmp::Ordering;
use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};

fn hash_of(v: &Value) -> u64 {
    let mut h = DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

fn ints(xs: &[i64]) -> Vec<Value> {
    xs.iter().map(|x| Value::int(*x)).collect()
}

fn entries(pairs: &[(&str, i64)]) -> BTreeMap<TableKey, Value> {
    pairs
        .iter()
        .map(|(k, v)| (TableKey::keyword(k), Value::int(*v)))
        .collect()
}

fn members(xs: &[i64]) -> BTreeSet<Value> {
    ints(xs).into_iter().collect()
}

/// Each kind's value in its immutable form and in its mutable form, for the
/// five kinds that have both, over the contents `n` names.
fn twins(h: &TestHeap, n: i64) -> Vec<(Value, Value)> {
    let c = h.ctx();
    let text = format!("s{n}");
    vec![
        (c.array(ints(&[n])), c.array_mut(ints(&[n]))),
        (c.string(&text), c.string_mut(text.clone().into_bytes())),
        (c.bytes(vec![n as u8]), c.bytes_mut(vec![n as u8])),
        (
            c.struct_from(entries(&[("a", n)])),
            c.struct_mut_from(entries(&[("a", n)])),
        ),
        (c.set(members(&[n])), c.set_mut(members(&[n]))),
    ]
}

// The counter-factual: each mutable kind held a rank of its own, so `sort`
// put every array before every @array, and a hash read the heap tag.
#[test]
fn a_value_and_its_mutable_twin_share_one_place_and_one_hash() {
    let h = TestHeap::new();
    for (fixed, mutable) in twins(&h, 1) {
        assert_eq!(
            fixed.cmp(&mutable),
            Ordering::Equal,
            "{fixed:?} against its twin"
        );
        assert_eq!(
            mutable.cmp(&fixed),
            Ordering::Equal,
            "{fixed:?}, the twin first"
        );
        assert_eq!(
            hash_of(&fixed),
            hash_of(&mutable),
            "{fixed:?} hashes as its twin"
        );
    }
}

// A twin's contents decide its place against another value of its kind,
// whichever of the two is mutable.
#[test]
fn a_mutable_twin_orders_by_its_contents() {
    let h = TestHeap::new();
    let low = twins(&h, 1);
    let high = twins(&h, 2);
    for ((low_fixed, low_mutable), (high_fixed, high_mutable)) in low.iter().zip(high.iter()) {
        for a in [low_fixed, low_mutable] {
            for b in [high_fixed, high_mutable] {
                assert_eq!(a.cmp(b), Ordering::Less, "{a:?} against {b:?}");
                assert_eq!(b.cmp(a), Ordering::Greater, "{b:?} against {a:?}");
            }
        }
    }
}

// One value of each kind this file can build, in the documented sequence,
// once with the immutable form of each kind that has two and once with the
// mutable form. The closure, the native function, the thread, the fiber, the
// syntax object and the FFI signature are absent: tests/lang/order.lisp
// places the first, second and fourth of them.
#[test]
fn kinds_order_in_the_documented_sequence() {
    let h = TestHeap::new();
    let c = h.ctx();
    for mutable in [false, true] {
        let twin = |i: usize| {
            let (fixed, thawed) = twins(&h, 1)[i];
            if mutable {
                thawed
            } else {
                fixed
            }
        };
        let kinds = vec![
            Value::NIL,
            Value::TRUE,
            Value::int(1),
            Value::symbol(SymbolId::of("a")),
            Value::keyword("k"),
            Value::pointer(0x1000),
            twin(1),
            Value::EMPTY_LIST,
            c.pair(Value::int(1), Value::EMPTY_LIST),
            twin(0),
            twin(2),
            twin(3),
            c.lbox(Value::int(1)),
            c.lib_handle(1),
            c.ffi_type(TypeDesc::I32),
            c.managed_pointer(0x2000),
            c.external("order-test", 1u32),
            c.parameter(Value::NIL),
            twin(4),
        ];
        for (i, a) in kinds.iter().enumerate() {
            for (j, b) in kinds.iter().enumerate() {
                assert_eq!(
                    a.cmp(b),
                    i.cmp(&j),
                    "kind {i} ({a:?}) against kind {j} ({b:?}), mutable forms: {mutable}"
                );
            }
        }
    }
}

// The trap: `f64::total_cmp` puts a NaN with a set sign bit before negative
// infinity and one with a clear sign bit after positive infinity. A division
// of zero by zero gives either, by platform and by tier.
#[test]
fn every_nan_orders_after_every_other_number() {
    let clear = Value::float(f64::from_bits(0x7ff8_0000_0000_0000));
    let set = Value::float(f64::from_bits(0xfff8_0000_0000_0000));
    assert!(clear.as_float().unwrap().is_nan() && set.as_float().unwrap().is_nan());
    assert_ne!(clear, set, "the two NaNs differ in the sign bit");

    // Every other number, in ascending order. The two zeros are two places,
    // and integer zero is the place of 0.0.
    let others = [
        Value::float(f64::NEG_INFINITY),
        Value::int(i64::MIN),
        Value::float(-0.0),
        Value::float(0.0),
        Value::int(i64::MAX),
        Value::float(f64::INFINITY),
    ];
    for (i, a) in others.iter().enumerate() {
        for (j, b) in others.iter().enumerate() {
            assert_eq!(a.cmp(b), i.cmp(&j), "{a:?} against {b:?}");
        }
    }
    assert_eq!(Value::int(0).cmp(&Value::float(0.0)), Ordering::Equal);
    assert_ne!(hash_of(&Value::float(-0.0)), hash_of(&Value::float(0.0)));

    for nan in [clear, set] {
        assert_eq!(nan.cmp(&nan), Ordering::Equal, "a NaN against itself");
        for other in others {
            assert_eq!(other.cmp(&nan), Ordering::Less, "{other:?} against {nan:?}");
            assert_eq!(
                nan.cmp(&other),
                Ordering::Greater,
                "{nan:?} against {other:?}"
            );
        }
        assert_eq!(
            nan.cmp(&Value::keyword("k")),
            Ordering::Less,
            "a NaN is a number"
        );
    }
    assert_ne!(
        clear.cmp(&set),
        Ordering::Equal,
        "two NaNs of two bit patterns are two keys, as equality has them"
    );
    assert_eq!(
        clear.cmp(&set),
        set.cmp(&clear).reverse(),
        "and the order of two NaNs is antisymmetric"
    );
}

// The counter-factual: `Hash` had no arm for an immutable set, so a set
// hashed its address. Two equal sets hashed apart, and two unequal sets built
// one after the other at one address hashed alike.
#[test]
fn a_set_hashes_by_its_members() {
    let h = TestHeap::new();
    let c = h.ctx();
    let a = c.set(members(&[1, 2]));
    let b = c.set(members(&[1, 2]));
    let other = c.set(members(&[1, 3]));
    assert_eq!(a, b, "two sets of the same members are equal");
    assert_eq!(hash_of(&a), hash_of(&b), "so they hash alike");
    assert_ne!(
        hash_of(&a),
        hash_of(&other),
        "and a set of other members hashes apart"
    );
    assert_eq!(
        hash_of(&a),
        hash_of(&c.set_mut(members(&[1, 2]))),
        "an @set hashes as the set of its members"
    );
}

// A value inside a collection is hashed and ordered as it is outside one, so
// a twin nested at any depth changes neither.
#[test]
fn a_nested_twin_changes_neither_the_place_nor_the_hash() {
    let h = TestHeap::new();
    let c = h.ctx();
    for (fixed, mutable) in twins(&h, 1) {
        let outer_fixed = c.array(vec![Value::int(0), fixed]);
        let outer_mutable = c.array_mut(vec![Value::int(0), mutable]);
        assert_eq!(
            outer_fixed.cmp(&outer_mutable),
            Ordering::Equal,
            "{fixed:?} in an array"
        );
        assert_eq!(
            hash_of(&outer_fixed),
            hash_of(&outer_mutable),
            "{fixed:?} in an array"
        );
        let in_list = c.pair(fixed, Value::EMPTY_LIST);
        let twin_in_list = c.pair(mutable, Value::EMPTY_LIST);
        assert_eq!(
            in_list.cmp(&twin_in_list),
            Ordering::Equal,
            "{fixed:?} in a list"
        );
        assert_eq!(
            hash_of(&in_list),
            hash_of(&twin_in_list),
            "{fixed:?} in a list"
        );
    }
}
