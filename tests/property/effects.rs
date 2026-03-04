// Property tests for effect combine laws and effect predicates.
//
// Verifies that the effect system satisfies algebraic laws:
// - Effect combine is commutative, associative, and idempotent
// - Effect::none() is the identity element
// - Propagates field is correctly ORed during combine
// - Effect predicates (may_yield, may_raise, may_suspend, etc.) work correctly

use elle::effects::Effect;
use proptest::prelude::*;

proptest! {
    #![proptest_config(crate::common::proptest_cases(1000))]

    // =========================================================================
    // Effect combine laws (pure Rust, cheap algebraic properties)
    // =========================================================================

    #[test]
    fn effect_combine_commutative(
        a_bits in 0u32..8,
        b_bits in 0u32..8,
    ) {
        let a = Effect { bits: a_bits, propagates: 0 };
        let b = Effect { bits: b_bits, propagates: 0 };
        prop_assert_eq!(a.combine(b), b.combine(a),
            "Effect combine is not commutative");
    }

    #[test]
    fn effect_combine_associative(
        a_bits in 0u32..8,
        b_bits in 0u32..8,
        c_bits in 0u32..8,
    ) {
        let a = Effect { bits: a_bits, propagates: 0 };
        let b = Effect { bits: b_bits, propagates: 0 };
        let c = Effect { bits: c_bits, propagates: 0 };
        prop_assert_eq!(
            a.combine(b).combine(c),
            a.combine(b.combine(c)),
            "Effect combine is not associative"
        );
    }

    #[test]
    fn effect_combine_identity(bits in 0u32..16) {
        let e = Effect { bits, propagates: 0 };
        prop_assert_eq!(e.combine(Effect::none()), e,
            "Effect::none() is not identity for combine");
        prop_assert_eq!(Effect::none().combine(e), e,
            "Effect::none() is not left identity for combine");
    }

    #[test]
    fn effect_combine_idempotent(bits in 0u32..16) {
        let e = Effect { bits, propagates: 0 };
        prop_assert_eq!(e.combine(e), e,
            "Effect combine is not idempotent");
    }

    #[test]
    fn effect_propagates_combine(
        a_prop in 0u32..256,
        b_prop in 0u32..256,
    ) {
        let a = Effect { bits: 0, propagates: a_prop };
        let b = Effect { bits: 0, propagates: b_prop };
        let combined = a.combine(b);
        // Propagates should be ORed
        prop_assert_eq!(combined.propagates, a_prop | b_prop,
            "Propagates not ORed correctly");
    }

    // =========================================================================
    // Polymorphic effects
    // =========================================================================

    #[test]
    fn polymorphic_effect_is_polymorphic(param in 0usize..8) {
        let effect = Effect::polymorphic(param);
        prop_assert!(effect.is_polymorphic(),
            "Polymorphic effect not marked as polymorphic");
        prop_assert!(effect.may_suspend(),
            "Polymorphic effect should may_suspend");
    }

    #[test]
    fn polymorphic_propagates_correct_param(param in 0usize..8) {
        let effect = Effect::polymorphic(param);
        let propagated: Vec<_> = effect.propagated_params().collect();
        prop_assert_eq!(propagated.len(), 1, "Should propagate exactly one param");
        prop_assert_eq!(propagated[0], param, "Should propagate param {}", param);
    }

    #[test]
    fn polymorphic_raises_has_error_bit(param in 0usize..8) {
        let effect = Effect::polymorphic_raises(param);
        prop_assert!(effect.may_raise(),
            "Polymorphic_raises should have error bit");
        prop_assert!(effect.is_polymorphic(),
            "Polymorphic_raises should be polymorphic");
    }

    // =========================================================================
    // Effect predicates
    // =========================================================================

    #[test]
    fn none_effect_is_not_yielding(_x in 0u32..1) {
        let effect = Effect::none();
        prop_assert!(!effect.may_yield());
        prop_assert!(!effect.may_raise());
        prop_assert!(!effect.may_suspend());
    }

    #[test]
    fn yields_effect_may_yield(_x in 0u32..1) {
        let effect = Effect::yields();
        prop_assert!(effect.may_yield());
        prop_assert!(effect.may_suspend());
    }

    #[test]
    fn raises_effect_may_raise(_x in 0u32..1) {
        let effect = Effect::raises();
        prop_assert!(effect.may_raise());
        prop_assert!(!effect.may_yield());
    }

    #[test]
    fn yields_raises_has_both(_x in 0u32..1) {
        let effect = Effect::yields_raises();
        prop_assert!(effect.may_yield());
        prop_assert!(effect.may_raise());
        prop_assert!(effect.may_suspend());
    }

    #[test]
    fn ffi_effect_may_ffi(_x in 0u32..1) {
        let effect = Effect::ffi();
        prop_assert!(effect.may_ffi());
    }

    #[test]
    fn halts_effect_may_halt(_x in 0u32..1) {
        let effect = Effect::halts();
        prop_assert!(effect.may_halt());
        prop_assert!(effect.may_raise());
    }
}
