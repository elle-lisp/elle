// Property tests for Value's Eq, Hash, and Ord trait implementations.
//
// Verifies the fundamental invariants:
// - Hash/Eq consistency: a == b → hash(a) == hash(b)
// - Ord/Eq consistency: (a == b) ↔ (a.cmp(&b) == Equal)
// - Ord reflexivity: a.cmp(&a) == Equal
// - Ord antisymmetry: a.cmp(&b) == b.cmp(&a).reverse()
// - Ord transitivity: a ≤ b ∧ b ≤ c → a ≤ c

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
    #![proptest_config(ProptestConfig::with_cases(1000))]

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
