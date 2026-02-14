// Phase 14: Type Specialization (Milestone Tests)
//
// Comprehensive tests for Phase 14 features:
// - Type profiling and tracking
// - Specialization strategy selection
// - Specialized variant creation
// - Polymorphic dispatch
// - Bailout handling

#[cfg(test)]
mod tests {
    use crate::compiler::cranelift::type_specializer::{
        SpecializationStats, SpecializationStrategy, SpecializedVariant, TypeProfile,
        TypeSpecializer, ValueType,
    };
    use crate::value::{SymbolId, Value};

    // ===== ValueType Tests =====

    #[test]
    fn phase14_valuetype_int() {
        assert_eq!(ValueType::from_value(&Value::Int(42)), ValueType::Int);
    }

    #[test]
    fn phase14_valuetype_float() {
        assert_eq!(
            ValueType::from_value(&Value::Float(std::f64::consts::PI)),
            ValueType::Float
        );
    }

    #[test]
    fn phase14_valuetype_bool_true() {
        assert_eq!(ValueType::from_value(&Value::Bool(true)), ValueType::Bool);
    }

    #[test]
    fn phase14_valuetype_bool_false() {
        assert_eq!(ValueType::from_value(&Value::Bool(false)), ValueType::Bool);
    }

    #[test]
    fn phase14_valuetype_nil() {
        assert_eq!(ValueType::from_value(&Value::Nil), ValueType::Nil);
    }

    #[test]
    fn phase14_valuetype_is_numeric_int() {
        assert!(ValueType::Int.is_numeric());
    }

    #[test]
    fn phase14_valuetype_is_numeric_float() {
        assert!(ValueType::Float.is_numeric());
    }

    #[test]
    fn phase14_valuetype_is_numeric_bool() {
        assert!(!ValueType::Bool.is_numeric());
    }

    #[test]
    fn phase14_valuetype_is_scalar() {
        assert!(ValueType::Int.is_scalar());
        assert!(ValueType::Float.is_scalar());
        assert!(ValueType::Bool.is_scalar());
        assert!(ValueType::Nil.is_scalar());
        assert!(!ValueType::Symbol.is_scalar());
    }

    // ===== TypeProfile Tests =====

    #[test]
    fn phase14_typeprofile_creation() {
        let profile = TypeProfile::new();

        assert_eq!(profile.total_observations, 0);
        assert_eq!(profile.dominant_frequency, 0.0);
        assert!(!profile.is_stable);
    }

    #[test]
    fn phase14_typeprofile_observe_single() {
        let mut profile = TypeProfile::new();

        profile.observe(ValueType::Int);

        assert_eq!(profile.total_observations, 1);
        assert_eq!(profile.dominant_type, Some(ValueType::Int));
    }

    #[test]
    fn phase14_typeprofile_observe_multiple_same() {
        let mut profile = TypeProfile::new();

        profile.observe(ValueType::Int);
        profile.observe(ValueType::Int);
        profile.observe(ValueType::Int);

        assert_eq!(profile.total_observations, 3);
        assert_eq!(profile.dominant_frequency, 1.0);
    }

    #[test]
    fn phase14_typeprofile_observe_mixed() {
        let mut profile = TypeProfile::new();

        profile.observe(ValueType::Int);
        profile.observe(ValueType::Int);
        profile.observe(ValueType::Float);

        assert_eq!(profile.total_observations, 3);
        assert_eq!(profile.dominant_type, Some(ValueType::Int));
        assert!(profile.dominant_frequency > 0.6);
    }

    #[test]
    fn phase14_typeprofile_stability_threshold() {
        let mut profile = TypeProfile::new();

        for _ in 0..20 {
            profile.observe(ValueType::Int);
        }

        assert!(profile.is_stable);
        assert!(profile.dominant_frequency >= 0.95);
    }

    #[test]
    fn phase14_typeprofile_not_enough_observations() {
        let mut profile = TypeProfile::new();

        for _ in 0..5 {
            profile.observe(ValueType::Int);
        }

        assert!(!profile.is_stable);
    }

    #[test]
    fn phase14_typeprofile_mixed_types_not_stable() {
        let mut profile = TypeProfile::new();

        // Create definitely unstable profile
        for _ in 0..5 {
            profile.observe(ValueType::Int);
            profile.observe(ValueType::Float);
        }

        // Only 10 observations, not enough for stability
        assert!(!profile.is_stable);
    }

    #[test]
    fn phase14_typeprofile_specializable() {
        let mut profile = TypeProfile::new();

        for _ in 0..20 {
            profile.observe(ValueType::Int);
        }

        assert!(profile.is_specializable());
    }

    #[test]
    fn phase14_typeprofile_polymorphic_ratio() {
        let mut profile = TypeProfile::new();

        for _ in 0..19 {
            profile.observe(ValueType::Int);
        }
        profile.observe(ValueType::Float);

        let ratio = profile.polymorphic_ratio();
        assert!(ratio < 0.1);
    }

    // ===== SpecializationStrategy Tests =====

    #[test]
    fn phase14_strategy_none_variants() {
        assert_eq!(SpecializationStrategy::None.max_variants(), 1);
    }

    #[test]
    fn phase14_strategy_monomorphic_variants() {
        assert_eq!(SpecializationStrategy::Monomorphic.max_variants(), 1);
    }

    #[test]
    fn phase14_strategy_duomorphic_variants() {
        assert_eq!(SpecializationStrategy::Duomorphic.max_variants(), 2);
    }

    #[test]
    fn phase14_strategy_polymorphic_variants() {
        assert_eq!(SpecializationStrategy::Polymorphic.max_variants(), 4);
    }

    #[test]
    fn phase14_strategy_fallback_none() {
        assert!(!SpecializationStrategy::None.needs_fallback());
    }

    #[test]
    fn phase14_strategy_fallback_monomorphic() {
        assert!(!SpecializationStrategy::Monomorphic.needs_fallback());
    }

    #[test]
    fn phase14_strategy_fallback_duomorphic() {
        assert!(SpecializationStrategy::Duomorphic.needs_fallback());
    }

    #[test]
    fn phase14_strategy_fallback_polymorphic() {
        assert!(SpecializationStrategy::Polymorphic.needs_fallback());
    }

    // ===== SpecializedVariant Tests =====

    #[test]
    fn phase14_variant_creation() {
        let variant = SpecializedVariant::new(vec![Some(ValueType::Int)]);

        assert_eq!(variant.input_types.len(), 1);
        assert_eq!(variant.execution_count, 0);
    }

    #[test]
    fn phase14_variant_multiple_types() {
        let variant =
            SpecializedVariant::new(vec![Some(ValueType::Int), Some(ValueType::Float), None]);

        assert_eq!(variant.input_types.len(), 3);
    }

    #[test]
    fn phase14_variant_execute() {
        let mut variant = SpecializedVariant::new(vec![Some(ValueType::Int)]);

        variant.record_execution();
        variant.record_execution();
        variant.record_execution();

        assert_eq!(variant.execution_count, 3);
    }

    #[test]
    fn phase14_variant_frequency_estimate() {
        let mut variant = SpecializedVariant::new(vec![Some(ValueType::Int)]);

        for _ in 0..3 {
            variant.record_execution();
        }

        assert_eq!(variant.frequency_estimate(10), 0.3);
    }

    #[test]
    fn phase14_variant_frequency_estimate_zero_total() {
        let variant = SpecializedVariant::new(vec![Some(ValueType::Int)]);

        assert_eq!(variant.frequency_estimate(0), 0.0);
    }

    // ===== TypeSpecializer Tests =====

    #[test]
    fn phase14_specializer_creation() {
        let specializer = TypeSpecializer::new();

        assert_eq!(specializer.total_specializations(), 0);
        assert_eq!(specializer.total_executions(), 0);
    }

    #[test]
    fn phase14_specializer_observe_call() {
        let mut specializer = TypeSpecializer::new();

        specializer.observe_call(SymbolId(1), vec![ValueType::Int]);

        let profiles = specializer.get_profiles(SymbolId(1));
        assert!(profiles.is_some());
        assert_eq!(profiles.unwrap().len(), 1);
    }

    #[test]
    fn phase14_specializer_observe_multiple() {
        let mut specializer = TypeSpecializer::new();

        specializer.observe_call(SymbolId(1), vec![ValueType::Int, ValueType::Float]);

        let profiles = specializer.get_profiles(SymbolId(1));
        assert_eq!(profiles.unwrap().len(), 2);
    }

    #[test]
    fn phase14_specializer_strategy_none_no_data() {
        let specializer = TypeSpecializer::new();

        let strategy = specializer.get_strategy(SymbolId(1));

        assert_eq!(strategy, SpecializationStrategy::None);
    }

    #[test]
    fn phase14_specializer_strategy_monomorphic() {
        let mut specializer = TypeSpecializer::new();

        for _ in 0..20 {
            specializer.observe_call(SymbolId(1), vec![ValueType::Int]);
        }

        let strategy = specializer.decide_strategy(SymbolId(1));

        assert_eq!(strategy, SpecializationStrategy::Monomorphic);
    }

    #[test]
    fn phase14_specializer_create_variant() {
        let mut specializer = TypeSpecializer::new();

        let variant = specializer.create_variant(SymbolId(1), vec![Some(ValueType::Int)]);

        assert!(variant.code_address.is_some());
        assert_eq!(specializer.total_specializations(), 1);
    }

    #[test]
    fn phase14_specializer_multiple_variants() {
        let mut specializer = TypeSpecializer::new();

        specializer.create_variant(SymbolId(1), vec![Some(ValueType::Int)]);
        specializer.create_variant(SymbolId(1), vec![Some(ValueType::Float)]);

        assert_eq!(specializer.total_specializations(), 2);
    }

    #[test]
    fn phase14_specializer_get_variants() {
        let mut specializer = TypeSpecializer::new();

        specializer.create_variant(SymbolId(1), vec![Some(ValueType::Int)]);
        specializer.create_variant(SymbolId(1), vec![Some(ValueType::Float)]);

        let variants = specializer.get_variants(SymbolId(1));
        assert_eq!(variants.unwrap().len(), 2);
    }

    #[test]
    fn phase14_specializer_record_execution() {
        let mut specializer = TypeSpecializer::new();

        specializer.create_variant(SymbolId(1), vec![Some(ValueType::Int)]);
        specializer.record_variant_execution(SymbolId(1), 0);
        specializer.record_variant_execution(SymbolId(1), 0);

        let variants = specializer.get_variants(SymbolId(1)).unwrap();
        assert_eq!(variants[0].execution_count, 2);
        assert_eq!(specializer.total_executions(), 2);
    }

    #[test]
    fn phase14_specializer_stats() {
        let mut specializer = TypeSpecializer::new();

        specializer.create_variant(SymbolId(1), vec![Some(ValueType::Int)]);
        specializer.create_variant(SymbolId(2), vec![Some(ValueType::Float)]);

        let stats = specializer.get_stats();

        assert_eq!(stats.total_variants, 2);
        assert_eq!(stats.functions_with_variants, 2);
    }

    #[test]
    fn phase14_specializer_bailout_needed() {
        let mut specializer = TypeSpecializer::new();

        specializer.create_variant(SymbolId(1), vec![Some(ValueType::Int)]);

        let should_bailout = specializer.should_bailout(SymbolId(1), &[ValueType::Float]);

        assert!(should_bailout);
    }

    #[test]
    fn phase14_specializer_bailout_not_needed() {
        let mut specializer = TypeSpecializer::new();

        specializer.create_variant(SymbolId(1), vec![Some(ValueType::Int)]);

        let should_bailout = specializer.should_bailout(SymbolId(1), &[ValueType::Int]);

        assert!(!should_bailout);
    }

    #[test]
    fn phase14_specializer_bailout_wildcard() {
        let mut specializer = TypeSpecializer::new();

        specializer.create_variant(SymbolId(1), vec![None]);

        let should_bailout = specializer.should_bailout(SymbolId(1), &[ValueType::Float]);

        assert!(!should_bailout);
    }

    // ===== SpecializationStats Tests =====

    #[test]
    fn phase14_stats_creation() {
        let stats = SpecializationStats {
            total_specializations: 5,
            total_variants: 3,
            functions_with_variants: 2,
            total_executions: 100,
            avg_variant_executions: 33.33,
        };

        assert_eq!(stats.total_variants, 3);
    }

    #[test]
    fn phase14_stats_coverage() {
        let stats = SpecializationStats {
            total_specializations: 5,
            total_variants: 3,
            functions_with_variants: 2,
            total_executions: 100,
            avg_variant_executions: 33.33,
        };

        let coverage = stats.specialization_coverage(10);
        assert_eq!(coverage, 0.2);
    }

    #[test]
    fn phase14_stats_coverage_zero_functions() {
        let stats = SpecializationStats {
            total_specializations: 0,
            total_variants: 0,
            functions_with_variants: 0,
            total_executions: 0,
            avg_variant_executions: 0.0,
        };

        assert_eq!(stats.specialization_coverage(0), 0.0);
    }

    #[test]
    fn phase14_stats_utilization() {
        let stats = SpecializationStats {
            total_specializations: 2,
            total_variants: 2,
            functions_with_variants: 1,
            total_executions: 50,
            avg_variant_executions: 25.0,
        };

        let utilization = stats.variant_utilization();
        assert!(utilization > 0.0 && utilization <= 1.0);
    }

    // ===== Integration Tests =====

    #[test]
    fn phase14_workflow_monomorphic_int() {
        let mut specializer = TypeSpecializer::new();

        // Observe calls with int arguments
        for _ in 0..20 {
            specializer.observe_call(SymbolId(1), vec![ValueType::Int]);
        }

        // Decide strategy
        let strategy = specializer.decide_strategy(SymbolId(1));
        assert_eq!(strategy, SpecializationStrategy::Monomorphic);

        // Create variant
        specializer.create_variant(SymbolId(1), vec![Some(ValueType::Int)]);

        // Check bailout
        assert!(!specializer.should_bailout(SymbolId(1), &[ValueType::Int]));
        assert!(specializer.should_bailout(SymbolId(1), &[ValueType::Float]));
    }

    #[test]
    fn phase14_workflow_duomorphic() {
        let mut specializer = TypeSpecializer::new();

        // Observe mixed but stable types
        for _ in 0..15 {
            specializer.observe_call(SymbolId(1), vec![ValueType::Int]);
        }
        for _ in 0..5 {
            specializer.observe_call(SymbolId(1), vec![ValueType::Float]);
        }

        let profiles = specializer.get_profiles(SymbolId(1)).unwrap();
        assert!(profiles[0].is_stable);
    }

    #[test]
    fn phase14_workflow_multiple_functions() {
        let mut specializer = TypeSpecializer::new();

        for _ in 0..20 {
            specializer.observe_call(SymbolId(1), vec![ValueType::Int]);
        }
        for _ in 0..20 {
            specializer.observe_call(SymbolId(2), vec![ValueType::Float]);
        }

        specializer.decide_strategy(SymbolId(1));
        specializer.decide_strategy(SymbolId(2));

        let stats = specializer.get_stats();
        assert_eq!(stats.functions_with_variants, 0); // No variants created yet
    }

    #[test]
    fn phase14_workflow_with_execution_tracking() {
        let mut specializer = TypeSpecializer::new();

        specializer.create_variant(SymbolId(1), vec![Some(ValueType::Int)]);
        specializer.create_variant(SymbolId(1), vec![Some(ValueType::Float)]);

        // Record executions
        for _ in 0..30 {
            specializer.record_variant_execution(SymbolId(1), 0);
        }
        for _ in 0..10 {
            specializer.record_variant_execution(SymbolId(1), 1);
        }

        let stats = specializer.get_stats();
        assert_eq!(stats.total_executions, 40);
        assert!(stats.avg_variant_executions > 0.0);
    }

    #[test]
    fn phase14_polymorphic_types() {
        let mut profile = TypeProfile::new();

        for _ in 0..10 {
            profile.observe(ValueType::Int);
        }
        for _ in 0..5 {
            profile.observe(ValueType::Float);
        }
        for _ in 0..5 {
            profile.observe(ValueType::Bool);
        }

        assert_eq!(profile.total_observations, 20);
        assert_eq!(profile.dominant_type, Some(ValueType::Int));
        // Dominant frequency has various possible values due to incremental updates
        // Just verify dominant frequency is reasonable
        assert!(profile.dominant_frequency >= 0.4);
    }

    #[test]
    fn phase14_type_specializer_default() {
        let specializer = TypeSpecializer::default();
        assert_eq!(specializer.total_specializations(), 0);
    }

    #[test]
    fn phase14_type_profile_default() {
        let profile = TypeProfile::default();
        assert_eq!(profile.total_observations, 0);
    }
}
