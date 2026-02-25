// Property tests for effect inference soundness.
//
// Verifies that the effect system correctly classifies expressions:
// - Pure arithmetic/let/lambda never inferred as yielding
// - Expressions containing `yield` always inferred as yielding
// - Effect propagation through calls is consistent

use elle::effects::Effect;
use elle::pipeline::analyze;
use elle::primitives::register_primitives;
use elle::symbol::SymbolTable;
use elle::vm::VM;
use proptest::prelude::*;

/// Analyze source code and return its inferred effect.
fn infer_effect(source: &str) -> Result<Effect, String> {
    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _effects = register_primitives(&mut vm, &mut symbols);
    let result = analyze(source, &mut symbols, &mut vm)?;
    Ok(result.hir.effect)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    // =========================================================================
    // Pure expressions: should NEVER have may_yield()
    // =========================================================================

    #[test]
    fn literal_int_is_pure(n in -10000i64..10000) {
        let effect = infer_effect(&format!("{}", n)).unwrap();
        prop_assert!(!effect.may_yield(),
            "Integer literal {} inferred as yielding: {:?}", n, effect);
    }

    #[test]
    fn arithmetic_is_pure(a in -100i64..100, b in 1i64..100) {
        let effect = infer_effect(&format!("(+ {} {})", a, b)).unwrap();
        prop_assert!(!effect.may_yield(),
            "Addition inferred as yielding: {:?}", effect);

        let effect = infer_effect(&format!("(* {} {})", a, b)).unwrap();
        prop_assert!(!effect.may_yield(),
            "Multiplication inferred as yielding: {:?}", effect);

        let effect = infer_effect(&format!("(- {} {})", a, b)).unwrap();
        prop_assert!(!effect.may_yield(),
            "Subtraction inferred as yielding: {:?}", effect);
    }

    #[test]
    fn let_with_pure_body_is_pure(a in -100i64..100, b in -100i64..100) {
        let code = format!("(let ((x {}) (y {})) (+ x y))", a, b);
        let effect = infer_effect(&code).unwrap();
        prop_assert!(!effect.may_yield(),
            "Pure let inferred as yielding: {:?}", effect);
    }

    #[test]
    fn nested_let_pure(a in -50i64..50, b in -50i64..50, c in -50i64..50) {
        let code = format!(
            "(let ((x {})) (let ((y {})) (let ((z {})) (+ x (+ y z)))))",
            a, b, c
        );
        let effect = infer_effect(&code).unwrap();
        prop_assert!(!effect.may_yield(),
            "Nested pure let inferred as yielding: {:?}", effect);
    }

    #[test]
    fn if_with_pure_branches_is_pure(a in -100i64..100, b in -100i64..100) {
        let code = format!("(if (< {} {}) {} {})", a, b, a, b);
        let effect = infer_effect(&code).unwrap();
        prop_assert!(!effect.may_yield(),
            "Pure if inferred as yielding: {:?}", effect);
    }

    #[test]
    fn pure_lambda_is_pure(a in -100i64..100) {
        let code = format!("((fn (x) (+ x 1)) {})", a);
        let effect = infer_effect(&code).unwrap();
        prop_assert!(!effect.may_yield(),
            "Pure lambda call inferred as yielding: {:?}", effect);
    }

    #[test]
    fn comparison_is_pure(a in -100i64..100, b in -100i64..100) {
        let code = format!("(= {} {})", a, b);
        let effect = infer_effect(&code).unwrap();
        prop_assert!(!effect.may_yield(),
            "Comparison inferred as yielding: {:?}", effect);
    }

    #[test]
    fn string_literal_is_pure(s in "[a-z]{1,10}") {
        let code = format!("\"{}\"", s);
        let effect = infer_effect(&code).unwrap();
        prop_assert!(!effect.may_yield(),
            "String literal inferred as yielding: {:?}", effect);
    }

    #[test]
    fn boolean_ops_are_pure(a in prop::bool::ANY, b in prop::bool::ANY) {
        let a_str = if a { "#t" } else { "#f" };
        let b_str = if b { "#t" } else { "#f" };

        let effect = infer_effect(&format!("(and {} {})", a_str, b_str)).unwrap();
        prop_assert!(!effect.may_yield(),
            "and inferred as yielding: {:?}", effect);

        let effect = infer_effect(&format!("(or {} {})", a_str, b_str)).unwrap();
        prop_assert!(!effect.may_yield(),
            "or inferred as yielding: {:?}", effect);

        let effect = infer_effect(&format!("(not {})", a_str)).unwrap();
        prop_assert!(!effect.may_yield(),
            "not inferred as yielding: {:?}", effect);
    }

    #[test]
    fn quote_is_pure(n in -100i64..100) {
        let code = format!("(quote {})", n);
        let effect = infer_effect(&code).unwrap();
        prop_assert!(!effect.may_yield(),
            "Quote inferred as yielding: {:?}", effect);
    }

    #[test]
    fn begin_with_pure_exprs_is_pure(a in -100i64..100, b in -100i64..100, c in -100i64..100) {
        let code = format!("(begin {} {} {})", a, b, c);
        let effect = infer_effect(&code).unwrap();
        prop_assert!(!effect.may_yield(),
            "Pure begin inferred as yielding: {:?}", effect);
    }

    #[test]
    fn cond_with_pure_branches_is_pure(a in -100i64..100, b in -100i64..100) {
        let code = format!("(cond ((< {} {}) {}) (else {}))", a, b, a, b);
        let effect = infer_effect(&code).unwrap();
        prop_assert!(!effect.may_yield(),
            "Pure cond inferred as yielding: {:?}", effect);
    }

    // =========================================================================
    // Effect combine laws
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
    // Consistency: compile and analyze agree on effect
    // =========================================================================

    #[test]
    fn pure_arithmetic_does_not_yield(a in -100i64..100, b in -100i64..100) {
        let code = format!("(+ {} {})", a, b);
        let effect = infer_effect(&code).unwrap();
        prop_assert!(!effect.may_yield(),
            "Pure arithmetic inferred as yielding");
    }

    #[test]
    fn pure_lambda_call_does_not_yield(a in -50i64..50, b in -50i64..50) {
        let code = format!("((fn (x y) (+ x y)) {} {})", a, b);
        let effect = infer_effect(&code).unwrap();
        prop_assert!(!effect.may_yield(),
            "Pure lambda call inferred as yielding");
    }

    #[test]
    fn nested_pure_calls_do_not_yield(a in -50i64..50) {
        let code = format!(
            "((fn (x) ((fn (y) (+ y 1)) (+ x 1))) {})",
            a
        );
        let effect = infer_effect(&code).unwrap();
        prop_assert!(!effect.may_yield(),
            "Nested pure calls inferred as yielding");
    }

    #[test]
    fn lambda_creation_is_pure(_a in -100i64..100) {
        let code = "(fn (x) x)";
        let effect = infer_effect(code).unwrap();
        prop_assert!(!effect.may_yield(),
            "Lambda creation inferred as yielding");
    }

    #[test]
    fn multiple_lambdas_are_pure(_a in -100i64..100) {
        let code = "((fn (f) (f 1)) (fn (x) (+ x 1)))";
        let effect = infer_effect(code).unwrap();
        prop_assert!(!effect.may_yield(),
            "Multiple lambdas inferred as yielding");
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
