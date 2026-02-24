//! Syntax to HIR analysis
//!
//! This module converts expanded Syntax trees into HIR by:
//! 1. Resolving all variable references to Bindings
//! 2. Computing captures for closures
//! 3. Inferring effects (including interprocedural effect tracking)
//! 4. Validating scope rules
//!
//! ## Interprocedural Effect Tracking
//!
//! The analyzer tracks effects across function boundaries:
//! - When a binding is defined with a lambda, we record the lambda body's effect
//! - When a call is analyzed, we look up the callee's effect and propagate it
//! - Polymorphic effects (like `map`) are resolved by examining the argument's effect
//! - `set!` invalidates effect tracking for the mutated binding

mod binding;
mod call;
mod forms;
mod special;

use super::binding::{Binding, CaptureInfo, CaptureKind};
use super::expr::{Hir, HirKind};
use crate::effects::Effect;
use crate::symbol::SymbolTable;
use crate::syntax::{ScopeId, Span};
use crate::value::heap::BindingScope;
use crate::value::types::Arity;
use crate::value::SymbolId;
use std::collections::{HashMap, HashSet};

/// Result of HIR analysis
pub struct AnalysisResult {
    /// The analyzed HIR expression
    pub hir: Hir,
}

/// Tracks the sources of Yields effects within a lambda body.
/// Used to infer Polymorphic effects for higher-order functions.
#[derive(Debug, Clone, Default)]
struct EffectSources {
    /// Parameters whose calls contribute Yields effects
    param_calls: HashSet<Binding>,
    /// Whether there's a direct yield (not from calling a parameter)
    has_direct_yield: bool,
    /// Whether there's a Yields from a non-parameter source (known yielding function, etc.)
    has_non_param_yield: bool,
}

/// A binding with its scope set for hygienic resolution.
#[derive(Debug, Clone)]
struct ScopedBinding {
    scopes: Vec<ScopeId>,
    binding: Binding,
}

/// Check if `subset` is a subset of `superset` (all elements of subset appear in superset).
fn is_scope_subset(subset: &[ScopeId], superset: &[ScopeId]) -> bool {
    subset.iter().all(|s| superset.contains(s))
}

/// A lexical scope
struct Scope {
    /// Bindings in this scope, by name. Multiple bindings per name are possible
    /// when macro expansion introduces bindings with different scope sets.
    bindings: HashMap<String, Vec<ScopedBinding>>,
    /// Is this a function scope (creates new capture boundary)
    is_function: bool,
    /// Next local index for this scope (used only for tracking local count)
    next_local: u16,
}

impl Scope {
    fn with_start_index(is_function: bool, start_index: u16) -> Self {
        Scope {
            bindings: HashMap::new(),
            is_function,
            next_local: start_index,
        }
    }
}

/// Analyzer that converts Syntax to HIR
pub struct Analyzer<'a> {
    symbols: &'a mut SymbolTable,
    scopes: Vec<Scope>,
    /// Captures for the current function being analyzed
    current_captures: Vec<CaptureInfo>,
    /// Captures from the parent function (for nested closures)
    parent_captures: Vec<CaptureInfo>,
    /// Maps Binding -> known effect of the bound value (if it's a callable)
    /// This enables interprocedural effect tracking: when we call a function,
    /// we can look up its effect and propagate it to the call site.
    effect_env: HashMap<Binding, Effect>,
    /// Maps SymbolId -> Effect for primitive functions
    /// Built from `register_primitive_effects` and passed in at construction
    primitive_effects: HashMap<SymbolId, Effect>,
    /// Maps SymbolId -> Effect for user-defined global functions from previous forms.
    /// This enables cross-form effect tracking in compile_all.
    global_effects: HashMap<SymbolId, Effect>,
    /// Effects of globally-defined functions in this form (for cross-form tracking)
    /// Populated during analysis, extracted after analysis completes.
    defined_global_effects: HashMap<SymbolId, Effect>,
    /// Arity environment: maps local function bindings to their arity.
    arity_env: HashMap<Binding, Arity>,
    /// Known arities of primitive functions, from PrimitiveMeta.
    primitive_arities: HashMap<SymbolId, Arity>,
    /// Known arities of global functions, from cross-form scanning.
    global_arities: HashMap<SymbolId, Arity>,
    /// Arities defined during this analysis pass, for fixpoint iteration.
    defined_global_arities: HashMap<SymbolId, Arity>,
    /// Bindings explicitly created by var/def forms (to distinguish from
    /// implicit global references when checking primitive arities).
    user_defined_globals: HashSet<Binding>,
    /// Tracks effect sources within the current lambda body for polymorphic inference
    current_effect_sources: EffectSources,
    /// Parameters of the current lambda being analyzed (for polymorphic inference)
    current_lambda_params: Vec<Binding>,
    /// Immutable global bindings from previous forms (for cross-form const tracking)
    immutable_globals: std::collections::HashSet<SymbolId>,
    /// Immutable global bindings defined in this form (for cross-form tracking)
    defined_immutable_globals: std::collections::HashSet<SymbolId>,
}

impl<'a> Analyzer<'a> {
    /// Create a new analyzer without primitive effects or arities
    pub fn new(symbols: &'a mut SymbolTable) -> Self {
        Self::new_with_primitives(symbols, HashMap::new(), HashMap::new())
    }

    /// Create a new analyzer with primitive effects for interprocedural tracking
    /// (convenience wrapper that passes empty arities)
    pub fn new_with_primitive_effects(
        symbols: &'a mut SymbolTable,
        primitive_effects: HashMap<SymbolId, Effect>,
    ) -> Self {
        Self::new_with_primitives(symbols, primitive_effects, HashMap::new())
    }

    /// Create a new analyzer with primitive effects and arities
    pub fn new_with_primitives(
        symbols: &'a mut SymbolTable,
        primitive_effects: HashMap<SymbolId, Effect>,
        primitive_arities: HashMap<SymbolId, Arity>,
    ) -> Self {
        let mut analyzer = Analyzer {
            symbols,
            scopes: Vec::new(),
            current_captures: Vec::new(),
            parent_captures: Vec::new(),
            effect_env: HashMap::new(),
            primitive_effects,
            global_effects: HashMap::new(),
            defined_global_effects: HashMap::new(),
            arity_env: HashMap::new(),
            primitive_arities,
            global_arities: HashMap::new(),
            defined_global_arities: HashMap::new(),
            user_defined_globals: HashSet::new(),
            current_effect_sources: EffectSources::default(),
            current_lambda_params: Vec::new(),
            immutable_globals: std::collections::HashSet::new(),
            defined_immutable_globals: std::collections::HashSet::new(),
        };
        // Initialize with a global scope so top-level bindings can be registered
        analyzer.push_scope(false);
        analyzer
    }

    /// Set global effects from previous forms (for cross-form effect tracking)
    pub fn set_global_effects(&mut self, global_effects: HashMap<SymbolId, Effect>) {
        self.global_effects = global_effects;
    }

    /// Take the defined global effects (consumes them, for use after analysis)
    pub fn take_defined_global_effects(&mut self) -> HashMap<SymbolId, Effect> {
        std::mem::take(&mut self.defined_global_effects)
    }

    /// Set global arities from previous forms (for cross-form arity tracking)
    pub fn set_global_arities(&mut self, arities: HashMap<SymbolId, Arity>) {
        self.global_arities = arities;
    }

    /// Take the defined global arities (consumes them, for use after analysis)
    pub fn take_defined_global_arities(&mut self) -> HashMap<SymbolId, Arity> {
        std::mem::take(&mut self.defined_global_arities)
    }

    /// Set immutable globals from previous forms (for cross-form const tracking)
    pub fn set_immutable_globals(
        &mut self,
        immutable_globals: std::collections::HashSet<SymbolId>,
    ) {
        self.immutable_globals = immutable_globals;
    }

    /// Take the defined immutable globals (consumes them, for use after analysis)
    pub fn take_defined_immutable_globals(&mut self) -> std::collections::HashSet<SymbolId> {
        std::mem::take(&mut self.defined_immutable_globals)
    }

    /// Analyze a syntax tree into HIR
    pub fn analyze(&mut self, syntax: &crate::syntax::Syntax) -> Result<AnalysisResult, String> {
        let hir = self.analyze_expr(syntax)?;
        Ok(AnalysisResult { hir })
    }

    // === Scope Management ===

    fn push_scope(&mut self, is_function: bool) {
        let start_index = if is_function {
            0
        } else {
            self.scopes.last().map(|s| s.next_local).unwrap_or(0)
        };
        self.scopes
            .push(Scope::with_start_index(is_function, start_index));
    }

    fn pop_scope(&mut self) -> Option<Scope> {
        self.scopes.pop()
    }

    fn bind(&mut self, name: &str, scopes: &[ScopeId], scope: BindingScope) -> Binding {
        let sym = self.symbols.intern(name);
        let binding = Binding::new(sym, scope);

        if let Some(scope_frame) = self.scopes.last_mut() {
            scope_frame
                .bindings
                .entry(name.to_string())
                .or_default()
                .push(ScopedBinding {
                    scopes: scopes.to_vec(),
                    binding,
                });
            if matches!(scope, BindingScope::Local) {
                scope_frame.next_local += 1;
            }
        }

        binding
    }

    fn lookup(&mut self, name: &str, ref_scopes: &[ScopeId]) -> Option<Binding> {
        let mut found_in_scope = None;
        let mut crossed_function_boundary = false;

        // Walk scopes from innermost to outermost
        for (depth, scope) in self.scopes.iter().enumerate().rev() {
            if let Some(candidates) = scope.bindings.get(name) {
                // Find the best candidate: binding's scopes must be a subset of
                // the reference's scopes, and the largest scope set wins.
                let best = candidates
                    .iter()
                    .filter(|c| is_scope_subset(&c.scopes, ref_scopes))
                    .max_by_key(|c| c.scopes.len());
                if let Some(winner) = best {
                    debug_assert!(
                        candidates
                            .iter()
                            .filter(|c| is_scope_subset(&c.scopes, ref_scopes))
                            .filter(|c| c.scopes.len() == winner.scopes.len())
                            .count()
                            == 1,
                        "Ambiguous binding: multiple candidates with same scope-set size for '{}'",
                        name
                    );
                    found_in_scope = Some((depth, winner.binding, crossed_function_boundary));
                    break;
                }
            }
            if scope.is_function {
                crossed_function_boundary = true;
            }
        }

        if let Some((_found_depth, binding, needs_capture)) = found_in_scope {
            if needs_capture {
                // Check if this is a global - globals are not captured, accessed directly
                if binding.is_global() {
                    return Some(binding);
                }

                // Mark as captured
                binding.mark_captured();

                // Determine capture kind based on where it was found
                let capture_kind = match binding.scope() {
                    BindingScope::Parameter | BindingScope::Local => {
                        // Direct capture from parent's locals
                        // Use binding_to_slot in the lowerer to find the actual index
                        // For now, use 0 as placeholder — the lowerer resolves this
                        CaptureKind::Local
                    }
                    BindingScope::Global => {
                        // This should not happen due to the check above
                        CaptureKind::Global {
                            sym: binding.name(),
                        }
                    }
                };

                // Add to current captures if not already present
                if !self.current_captures.iter().any(|c| c.binding == binding) {
                    self.current_captures.push(CaptureInfo {
                        binding,
                        kind: capture_kind,
                    });
                }
            }
            return Some(binding);
        }

        // If not found in scopes, check if it's in parent captures (for nested lambdas)
        if !self.parent_captures.is_empty() {
            for (capture_index, parent_cap) in self.parent_captures.iter().enumerate() {
                if parent_cap.binding.name().0 == self.symbols.intern(name).0 {
                    // Found in parent captures - create a transitive capture
                    let binding = parent_cap.binding;

                    // Mark as captured
                    binding.mark_captured();

                    // Create a Capture kind that references the parent's capture index
                    let capture_kind = CaptureKind::Capture {
                        index: capture_index as u16,
                    };

                    // Add to current captures if not already present
                    if !self.current_captures.iter().any(|c| c.binding == binding) {
                        self.current_captures.push(CaptureInfo {
                            binding,
                            kind: capture_kind,
                        });
                    }

                    return Some(binding);
                }
            }
        }

        None
    }

    fn current_local_count(&self) -> u16 {
        self.scopes.last().map(|s| s.next_local).unwrap_or(0)
    }

    /// Check if a binding is accessible in the current scope stack without crossing a function boundary
    fn is_binding_in_current_scope(&self, binding: Binding) -> bool {
        // Walk scopes from innermost to outermost, stopping at function boundaries
        for scope in self.scopes.iter().rev() {
            if scope
                .bindings
                .values()
                .flat_map(|v| v.iter())
                .any(|sb| sb.binding == binding)
            {
                return true;
            }
            if scope.is_function {
                // Stop at function boundary - anything beyond requires capturing
                break;
            }
        }
        false
    }

    /// Look up a binding in only the current (innermost) scope, not walking up the scope chain
    fn lookup_in_current_scope(&self, name: &str, ref_scopes: &[ScopeId]) -> Option<Binding> {
        self.scopes.last().and_then(|scope| {
            scope.bindings.get(name).and_then(|candidates| {
                candidates
                    .iter()
                    .filter(|c| is_scope_subset(&c.scopes, ref_scopes))
                    .max_by_key(|c| c.scopes.len())
                    .map(|c| c.binding)
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::{Span, Syntax, SyntaxKind};

    fn make_span() -> Span {
        Span::new(0, 0, 1, 1)
    }

    fn make_int(n: i64) -> Syntax {
        Syntax::new(SyntaxKind::Int(n), make_span())
    }

    fn make_symbol(name: &str) -> Syntax {
        Syntax::new(SyntaxKind::Symbol(name.to_string()), make_span())
    }

    fn make_list(items: Vec<Syntax>) -> Syntax {
        Syntax::new(SyntaxKind::List(items), make_span())
    }

    #[test]
    fn test_analyze_literal() {
        let mut symbols = SymbolTable::new();
        let mut analyzer = Analyzer::new(&mut symbols);

        let syntax = make_int(42);
        let result = analyzer.analyze(&syntax).unwrap();

        match result.hir.kind {
            HirKind::Int(n) => assert_eq!(n, 42),
            _ => panic!("Expected Int"),
        }
    }

    #[test]
    fn test_analyze_if() {
        let mut symbols = SymbolTable::new();
        let mut analyzer = Analyzer::new(&mut symbols);

        let syntax = make_list(vec![
            make_symbol("if"),
            Syntax::new(SyntaxKind::Bool(true), make_span()),
            make_int(1),
            make_int(2),
        ]);

        let result = analyzer.analyze(&syntax).unwrap();
        assert!(matches!(result.hir.kind, HirKind::If { .. }));
    }

    #[test]
    fn test_analyze_let() {
        let mut symbols = SymbolTable::new();
        let mut analyzer = Analyzer::new(&mut symbols);

        let syntax = make_list(vec![
            make_symbol("let"),
            make_list(vec![make_list(vec![make_symbol("x"), make_int(10)])]),
            make_symbol("x"),
        ]);

        let result = analyzer.analyze(&syntax).unwrap();
        assert!(matches!(result.hir.kind, HirKind::Let { .. }));
    }

    #[test]
    fn test_analyze_lambda() {
        let mut symbols = SymbolTable::new();
        let mut analyzer = Analyzer::new(&mut symbols);

        let syntax = make_list(vec![
            make_symbol("fn"),
            make_list(vec![make_symbol("x")]),
            make_symbol("x"),
        ]);

        let result = analyzer.analyze(&syntax).unwrap();
        assert!(matches!(result.hir.kind, HirKind::Lambda { .. }));
    }

    #[test]
    fn test_analyze_call() {
        let mut symbols = SymbolTable::new();
        let mut analyzer = Analyzer::new(&mut symbols);

        let syntax = make_list(vec![make_symbol("+"), make_int(1), make_int(2)]);

        let result = analyzer.analyze(&syntax).unwrap();
        assert!(matches!(result.hir.kind, HirKind::Call { .. }));
    }

    #[test]
    fn test_binding_info() {
        let sym = SymbolId(1);
        let binding = Binding::new(sym, BindingScope::Local);
        assert!(!binding.is_mutated());
        assert!(!binding.is_captured());
        assert!(!binding.needs_cell());

        binding.mark_mutated();
        assert!(binding.is_mutated());
        assert!(!binding.needs_cell());

        binding.mark_captured();
        assert!(binding.is_captured());
        assert!(binding.needs_cell());
    }

    // === Scope-aware binding resolution tests ===

    use crate::syntax::ScopeId;

    #[test]
    fn test_is_scope_subset_empty_is_subset_of_everything() {
        assert!(is_scope_subset(&[], &[]));
        assert!(is_scope_subset(&[], &[ScopeId(1)]));
        assert!(is_scope_subset(&[], &[ScopeId(1), ScopeId(2)]));
    }

    #[test]
    fn test_is_scope_subset_nonempty_not_subset_of_empty() {
        assert!(!is_scope_subset(&[ScopeId(1)], &[]));
    }

    #[test]
    fn test_is_scope_subset_identical_sets() {
        assert!(is_scope_subset(
            &[ScopeId(1), ScopeId(2)],
            &[ScopeId(1), ScopeId(2)]
        ));
    }

    #[test]
    fn test_is_scope_subset_proper_subset() {
        assert!(is_scope_subset(&[ScopeId(1)], &[ScopeId(1), ScopeId(2)]));
    }

    #[test]
    fn test_is_scope_subset_not_subset() {
        assert!(!is_scope_subset(
            &[ScopeId(1), ScopeId(3)],
            &[ScopeId(1), ScopeId(2)]
        ));
    }

    #[test]
    fn test_bind_and_lookup_with_empty_scopes() {
        // Pre-expansion code: empty scopes work identically to before
        let mut symbols = SymbolTable::new();
        let mut analyzer = Analyzer::new(&mut symbols);
        analyzer.push_scope(false);
        let binding = analyzer.bind("x", &[], BindingScope::Local);
        assert_eq!(analyzer.lookup("x", &[]), Some(binding));
    }

    #[test]
    fn test_lookup_scope_filtering() {
        // Binding with scope {S1} is invisible to reference with empty scopes
        let mut symbols = SymbolTable::new();
        let mut analyzer = Analyzer::new(&mut symbols);
        analyzer.push_scope(false);
        analyzer.bind("tmp", &[ScopeId(1)], BindingScope::Local);
        // Reference with empty scopes cannot see binding with {S1}
        assert_eq!(analyzer.lookup("tmp", &[]), None);
    }

    #[test]
    fn test_lookup_scope_subset_match() {
        // Binding with scope {S1} is visible to reference with {S1}
        let mut symbols = SymbolTable::new();
        let mut analyzer = Analyzer::new(&mut symbols);
        analyzer.push_scope(false);
        let binding = analyzer.bind("tmp", &[ScopeId(1)], BindingScope::Local);
        assert_eq!(analyzer.lookup("tmp", &[ScopeId(1)]), Some(binding));
    }

    #[test]
    fn test_lookup_largest_scope_wins() {
        // Two bindings for "tmp": one with {} and one with {S1}
        // Reference with {S1} should see the {S1} binding (more specific)
        let mut symbols = SymbolTable::new();
        let mut analyzer = Analyzer::new(&mut symbols);
        analyzer.push_scope(false);
        let _outer = analyzer.bind("tmp", &[], BindingScope::Local);
        let inner = analyzer.bind("tmp", &[ScopeId(1)], BindingScope::Local);
        assert_eq!(analyzer.lookup("tmp", &[ScopeId(1)]), Some(inner));
    }

    #[test]
    fn test_lookup_empty_scopes_sees_empty_binding() {
        // Two bindings for "tmp": one with {} and one with {S1}
        // Reference with {} should see only the {} binding
        let mut symbols = SymbolTable::new();
        let mut analyzer = Analyzer::new(&mut symbols);
        analyzer.push_scope(false);
        let outer = analyzer.bind("tmp", &[], BindingScope::Local);
        let _inner = analyzer.bind("tmp", &[ScopeId(1)], BindingScope::Local);
        assert_eq!(analyzer.lookup("tmp", &[]), Some(outer));
    }

    #[test]
    fn test_lookup_in_current_scope_with_scopes() {
        let mut symbols = SymbolTable::new();
        let mut analyzer = Analyzer::new(&mut symbols);
        analyzer.push_scope(false);
        let binding = analyzer.bind("x", &[ScopeId(1)], BindingScope::Local);
        // Visible with matching scopes
        assert_eq!(
            analyzer.lookup_in_current_scope("x", &[ScopeId(1)]),
            Some(binding)
        );
        // Invisible with empty scopes
        assert_eq!(analyzer.lookup_in_current_scope("x", &[]), None);
    }
}
