//! Syntax to HIR analysis
//!
//! This module converts expanded Syntax trees into HIR by:
//! 1. Resolving all variable references to BindingIds
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

use super::binding::{BindingId, BindingInfo, BindingKind, CaptureInfo, CaptureKind};
use super::expr::{Hir, HirKind};
use crate::effects::Effect;
use crate::symbol::SymbolTable;
use crate::syntax::Span;
use crate::value::SymbolId;
use std::collections::{HashMap, HashSet};

/// Result of HIR analysis
pub struct AnalysisResult {
    /// The analyzed HIR expression
    pub hir: Hir,
    /// Binding metadata from analysis
    pub bindings: HashMap<BindingId, BindingInfo>,
}

/// Analysis context tracking scopes and bindings
pub struct AnalysisContext {
    /// All bindings in the program
    bindings: HashMap<BindingId, BindingInfo>,
    /// Next binding ID to assign
    next_binding_id: u32,
}

impl AnalysisContext {
    pub fn new() -> Self {
        AnalysisContext {
            bindings: HashMap::new(),
            next_binding_id: 0,
        }
    }

    /// Create a fresh binding ID
    pub fn fresh_binding(&mut self) -> BindingId {
        let id = BindingId::new(self.next_binding_id);
        self.next_binding_id += 1;
        id
    }

    /// Register a binding
    pub fn register_binding(&mut self, info: BindingInfo) {
        self.bindings.insert(info.id, info);
    }

    /// Get binding info
    pub fn get_binding(&self, id: BindingId) -> Option<&BindingInfo> {
        self.bindings.get(&id)
    }

    /// Get mutable binding info
    pub fn get_binding_mut(&mut self, id: BindingId) -> Option<&mut BindingInfo> {
        self.bindings.get_mut(&id)
    }
}

impl Default for AnalysisContext {
    fn default() -> Self {
        Self::new()
    }
}

/// Tracks the sources of Yields effects within a lambda body.
/// Used to infer Polymorphic effects for higher-order functions.
#[derive(Debug, Clone, Default)]
struct EffectSources {
    /// Parameters whose calls contribute Yields effects (by their BindingId)
    param_calls: HashSet<BindingId>,
    /// Whether there's a direct yield (not from calling a parameter)
    has_direct_yield: bool,
    /// Whether there's a Yields from a non-parameter source (known yielding function, etc.)
    has_non_param_yield: bool,
}

/// A lexical scope
struct Scope {
    /// Bindings in this scope, by name
    bindings: HashMap<String, BindingId>,
    /// Is this a function scope (creates new capture boundary)
    is_function: bool,
    /// Next local index for this scope
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
    ctx: AnalysisContext,
    symbols: &'a mut SymbolTable,
    scopes: Vec<Scope>,
    /// Captures for the current function being analyzed
    current_captures: Vec<CaptureInfo>,
    /// Captures from the parent function (for nested closures)
    parent_captures: Vec<CaptureInfo>,
    /// Maps BindingId -> known effect of the bound value (if it's a callable)
    /// This enables interprocedural effect tracking: when we call a function,
    /// we can look up its effect and propagate it to the call site.
    effect_env: HashMap<BindingId, Effect>,
    /// Maps SymbolId -> Effect for primitive functions
    /// Built from `register_primitive_effects` and passed in at construction
    primitive_effects: HashMap<SymbolId, Effect>,
    /// Maps SymbolId -> Effect for user-defined global functions from previous forms.
    /// This enables cross-form effect tracking in compile_all_new.
    global_effects: HashMap<SymbolId, Effect>,
    /// Effects of globally-defined functions in this form (for cross-form tracking)
    /// Populated during analysis, extracted after analysis completes.
    defined_global_effects: HashMap<SymbolId, Effect>,
    /// Tracks effect sources within the current lambda body for polymorphic inference
    current_effect_sources: EffectSources,
    /// Parameters of the current lambda being analyzed (for polymorphic inference)
    current_lambda_params: Vec<BindingId>,
}

impl<'a> Analyzer<'a> {
    /// Create a new analyzer without primitive effects
    /// Use `new_with_primitive_effects` for full interprocedural effect tracking
    pub fn new(symbols: &'a mut SymbolTable) -> Self {
        Self::new_with_primitive_effects(symbols, HashMap::new())
    }

    /// Create a new analyzer with primitive effects for interprocedural tracking
    pub fn new_with_primitive_effects(
        symbols: &'a mut SymbolTable,
        primitive_effects: HashMap<SymbolId, Effect>,
    ) -> Self {
        let mut analyzer = Analyzer {
            ctx: AnalysisContext::new(),
            symbols,
            scopes: Vec::new(),
            current_captures: Vec::new(),
            parent_captures: Vec::new(),
            effect_env: HashMap::new(),
            primitive_effects,
            global_effects: HashMap::new(),
            defined_global_effects: HashMap::new(),
            current_effect_sources: EffectSources::default(),
            current_lambda_params: Vec::new(),
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

    /// Analyze a syntax tree into HIR
    pub fn analyze(&mut self, syntax: &crate::syntax::Syntax) -> Result<AnalysisResult, String> {
        let hir = self.analyze_expr(syntax)?;
        // Clone bindings instead of taking them, so they persist across multiple analyze() calls
        let bindings = self.ctx.bindings.clone();
        Ok(AnalysisResult { hir, bindings })
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

    fn bind(&mut self, name: &str, kind: BindingKind) -> BindingId {
        let id = self.ctx.fresh_binding();
        let sym = self.symbols.intern(name);

        let info = match kind {
            BindingKind::Parameter { index } => BindingInfo::parameter(id, sym, index),
            BindingKind::Local { index } => BindingInfo::local(id, sym, index),
            BindingKind::Global => BindingInfo::global(id, sym),
        };
        self.ctx.register_binding(info);

        if let Some(scope) = self.scopes.last_mut() {
            scope.bindings.insert(name.to_string(), id);
            if matches!(kind, BindingKind::Local { .. }) {
                scope.next_local += 1;
            }
        }

        id
    }

    fn lookup(&mut self, name: &str) -> Option<BindingId> {
        let mut found_in_scope = None;
        let mut crossed_function_boundary = false;

        // Walk scopes from innermost to outermost
        for (depth, scope) in self.scopes.iter().enumerate().rev() {
            if let Some(&id) = scope.bindings.get(name) {
                found_in_scope = Some((depth, id, crossed_function_boundary));
                break;
            }
            if scope.is_function {
                crossed_function_boundary = true;
            }
        }

        if let Some((_found_depth, id, needs_capture)) = found_in_scope {
            if needs_capture {
                // Check if this is a global - globals are not captured, accessed directly
                if let Some(info) = self.ctx.get_binding(id) {
                    if matches!(info.kind, BindingKind::Global) {
                        // Globals are accessed directly, not captured
                        return Some(id);
                    }
                }

                // Mark as captured
                if let Some(info) = self.ctx.get_binding_mut(id) {
                    info.mark_captured();
                }

                // Determine capture kind based on where it was found
                let capture_kind = if let Some(info) = self.ctx.get_binding(id) {
                    match info.kind {
                        BindingKind::Parameter { index } | BindingKind::Local { index } => {
                            // Direct capture from parent's locals (parameters or local variables)
                            CaptureKind::Local { index }
                        }
                        BindingKind::Global => {
                            // This should not happen due to the check above
                            CaptureKind::Global { sym: info.name }
                        }
                    }
                } else {
                    return Some(id);
                };

                // Add to current captures if not already present
                if !self.current_captures.iter().any(|c| c.binding == id) {
                    let is_mutated = self
                        .ctx
                        .get_binding(id)
                        .map(|i| i.is_mutated)
                        .unwrap_or(false);

                    self.current_captures.push(CaptureInfo {
                        binding: id,
                        kind: capture_kind,
                        is_mutated,
                    });
                }
            }
            return Some(id);
        }

        // If not found in scopes, check if it's in parent captures (for nested lambdas)
        if !self.parent_captures.is_empty() {
            for (capture_index, parent_cap) in self.parent_captures.iter().enumerate() {
                if let Some(info) = self.ctx.get_binding(parent_cap.binding) {
                    if info.name.0 == self.symbols.intern(name).0 {
                        // Found in parent captures - create a transitive capture
                        let binding_id = parent_cap.binding;

                        // Mark as captured
                        if let Some(info) = self.ctx.get_binding_mut(binding_id) {
                            info.mark_captured();
                        }

                        // Create a Capture kind that references the parent's capture index
                        let capture_kind = CaptureKind::Capture {
                            index: capture_index as u16,
                        };

                        // Add to current captures if not already present
                        if !self
                            .current_captures
                            .iter()
                            .any(|c| c.binding == binding_id)
                        {
                            let is_mutated = self
                                .ctx
                                .get_binding(binding_id)
                                .map(|i| i.is_mutated)
                                .unwrap_or(false);

                            self.current_captures.push(CaptureInfo {
                                binding: binding_id,
                                kind: capture_kind,
                                is_mutated,
                            });
                        }

                        return Some(binding_id);
                    }
                }
            }
        }

        None
    }

    fn current_local_index(&self) -> u16 {
        self.scopes.last().map(|s| s.next_local).unwrap_or(0)
    }

    fn current_local_count(&self) -> u16 {
        self.scopes.last().map(|s| s.next_local).unwrap_or(0)
    }

    /// Check if a binding is accessible in the current scope stack without crossing a function boundary
    fn is_binding_in_current_scope(&self, binding_id: BindingId) -> bool {
        // Walk scopes from innermost to outermost, stopping at function boundaries
        for scope in self.scopes.iter().rev() {
            if scope.bindings.values().any(|&id| id == binding_id) {
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
    fn lookup_in_current_scope(&self, name: &str) -> Option<BindingId> {
        self.scopes
            .last()
            .and_then(|scope| scope.bindings.get(name).copied())
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
    fn test_fresh_binding_id() {
        let mut ctx = AnalysisContext::new();
        let id1 = ctx.fresh_binding();
        let id2 = ctx.fresh_binding();
        assert_ne!(id1, id2);
        assert_eq!(id1, BindingId::new(0));
        assert_eq!(id2, BindingId::new(1));
    }

    #[test]
    fn test_binding_info() {
        let id = BindingId::new(0);
        let sym = SymbolId(1);

        let mut info = BindingInfo::local(id, sym, 0);
        assert!(!info.is_mutated);
        assert!(!info.is_captured);
        assert!(!info.needs_cell());

        info.mark_mutated();
        assert!(info.is_mutated);
        assert!(!info.needs_cell());

        info.mark_captured();
        assert!(info.is_captured);
        assert!(info.needs_cell());
    }
}
