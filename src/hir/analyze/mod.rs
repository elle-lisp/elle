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
mod destructure;
mod fileletrec;
mod forms;
mod lambda;
mod special;

use super::binding::{Binding, CaptureInfo, CaptureKind};
use super::expr::{BlockId, Hir, HirKind};
use crate::effects::Effect;
use crate::primitives::def::PrimitiveMeta;
use crate::symbol::SymbolTable;
use crate::syntax::{ScopeId, Span, Syntax};
use crate::value::heap::BindingScope;
use crate::value::types::Arity;
use crate::value::{SymbolId, Value};
use std::collections::{HashMap, HashSet};

/// A classified top-level form for file-as-letrec compilation.
///
/// The pipeline classifies each expanded top-level form into one of
/// these variants before passing them to `Analyzer::analyze_file_letrec`.
pub enum FileForm<'a> {
    /// `(def name value)` or `(def pattern value)` — immutable binding
    Def(&'a Syntax, &'a Syntax),
    /// `(var name value)` or `(var pattern value)` — mutable binding
    Var(&'a Syntax, &'a Syntax),
    /// `(effect :keyword)` — user-defined effect declaration
    Effect(&'a Syntax),
    /// Bare expression — gets a gensym name
    Expr(&'a Syntax),
}

/// Tracks an active block for `break` targeting.
struct BlockContext {
    block_id: BlockId,
    name: Option<String>,
    /// fn_depth at the point the block was entered.
    /// A break can only target blocks at the same fn_depth.
    fn_depth: u32,
}

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
    /// Arity environment: maps local function bindings to their arity.
    /// Populated by `bind_primitives` for primitive bindings; user
    /// shadows create new bindings that won't be in this map,
    /// correctly disabling the primitive arity check.
    arity_env: HashMap<Binding, Arity>,

    /// Tracks effect sources within the current lambda body for polymorphic inference
    current_effect_sources: EffectSources,
    /// Parameters of the current lambda being analyzed (for polymorphic inference)
    current_lambda_params: Vec<Binding>,
    /// Stack of active blocks for `break` targeting
    block_contexts: Vec<BlockContext>,
    /// Next block ID to allocate
    next_block_id: u32,
    /// Current function nesting depth (incremented in analyze_lambda).
    /// Used to prevent `break` from crossing function boundaries.
    fn_depth: u32,
    /// Pre-created bindings from letrec pass 1 for destructured forms.
    /// When set, `analyze_destructure_pattern` uses these instead of
    /// `lookup_in_current_scope` to avoid binding identity mismatch
    /// when the same name appears in multiple file-scope forms.
    pre_bindings: HashMap<String, Binding>,
    /// Compile-time constant values for primitive bindings.
    /// Populated by `bind_primitives`. The lowerer seeds its
    /// `immutable_values` map from this so primitive references
    /// emit `LoadConst` instead of `LoadGlobal`.
    /// No slot allocation is needed.
    primitive_values: HashMap<Binding, Value>,
    /// Accumulated parameter bounds from restrict forms in current lambda.
    /// Populated by `analyze_restrict`, consumed by `analyze_lambda`.
    current_param_bounds: HashMap<Binding, Effect>,
    /// Accumulated function-level ceiling from restrict forms in current lambda.
    /// Populated by `analyze_restrict`, consumed by `analyze_lambda`.
    current_declared_ceiling: Option<Effect>,
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
        _primitive_arities: HashMap<SymbolId, Arity>,
    ) -> Self {
        let mut analyzer = Analyzer {
            symbols,
            scopes: Vec::new(),
            current_captures: Vec::new(),
            parent_captures: Vec::new(),
            effect_env: HashMap::new(),
            primitive_effects,
            arity_env: HashMap::new(),

            current_effect_sources: EffectSources::default(),
            current_lambda_params: Vec::new(),
            block_contexts: Vec::new(),
            next_block_id: 0,
            fn_depth: 0,
            pre_bindings: HashMap::new(),
            primitive_values: HashMap::new(),
            current_param_bounds: HashMap::new(),
            current_declared_ceiling: None,
        };
        // Initialize with a global scope so top-level bindings can be registered
        analyzer.push_scope(false);
        analyzer
    }

    /// Analyze a syntax tree into HIR
    pub fn analyze(&mut self, syntax: &crate::syntax::Syntax) -> Result<AnalysisResult, String> {
        let hir = self.analyze_expr(syntax)?;
        Ok(AnalysisResult { hir })
    }

    /// Bind all registered primitives as immutable Local bindings in the
    /// analyzer's initial scope.
    ///
    /// Called before `analyze_file_letrec` so that primitives are in scope
    /// during file analysis. Primitives are `BindingScope::Local` with
    /// `mark_immutable()` set. File-level `def` bindings shadow primitives
    /// because `analyze_file_letrec` pushes a new scope.
    ///
    /// The lowerer uses `immutable_values` to emit `LoadConst` for these
    /// bindings — the `NativeFn` values are baked into the constant pool.
    /// No slot allocation is needed.
    pub fn bind_primitives(&mut self, meta: &PrimitiveMeta) {
        for (&sym_id, &effect) in &meta.effects {
            let binding = self.bind_by_sym(sym_id, BindingScope::Local);
            binding.mark_immutable();
            self.effect_env.insert(binding, effect);
            if let Some(&arity) = meta.arities.get(&sym_id) {
                self.arity_env.insert(binding, arity);
            }
            if let Some(&func_value) = meta.functions.get(&sym_id) {
                self.primitive_values.insert(binding, func_value);
            }
        }
    }

    /// Return the primitive binding→value map for the lowerer.
    ///
    /// The lowerer seeds its `immutable_values` from this so that
    /// primitive references compile to `LoadConst`.
    pub fn primitive_values(&self) -> &HashMap<Binding, Value> {
        &self.primitive_values
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

    /// Register an already-created binding in the current scope without
    /// creating a new one. Used by `analyze_file_letrec` Pass 2 to add
    /// deferred duplicate-name bindings at the correct sequential point.
    fn register_binding(&mut self, name: &str, scopes: &[ScopeId], binding: Binding) {
        if let Some(scope_frame) = self.scopes.last_mut() {
            scope_frame
                .bindings
                .entry(name.to_string())
                .or_default()
                .push(ScopedBinding {
                    scopes: scopes.to_vec(),
                    binding,
                });
            if matches!(binding.scope(), BindingScope::Local) {
                scope_frame.next_local += 1;
            }
        }
    }

    /// Bind a symbol by its already-interned SymbolId.
    ///
    /// Used by `bind_primitives` where we already have SymbolIds from
    /// PrimitiveMeta and need to resolve the name for scope registration.
    fn bind_by_sym(&mut self, sym: SymbolId, scope: BindingScope) -> Binding {
        let name = self
            .symbols
            .name(sym)
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("sym#{}", sym.0));
        let binding = Binding::new(sym, scope);

        if let Some(scope_frame) = self.scopes.last_mut() {
            scope_frame
                .bindings
                .entry(name)
                .or_default()
                .push(ScopedBinding {
                    scopes: Vec::new(), // primitives have empty scopes (visible everywhere)
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
                // When multiple candidates share the largest scope-set size,
                // max_by_key returns the last one (the most recently bound),
                // which gives correct file-level redefinition semantics.
                let best = candidates
                    .iter()
                    .filter(|c| is_scope_subset(&c.scopes, ref_scopes))
                    .max_by_key(|c| c.scopes.len());
                if let Some(winner) = best {
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
                // Primitives are immutable locals with known constant values.
                // They don't need capturing — the lowerer emits LoadConst
                // for them directly from immutable_values.
                if self.primitive_values.contains_key(&binding) {
                    return Some(binding);
                }

                // Mark as captured
                binding.mark_captured();

                // Determine capture kind
                let capture_kind = CaptureKind::Local;

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
        // Pre-bind "+" so it resolves during analysis
        analyzer.bind("+", &[], BindingScope::Local);

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
        assert!(!binding.needs_lbox());

        binding.mark_mutated();
        assert!(binding.is_mutated());
        assert!(!binding.needs_lbox());

        binding.mark_captured();
        assert!(binding.is_captured());
        assert!(binding.needs_lbox());
    }

    #[test]
    fn test_immutable_captured_local_no_cell() {
        // An immutable local (let-bound) that is captured should NOT need a cell.
        // Immutable captures are captured by value directly.
        let binding = Binding::new(SymbolId(2), BindingScope::Local);
        binding.mark_immutable();
        binding.mark_captured();
        assert!(binding.is_immutable());
        assert!(binding.is_captured());
        assert!(!binding.is_prebound());
        assert!(!binding.needs_lbox());
    }

    #[test]
    fn test_immutable_prebound_captured_local_needs_lbox() {
        // An immutable local that is prebound (def in begin, letrec) AND
        // captured DOES need a cell — the capture may happen before the
        // binding is initialized (self-recursion, forward references).
        let binding = Binding::new(SymbolId(2), BindingScope::Local);
        binding.mark_prebound();
        binding.mark_immutable();
        binding.mark_captured();
        assert!(binding.is_immutable());
        assert!(binding.is_captured());
        assert!(binding.is_prebound());
        assert!(binding.needs_lbox());
    }

    #[test]
    fn test_mutable_captured_local_needs_lbox() {
        // A mutable local (var) that is captured DOES need a cell.
        let binding = Binding::new(SymbolId(3), BindingScope::Local);
        binding.mark_captured();
        assert!(!binding.is_immutable());
        assert!(binding.is_captured());
        assert!(binding.needs_lbox());
    }

    #[test]
    fn test_immutable_uncaptured_local_no_cell() {
        // An immutable local that is NOT captured should not need a cell.
        let binding = Binding::new(SymbolId(4), BindingScope::Local);
        binding.mark_immutable();
        assert!(!binding.needs_lbox());
    }

    #[test]
    fn test_immutable_mutated_captured_local_needs_lbox() {
        // Edge case: a binding marked immutable but also mutated and captured.
        // Immutable wins — no cell needed. (In practice, the analyzer would
        // reject set on an immutable binding, so this shouldn't happen.)
        let binding = Binding::new(SymbolId(5), BindingScope::Local);
        binding.mark_immutable();
        binding.mark_mutated();
        binding.mark_captured();
        assert!(!binding.needs_lbox());
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
