// Phase 14: Type Specialization & Specialization Framework
//
// Enables type-based code specialization:
// - Type profiling from runtime values
// - Specialization strategy selection
// - Type-specialized code generation
// - Polymorphic dispatch with type guards
// - Specialization performance tracking
// - Bailout handling for unexpected types

use crate::value::{SymbolId, Value};
use std::collections::HashMap;

/// Type information gathered at runtime
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ValueType {
    /// Integer type
    Int,
    /// Floating point type
    Float,
    /// Boolean type
    Bool,
    /// Nil type
    Nil,
    /// Symbol type
    Symbol,
    /// List/Cons type
    List,
    /// Vector type
    Vector,
    /// String type
    String,
    /// Unknown or mixed type
    Unknown,
}

impl ValueType {
    /// Get the type of a value
    pub fn from_value(val: &Value) -> Self {
        match val {
            Value::Int(_) => ValueType::Int,
            Value::Float(_) => ValueType::Float,
            Value::Bool(_) => ValueType::Bool,
            Value::Nil => ValueType::Nil,
            Value::Symbol(_) => ValueType::Symbol,
            _ => ValueType::Unknown,
        }
    }

    /// Check if this is a numeric type
    pub fn is_numeric(&self) -> bool {
        matches!(self, ValueType::Int | ValueType::Float)
    }

    /// Check if this is a scalar type
    pub fn is_scalar(&self) -> bool {
        matches!(
            self,
            ValueType::Int | ValueType::Float | ValueType::Bool | ValueType::Nil
        )
    }
}

/// Profile of types observed for a function parameter or result
#[derive(Debug, Clone)]
pub struct TypeProfile {
    /// Most common type (0.0 to 1.0 frequency)
    pub dominant_type: Option<ValueType>,
    /// Dominant type frequency
    pub dominant_frequency: f64,
    /// All observed types and their frequencies
    pub type_frequencies: HashMap<ValueType, usize>,
    /// Total observations
    pub total_observations: usize,
    /// Whether type is stable (always same type)
    pub is_stable: bool,
}

impl TypeProfile {
    /// Create new type profile
    pub fn new() -> Self {
        TypeProfile {
            dominant_type: None,
            dominant_frequency: 0.0,
            type_frequencies: HashMap::new(),
            total_observations: 0,
            is_stable: false,
        }
    }

    /// Record a type observation
    pub fn observe(&mut self, ty: ValueType) {
        *self.type_frequencies.entry(ty).or_insert(0) += 1;
        self.total_observations += 1;

        // Update dominant type
        if let Some((t, freq)) = self.type_frequencies.iter().max_by_key(|(_, f)| *f) {
            let freq_ratio = *freq as f64 / self.total_observations as f64;
            if freq_ratio > self.dominant_frequency {
                self.dominant_type = Some(*t);
                self.dominant_frequency = freq_ratio;
            }
        }

        // Check if stable (95% same type with 20+ observations)
        self.is_stable = self.total_observations >= 20 && self.dominant_frequency >= 0.95;
    }

    /// Check if type profile is useful for specialization
    pub fn is_specializable(&self) -> bool {
        self.is_stable && self.dominant_frequency > 0.9
    }

    /// Get polymorphic frequency (inverse of dominance)
    pub fn polymorphic_ratio(&self) -> f64 {
        1.0 - self.dominant_frequency
    }
}

impl Default for TypeProfile {
    fn default() -> Self {
        Self::new()
    }
}

/// Specialization strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SpecializationStrategy {
    /// No specialization
    #[default]
    None,
    /// Specialize for dominant type only
    Monomorphic,
    /// Specialize for top 2 types
    Duomorphic,
    /// Specialize for top 3+ types
    Polymorphic,
}

impl SpecializationStrategy {
    /// Get the maximum number of specialized variants
    pub fn max_variants(&self) -> usize {
        match self {
            SpecializationStrategy::None => 1,
            SpecializationStrategy::Monomorphic => 1,
            SpecializationStrategy::Duomorphic => 2,
            SpecializationStrategy::Polymorphic => 4,
        }
    }

    /// Check if this strategy should create a generic fallback
    pub fn needs_fallback(&self) -> bool {
        matches!(
            self,
            SpecializationStrategy::Duomorphic | SpecializationStrategy::Polymorphic
        )
    }
}

/// Specialized code variant
#[derive(Debug, Clone)]
pub struct SpecializedVariant {
    /// Input type specification
    pub input_types: Vec<Option<ValueType>>,
    /// Output type (if known)
    pub output_type: Option<ValueType>,
    /// Code address (in JIT code cache)
    pub code_address: Option<usize>,
    /// Execution count for this variant
    pub execution_count: usize,
}

impl SpecializedVariant {
    /// Create new specialized variant
    pub fn new(input_types: Vec<Option<ValueType>>) -> Self {
        SpecializedVariant {
            input_types,
            output_type: None,
            code_address: None,
            execution_count: 0,
        }
    }

    /// Record an execution
    pub fn record_execution(&mut self) {
        self.execution_count += 1;
    }

    /// Get execution frequency estimate (0.0 to 1.0)
    pub fn frequency_estimate(&self, total: usize) -> f64 {
        if total == 0 {
            0.0
        } else {
            self.execution_count as f64 / total as f64
        }
    }
}

/// Type specialization manager
pub struct TypeSpecializer {
    /// Type profiles per function
    function_profiles: HashMap<SymbolId, Vec<TypeProfile>>,
    /// Specialization strategies per function
    specialization_strategies: HashMap<SymbolId, SpecializationStrategy>,
    /// Specialized variants per function
    specialized_variants: HashMap<SymbolId, Vec<SpecializedVariant>>,
    /// Total specializations created
    total_specializations: usize,
    /// Total executions across all specializations
    total_executions: usize,
}

impl TypeSpecializer {
    /// Create new type specializer
    pub fn new() -> Self {
        TypeSpecializer {
            function_profiles: HashMap::new(),
            specialization_strategies: HashMap::new(),
            specialized_variants: HashMap::new(),
            total_specializations: 0,
            total_executions: 0,
        }
    }

    /// Record a function call with argument types
    pub fn observe_call(&mut self, func: SymbolId, arg_types: Vec<ValueType>) {
        let profiles = self.function_profiles.entry(func).or_default();

        // Ensure we have profiles for all arguments
        while profiles.len() < arg_types.len() {
            profiles.push(TypeProfile::new());
        }

        // Record each argument type
        for (i, ty) in arg_types.iter().enumerate() {
            profiles[i].observe(*ty);
        }
    }

    /// Decide specialization strategy for a function
    pub fn decide_strategy(&mut self, func: SymbolId) -> SpecializationStrategy {
        let profiles = self.function_profiles.get(&func);

        match profiles {
            None => SpecializationStrategy::None,
            Some(profs) => {
                // Check if all parameters have stable types
                let all_stable = profs.iter().all(|p| p.is_stable);
                let specializable_count = profs.iter().filter(|p| p.is_specializable()).count();

                let strategy = if !all_stable || specializable_count == 0 {
                    SpecializationStrategy::None
                } else if specializable_count == 1 {
                    // Check polymorphism rate
                    let avg_polymorphic: f64 =
                        profs.iter().map(|p| p.polymorphic_ratio()).sum::<f64>()
                            / profs.len() as f64;

                    if avg_polymorphic < 0.1 {
                        SpecializationStrategy::Monomorphic
                    } else if avg_polymorphic < 0.2 {
                        SpecializationStrategy::Duomorphic
                    } else {
                        SpecializationStrategy::Polymorphic
                    }
                } else {
                    SpecializationStrategy::Polymorphic
                };

                self.specialization_strategies.insert(func, strategy);
                strategy
            }
        }
    }

    /// Create a specialized variant
    pub fn create_variant(
        &mut self,
        func: SymbolId,
        input_types: Vec<Option<ValueType>>,
    ) -> SpecializedVariant {
        self.total_specializations += 1;

        let mut variant = SpecializedVariant::new(input_types);
        variant.code_address = Some(self.total_specializations * 256); // Placeholder address

        let variants = self.specialized_variants.entry(func).or_default();
        variants.push(variant.clone());

        variant
    }

    /// Record execution of a variant
    pub fn record_variant_execution(&mut self, func: SymbolId, variant_index: usize) {
        if let Some(variants) = self.specialized_variants.get_mut(&func) {
            if variant_index < variants.len() {
                variants[variant_index].record_execution();
                self.total_executions += 1;
            }
        }
    }

    /// Get type profiles for a function
    pub fn get_profiles(&self, func: SymbolId) -> Option<&[TypeProfile]> {
        self.function_profiles.get(&func).map(|v| v.as_slice())
    }

    /// Get specialization strategy for a function
    pub fn get_strategy(&self, func: SymbolId) -> SpecializationStrategy {
        self.specialization_strategies
            .get(&func)
            .copied()
            .unwrap_or(SpecializationStrategy::None)
    }

    /// Get variants for a function
    pub fn get_variants(&self, func: SymbolId) -> Option<&[SpecializedVariant]> {
        self.specialized_variants.get(&func).map(|v| v.as_slice())
    }

    /// Get specialization statistics
    pub fn get_stats(&self) -> SpecializationStats {
        let total_variants = self.specialized_variants.values().map(|v| v.len()).sum();
        let functions_with_variants = self.specialized_variants.len();

        let avg_variant_executions = if total_variants > 0 {
            self.total_executions as f64 / total_variants as f64
        } else {
            0.0
        };

        SpecializationStats {
            total_specializations: self.total_specializations,
            total_variants,
            functions_with_variants,
            total_executions: self.total_executions,
            avg_variant_executions,
        }
    }

    /// Get total specializations created
    pub fn total_specializations(&self) -> usize {
        self.total_specializations
    }

    /// Get total executions
    pub fn total_executions(&self) -> usize {
        self.total_executions
    }

    /// Check if bailout is needed (unexpected type)
    pub fn should_bailout(&self, func: SymbolId, arg_types: &[ValueType]) -> bool {
        if let Some(variants) = self.specialized_variants.get(&func) {
            if variants.is_empty() {
                return false;
            }

            // Check if arg types match any variant
            for variant in variants {
                let matches = variant.input_types.iter().zip(arg_types.iter()).all(
                    |(spec_type, arg_type)| spec_type.is_none() || *spec_type == Some(*arg_type),
                );

                if matches {
                    return false;
                }
            }

            true // No matching variant, bailout needed
        } else {
            false
        }
    }
}

impl Default for TypeSpecializer {
    fn default() -> Self {
        Self::new()
    }
}

/// Specialization statistics
#[derive(Debug, Clone)]
pub struct SpecializationStats {
    /// Total specializations created
    pub total_specializations: usize,
    /// Total specialized variants
    pub total_variants: usize,
    /// Functions with at least one specialization
    pub functions_with_variants: usize,
    /// Total executions across all variants
    pub total_executions: usize,
    /// Average executions per variant
    pub avg_variant_executions: f64,
}

impl SpecializationStats {
    /// Get specialization coverage (fraction of functions with specializations)
    pub fn specialization_coverage(&self, total_functions: usize) -> f64 {
        if total_functions == 0 {
            0.0
        } else {
            self.functions_with_variants as f64 / total_functions as f64
        }
    }

    /// Get variant utilization (average executions per variant)
    pub fn variant_utilization(&self) -> f64 {
        self.avg_variant_executions.min(100.0) / 100.0 // Normalize to 0-1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase14_value_type_from_value() {
        assert_eq!(ValueType::from_value(&Value::Int(42)), ValueType::Int);
        assert_eq!(
            ValueType::from_value(&Value::Float(std::f64::consts::PI)),
            ValueType::Float
        );
        assert_eq!(ValueType::from_value(&Value::Bool(true)), ValueType::Bool);
        assert_eq!(ValueType::from_value(&Value::Nil), ValueType::Nil);
    }

    #[test]
    fn phase14_value_type_is_numeric() {
        assert!(ValueType::Int.is_numeric());
        assert!(ValueType::Float.is_numeric());
        assert!(!ValueType::Bool.is_numeric());
    }

    #[test]
    fn phase14_value_type_is_scalar() {
        assert!(ValueType::Int.is_scalar());
        assert!(ValueType::Float.is_scalar());
        assert!(ValueType::Bool.is_scalar());
        assert!(!ValueType::Symbol.is_scalar());
    }

    #[test]
    fn phase14_type_profile_observe() {
        let mut profile = TypeProfile::new();

        profile.observe(ValueType::Int);
        profile.observe(ValueType::Int);
        profile.observe(ValueType::Float);

        assert_eq!(profile.total_observations, 3);
        assert_eq!(profile.dominant_type, Some(ValueType::Int));
    }

    #[test]
    fn phase14_type_profile_stability() {
        let mut profile = TypeProfile::new();

        for _ in 0..20 {
            profile.observe(ValueType::Int);
        }

        assert!(profile.is_stable);
        assert!(profile.is_specializable());
    }

    #[test]
    fn phase14_type_profile_not_stable() {
        let mut profile = TypeProfile::new();

        // Create unstable profile (not enough observations)
        for _ in 0..5 {
            profile.observe(ValueType::Int);
            profile.observe(ValueType::Float);
        }

        assert!(!profile.is_stable);
    }

    #[test]
    fn phase14_specialization_strategy_monomorphic() {
        assert_eq!(SpecializationStrategy::Monomorphic.max_variants(), 1);
        assert!(!SpecializationStrategy::Monomorphic.needs_fallback());
    }

    #[test]
    fn phase14_specialization_strategy_duomorphic() {
        assert_eq!(SpecializationStrategy::Duomorphic.max_variants(), 2);
        assert!(SpecializationStrategy::Duomorphic.needs_fallback());
    }

    #[test]
    fn phase14_specialization_strategy_polymorphic() {
        assert_eq!(SpecializationStrategy::Polymorphic.max_variants(), 4);
        assert!(SpecializationStrategy::Polymorphic.needs_fallback());
    }

    #[test]
    fn phase14_specialized_variant_creation() {
        let variant = SpecializedVariant::new(vec![Some(ValueType::Int)]);

        assert_eq!(variant.input_types.len(), 1);
        assert_eq!(variant.execution_count, 0);
    }

    #[test]
    fn phase14_specialized_variant_execution() {
        let mut variant = SpecializedVariant::new(vec![Some(ValueType::Int)]);

        variant.record_execution();
        variant.record_execution();

        assert_eq!(variant.execution_count, 2);
        assert_eq!(variant.frequency_estimate(10), 0.2);
    }

    #[test]
    fn phase14_type_specializer_creation() {
        let specializer = TypeSpecializer::new();

        assert_eq!(specializer.total_specializations, 0);
        assert_eq!(specializer.total_executions, 0);
    }

    #[test]
    fn phase14_type_specializer_observe() {
        let mut specializer = TypeSpecializer::new();

        specializer.observe_call(SymbolId(1), vec![ValueType::Int, ValueType::Float]);

        let profiles = specializer.get_profiles(SymbolId(1));
        assert!(profiles.is_some());
    }

    #[test]
    fn phase14_type_specializer_strategy_decision() {
        let mut specializer = TypeSpecializer::new();

        for _ in 0..20 {
            specializer.observe_call(SymbolId(1), vec![ValueType::Int]);
        }

        let strategy = specializer.decide_strategy(SymbolId(1));

        assert_ne!(strategy, SpecializationStrategy::None);
    }

    #[test]
    fn phase14_type_specializer_create_variant() {
        let mut specializer = TypeSpecializer::new();

        let variant = specializer.create_variant(SymbolId(1), vec![Some(ValueType::Int)]);

        assert!(variant.code_address.is_some());
        assert_eq!(specializer.total_specializations, 1);
    }

    #[test]
    fn phase14_type_specializer_stats() {
        let mut specializer = TypeSpecializer::new();

        specializer.create_variant(SymbolId(1), vec![Some(ValueType::Int)]);
        specializer.create_variant(SymbolId(1), vec![Some(ValueType::Float)]);

        let stats = specializer.get_stats();

        assert_eq!(stats.total_variants, 2);
        assert_eq!(stats.functions_with_variants, 1);
    }

    #[test]
    fn phase14_type_specializer_bailout() {
        let mut specializer = TypeSpecializer::new();

        specializer.create_variant(SymbolId(1), vec![Some(ValueType::Int)]);

        let should_bailout = specializer.should_bailout(SymbolId(1), &[ValueType::Float]);

        assert!(should_bailout);
    }

    #[test]
    fn phase14_specialization_stats_coverage() {
        let stats = SpecializationStats {
            total_specializations: 5,
            total_variants: 3,
            functions_with_variants: 2,
            total_executions: 100,
            avg_variant_executions: 33.33,
        };

        assert!(stats.specialization_coverage(10) > 0.0);
    }
}
