// audited: 2026-11-25
// `Eq`, `Hash` and `Ord` on `Value` agree, and `Ord` is a total order, over generated values.
//
// docs/types.md
//
// The invariants:
// - Hash/Eq consistency: a == b → hash(a) == hash(b)
// - Ord/Eq consistency: (a == b) ↔ (a.cmp(&b) == Equal)
// - Ord reflexivity: a.cmp(&a) == Equal
// - Ord antisymmetry: a.cmp(&b) == b.cmp(&a).reverse()
// - Ord transitivity: a ≤ b ∧ b ≤ c → a ≤ c
// - Ord/Hash consistency: a.cmp(&b) == Equal → hash(a) == hash(b)
// - A value shares its place and its hash with its mutable twin

use proptest::prelude::*;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use super::strategies::arb_value;
use elle::Value;

fn hash_value(v: &Value) -> u64 {
    let mut h = DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

proptest! {
    #![proptest_config(crate::common::proptest_cases(1000))]

    #[test]
    fn hash_eq_consistency(a in arb_value(), b in arb_value()) {
        if a == b {
            prop_assert_eq!(
                hash_value(&a), hash_value(&b),
                "Equal values must have equal hashes: a={:?}, b={:?}", a, b
            );
        }
    }

    #[test]
    fn ord_eq_consistency(a in arb_value(), b in arb_value()) {
        prop_assert_eq!(
            a == b,
            a.cmp(&b) == std::cmp::Ordering::Equal,
            "Eq and Ord must agree: a={:?}, b={:?}, eq={}, cmp={:?}",
            a, b, a == b, a.cmp(&b)
        );
    }

    #[test]
    fn ord_reflexive(a in arb_value()) {
        prop_assert_eq!(
            a.cmp(&a), std::cmp::Ordering::Equal,
            "Ord must be reflexive: a={:?}", a
        );
    }

    #[test]
    fn ord_antisymmetric(a in arb_value(), b in arb_value()) {
        let ab = a.cmp(&b);
        let ba = b.cmp(&a);
        prop_assert_eq!(
            ab, ba.reverse(),
            "Ord must be antisymmetric: a={:?}, b={:?}, a.cmp(b)={:?}, b.cmp(a)={:?}",
            a, b, ab, ba
        );
    }

    #[test]
    fn ord_transitive(
        a in arb_value(),
        b in arb_value(),
        c in arb_value()
    ) {
        let ab = a.cmp(&b);
        let bc = b.cmp(&c);
        let ac = a.cmp(&c);

        // If a ≤ b and b ≤ c, then a ≤ c
        if ab != std::cmp::Ordering::Greater && bc != std::cmp::Ordering::Greater {
            prop_assert_ne!(
                ac, std::cmp::Ordering::Greater,
                "Ord must be transitive: a={:?}, b={:?}, c={:?}, \
                 a.cmp(b)={:?}, b.cmp(c)={:?}, a.cmp(c)={:?}",
                a, b, c, ab, bc, ac
            );
        }
    }
}

proptest! {
    #![proptest_config(crate::common::proptest_cases(1000))]

    // Two values at one place in the order are one key, so a table that finds
    // a key by its hash must find it. Each case hashes a value against a copy
    // of itself with every array made mutable, which shares its place.
    #[test]
    fn ord_hash_consistency(a in arb_value(), b in arb_value()) {
        if a.cmp(&b) == std::cmp::Ordering::Equal {
            prop_assert_eq!(
                hash_value(&a), hash_value(&b),
                "Values at one place in the order must hash alike: a={:?}, b={:?}", a, b
            );
        }
        let twin = thawed(&a);
        prop_assert_eq!(
            a.cmp(&twin), std::cmp::Ordering::Equal,
            "A value and its mutable twin share one place: a={:?}", a
        );
        prop_assert_eq!(
            hash_value(&a), hash_value(&twin),
            "A value and its mutable twin hash alike: a={:?}", a
        );
    }
}

/// `v` with every array in it, at any depth of arrays and list pairs, rebuilt
/// as an `@array`, and every string as an `@string`.
fn thawed(v: &Value) -> Value {
    let h = elle::primitives::ctx::TestHeap::new();
    if let Some(elements) = v.as_array() {
        h.ctx()
            .array_mut(elements.iter().map(thawed).collect::<Vec<_>>())
    } else if let Some(pair) = v.as_pair() {
        h.ctx().pair(thawed(&pair.first), thawed(&pair.rest))
    } else if let Some(text) = v.as_str() {
        h.ctx().string_mut(text.as_bytes().to_vec())
    } else {
        *v
    }
}
