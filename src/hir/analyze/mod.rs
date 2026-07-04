//! Syntax to HIR analysis
//!
//! This module converts expanded Syntax trees into HIR by:
//! 1. Resolving all variable references to Bindings
//! 2. Computing captures for closures
//! 3. Inferring signals (including interprocedural signal tracking)
//! 4. Validating scope rules
//!
//! ## Interprocedural Signal Tracking
//!
//! The analyzer tracks signals across function boundaries:
//! - When a binding is defined with a lambda, we record the lambda body's signal
//! - When a call is analyzed, we look up the callee's signal and propagate it
//! - Polymorphic signals (like `map`) are resolved by examining the argument's signal
//! - `set!` invalidates signal tracking for the mutated binding

mod binding;
mod call;
mod destructure;
mod fileletrec;
mod letrec;
pub use fileletrec::classify_form;
pub(crate) mod forms;
mod lambda;
mod special;

use super::binding::{Binding, CaptureInfo, CaptureKind};
use super::expr::{BlockId, Hir, HirKind};
use crate::error::LError;
use crate::hir::arena::{BindingArena, BindingScope};
use crate::primitives::def::PrimitiveMeta;
use crate::signals::Signal;
use crate::symbol::SymbolTable;
use crate::syntax::{ScopeId, Span, Syntax};
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
    /// `(signal :keyword)` — user-defined signal declaration
    Signal(&'a Syntax),
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
    /// Accumulated non-fatal errors (undefined vars, signal mismatches)
    pub errors: Vec<LError>,
}

/// Tracks signal sources within a lambda body, separating inherent
/// signals (emitted directly or by non-parameter callees) from
/// parameter-dependent signals (polymorphic propagation).
#[derive(Debug, Clone)]
struct SignalSources {
    /// Parameters whose calls contribute signals.
    param_calls: HashSet<Binding>,
    /// Bits from direct `emit` forms (error, yield, io, etc.).
    direct_bits: crate::value::fiber::SignalBits,
    /// Bits from calls to non-parameter callees whose signals
    /// are statically known (not polymorphic, not parameter calls).
    non_param_bits: crate::value::fiber::SignalBits,
}

impl Default for SignalSources {
    fn default() -> Self {
        SignalSources {
            param_calls: HashSet::new(),
            direct_bits: crate::value::fiber::SignalBits::EMPTY,
            non_param_bits: crate::value::fiber::SignalBits::EMPTY,
        }
    }
}

/// A binding with its scope set for hygienic resolution.
#[derive(Debug, Clone)]
struct ScopedBinding {
    scopes: Vec<ScopeId>,
    binding: Binding,
}

/// Strip `@` prefix from a binding name. Returns (actual_name, is_mutable).
pub(super) fn strip_at_prefix(name: &str) -> (&str, bool) {
    if let Some(stripped) = name.strip_prefix('@') {
        (stripped, true)
    } else {
        (name, false)
    }
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
    arena: &'a mut BindingArena,
    scopes: Vec<Scope>,
    /// Captures for the current function being analyzed
    current_captures: Vec<CaptureInfo>,
    /// Captures from the parent function (for nested closures)
    parent_captures: Vec<CaptureInfo>,
    /// The binding whose initializer is currently being analyzed — the analyzer
    /// analogue of the lowerer's `current_function_binding` (lir/lower/binding.rs).
    /// When a lookup inside this initializer's lambda resolves back to this same
    /// binding (a self-edge across the lambda boundary), the capture is classified
    /// `CaptureKind::Recursive` rather than a sibling `Local` (scopes.rs::lookup).
    /// Saved/restored around each initializer by `analyze_initializer`, so it names
    /// the nearest enclosing `letrec`/`def` binding. Paired with
    /// `current_init_binding_depth`: a reference is a self-edge only when it resolves
    /// to this binding from *directly* inside its initializer lambda, one function
    /// level deeper — a reference from a further-nested lambda is a sibling capture of
    /// that inner lambda, not this binding's self-edge (scopes.rs::lookup).
    current_init_binding: Option<Binding>,
    /// The `fn_depth` at the point `analyze_initializer` set `current_init_binding`
    /// (the depth of the enclosing `letrec`/`def`, one level *outside* the initializer
    /// lambda). A self-edge fires only at `current_init_binding_depth + 1` — the
    /// initializer lambda's own body — so a reference from a deeper nested lambda,
    /// which sits at a greater depth, classifies as that inner lambda's sibling
    /// capture rather than this binding's self-edge.
    current_init_binding_depth: u32,
    /// Maps Binding -> known signal of the bound value (if it's a callable)
    /// This enables interprocedural signal tracking: when we call a function,
    /// we can look up its signal and propagate it to the call site.
    signal_env: HashMap<Binding, Signal>,
    /// Maps SymbolId -> Signal for primitive functions
    /// Built from `register_primitive_signals` and passed in at construction
    primitive_signals: HashMap<SymbolId, Signal>,
    /// Arity environment: maps local function bindings to their arity.
    /// Populated by `bind_primitives` for primitive bindings; user
    /// shadows create new bindings that won't be in this map,
    /// correctly disabling the primitive arity check.
    arity_env: HashMap<Binding, Arity>,

    /// Signal projections for bindings initialized from imported modules.
    /// Maps a binding to a keyword→signal projection so that qualified
    /// access (`module:field`) uses the projected signal instead of the
    /// conservative `Polymorphic` fallback.
    projection_env: HashMap<Binding, HashMap<String, Signal>>,
    /// Compile-time squelch result signal. Set during call analysis when
    /// the analyzer detects `(squelch f mask)` and computes the resulting
    /// closure's signal statically. Consumed by binding analysis to seed
    /// the binding's signal_env entry.
    last_squelch_signal: Option<Signal>,
    /// Import projection detected during call analysis. Set when the
    /// analyzer sees `((import "literal"))` and the target file has a
    /// projection. Consumed by binding analysis to populate projection_env.
    last_import_projection: Option<HashMap<String, Signal>>,
    /// Tracks signal sources within the current lambda body for polymorphic inference
    current_signal_sources: SignalSources,
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
    /// Accumulated parameter bounds from silence forms in current lambda.
    /// Populated by `analyze_silence`, consumed by `analyze_lambda`.
    current_param_bounds: HashMap<Binding, Signal>,
    /// Accumulated function-level constraint from silence forms in current lambda.
    /// Populated by `analyze_silence`, consumed by `analyze_lambda`.
    current_declared_ceiling: Option<Signal>,
    /// Accumulated muffle bits from muffle forms in current lambda.
    /// Populated by `analyze_muffle`, consumed by `analyze_lambda`.
    current_muffle_bits: crate::value::fiber::SignalBits,
    /// Accumulated non-fatal errors. Recoverable error sites (undefined var,
    /// signal mismatch) push here and return `Ok(Hir::error(span))` to
    /// continue analysis. The pipeline checks this after analysis.
    errors: Vec<LError>,
    /// Set by `(silence!)` assertion form. Consumed by `analyze_lambda`.
    current_silence_assert: bool,
    /// Set by `(numeric!)` assertion form. Consumed by `analyze_lambda`.
    current_numeric_assert: bool,
    /// Signal projection computed by `analyze_file_letrec`. Retrieved by
    /// the pipeline to store on `Bytecode.signal_projection`.
    last_signal_projection: Option<HashMap<String, Signal>>,
    /// Set by `(immutable! x)` assertion form. Consumed by `analyze_lambda`.
    current_immutability_asserts: HashSet<Binding>,
    /// When true, bindings without `@` prefix are immutable.
    /// Gated on epoch >= 8; epoch <= 7 files are mutable-by-default.
    immutable_by_default: bool,
    /// User signals declared in THIS compilation (`(signal :kw)`). Used to
    /// reject duplicate declarations within one compile while allowing the same
    /// signal to be re-declared by a SEPARATE compile — the process-global signal
    /// registry persists across compiles, so the test runner recompiling a file
    /// once per tier would otherwise collide ("already registered").
    signals_declared: HashSet<String>,
    /// The owning instance's compile context, for resolving `(import "literal")`
    /// signal projections during analysis (`get_or_compile_projection`). Set by
    /// the file frontend via [`set_compile_ctx`](Analyzer::set_compile_ctx); the
    /// frontend owns the `CompileCtx`, outlives this analyzer, and never touches
    /// it while analysis runs, so the reborrow is sound. `None` in pure-analysis
    /// contexts (lint/LSP/tests), where imports fall back to the conservative
    /// `Polymorphic` projection.
    import_ctx: Option<*mut crate::pipeline::CompileCtx>,
}

mod scopes;

impl<'a> Analyzer<'a> {
    /// Create a new analyzer without primitive signals or arities
    pub fn new(symbols: &'a mut SymbolTable, arena: &'a mut BindingArena) -> Self {
        Self::new_with_primitives(symbols, arena, HashMap::new(), HashMap::new())
    }

    /// Create a new analyzer with primitive signals for interprocedural tracking
    /// (convenience wrapper that passes empty arities)
    pub fn new_with_primitive_signals(
        symbols: &'a mut SymbolTable,
        arena: &'a mut BindingArena,
        primitive_signals: HashMap<SymbolId, Signal>,
    ) -> Self {
        Self::new_with_primitives(symbols, arena, primitive_signals, HashMap::new())
    }

    /// Create a new analyzer with primitive signals and arities
    pub fn new_with_primitives(
        symbols: &'a mut SymbolTable,
        arena: &'a mut BindingArena,
        primitive_signals: HashMap<SymbolId, Signal>,
        _primitive_arities: HashMap<SymbolId, Arity>,
    ) -> Self {
        let mut analyzer = Analyzer {
            symbols,
            arena,
            scopes: Vec::new(),
            current_captures: Vec::new(),
            parent_captures: Vec::new(),
            current_init_binding: None,
            current_init_binding_depth: 0,
            signal_env: HashMap::new(),
            primitive_signals,
            arity_env: HashMap::new(),

            projection_env: HashMap::new(),
            last_squelch_signal: None,
            last_import_projection: None,
            current_signal_sources: SignalSources::default(),
            current_lambda_params: Vec::new(),
            block_contexts: Vec::new(),
            next_block_id: 0,
            fn_depth: 0,
            pre_bindings: HashMap::new(),
            primitive_values: HashMap::new(),
            current_param_bounds: HashMap::new(),
            current_declared_ceiling: None,
            current_muffle_bits: crate::value::fiber::SignalBits::EMPTY,
            errors: Vec::new(),
            current_silence_assert: false,
            current_numeric_assert: false,
            last_signal_projection: None,
            current_immutability_asserts: HashSet::new(),
            immutable_by_default: true,
            signals_declared: HashSet::new(),
            import_ctx: None,
        };
        // Initialize with a global scope so top-level bindings can be registered
        analyzer.push_scope(false);
        analyzer
    }

    /// Provide the owning instance's compile context so that `(import
    /// "literal")` forms resolve their signal projection during analysis. Called
    /// by the file frontend, which owns the `CompileCtx` for the analyzer's
    /// whole lifetime. See the `import_ctx` field.
    pub fn set_compile_ctx(&mut self, cctx: &mut crate::pipeline::CompileCtx) {
        self.import_ctx = Some(cctx as *mut _);
    }

    /// Declare a user signal `(signal :kw)`. Rejects a duplicate declaration
    /// WITHIN this compilation, but re-declaring a user signal across SEPARATE
    /// compilations reuses its registry bit (idempotent) — re-declaring a builtin
    /// still errors. `span` prefixes the error message.
    pub(crate) fn declare_signal(
        &mut self,
        keyword: &str,
        span: &crate::syntax::Span,
    ) -> Result<(), String> {
        if !self.signals_declared.insert(keyword.to_string()) {
            return Err(format!("{}: Signal '{}' already registered", span, keyword));
        }
        let mut reg = crate::signals::registry::global_registry()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        reg.register_or_get(keyword)
            .map(|_| ())
            .map_err(|e| format!("{}: {}", span, e))
    }

    /// Analyze a syntax tree into HIR
    pub fn analyze(&mut self, syntax: &crate::syntax::Syntax) -> Result<AnalysisResult, String> {
        let hir = self.analyze_expr(syntax)?;
        let errors = std::mem::take(&mut self.errors);
        Ok(AnalysisResult { hir, errors })
    }

    /// Analyze a binding's initializer with `current_init_binding` set to `binding`
    /// and `current_init_binding_depth` to the current `fn_depth`, so a self-reference
    /// resolving back to `binding` from *directly* inside the initializer's lambda
    /// (one function level deeper, `scopes.rs::lookup`) classifies
    /// `CaptureKind::Recursive` rather than a sibling `Local`. The depth guard is what
    /// keeps a reference from a further-nested lambda a sibling capture of that inner
    /// lambda, not this binding's self-edge — `analyze_lambda` leaves the field
    /// untouched, so without the depth check the context would leak into every nested
    /// lambda and mis-materialize their self-references. Saved/restored so a nested
    /// initializer (an inner `letrec`/`def`) names its own binding at its own depth and
    /// the outer one is restored after.
    pub(crate) fn analyze_initializer(
        &mut self,
        binding: Binding,
        value: &Syntax,
    ) -> Result<Hir, String> {
        let saved = self.current_init_binding.replace(binding);
        let saved_depth = self.current_init_binding_depth;
        self.current_init_binding_depth = self.fn_depth;
        let result = self.analyze_expr(value);
        self.current_init_binding = saved;
        self.current_init_binding_depth = saved_depth;
        result
    }

    /// Accumulate a non-fatal error and return a poison node.
    /// Used at recoverable error sites to continue analysis.
    fn accumulate_error(&mut self, error: LError, span: &Span) -> Hir {
        self.errors.push(error);
        Hir::error(span.clone())
    }

    /// Return accumulated errors (for the pipeline to check).
    pub fn take_errors(&mut self) -> Vec<LError> {
        std::mem::take(&mut self.errors)
    }

    /// Take the signal projection computed by `analyze_file_letrec`.
    pub fn take_signal_projection(&mut self) -> Option<HashMap<String, Signal>> {
        self.last_signal_projection.take()
    }

    /// Set whether bindings without `@` are immutable by default.
    /// Epoch >= 8 enables this; epoch <= 7 disables it.
    pub fn set_immutable_by_default(&mut self, v: bool) {
        self.immutable_by_default = v;
    }

    /// Levenshtein edit distance between two strings.
    fn levenshtein(a: &str, b: &str) -> usize {
        let m = a.len();
        let n = b.len();
        if m == 0 {
            return n;
        }
        if n == 0 {
            return m;
        }

        let mut prev: Vec<usize> = (0..=n).collect();
        let mut curr = vec![0; n + 1];

        for (i, ca) in a.chars().enumerate() {
            curr[0] = i + 1;
            for (j, cb) in b.chars().enumerate() {
                let cost = if ca == cb { 0 } else { 1 };
                curr[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(curr[j] + 1);
            }
            std::mem::swap(&mut prev, &mut curr);
        }
        prev[n]
    }

    /// Find bindings in scope with names similar to `name` (edit distance <= 2).
    fn suggest_similar(&self, name: &str) -> Vec<String> {
        let mut candidates: Vec<(usize, String)> = Vec::new();
        for scope in self.scopes.iter().rev() {
            for scope_name in scope.bindings.keys() {
                let dist = Self::levenshtein(name, scope_name);
                if dist > 0 && dist <= 2 && !candidates.iter().any(|(_, n)| n == scope_name) {
                    candidates.push((dist, scope_name.clone()));
                }
            }
        }
        candidates.sort_by_key(|(d, _)| *d);
        candidates.into_iter().map(|(_, n)| n).take(3).collect()
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
        for (&sym_id, &signal) in &meta.signals {
            let binding = self.bind_by_sym(sym_id, BindingScope::Local);
            self.arena.get_mut(binding).is_immutable = true;
            self.arena.get_mut(binding).is_primitive = true;
            self.signal_env.insert(binding, signal);
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

    /// Bind compile-time values (from `begin-for-syntax`) into the Analyzer's
    /// current scope as immutable local bindings backed by constant values.
    ///
    /// Called from `eval_syntax` after `bind_primitives` so that compile-time
    /// names are visible in macro body analysis. The Lowerer emits `LoadConst`
    /// for these bindings (same mechanism as primitive functions).
    ///
    /// `env`: map from name string to Value, from `Expander.compile_time_env`.
    pub fn bind_compile_time_env(
        &mut self,
        env: &std::collections::HashMap<String, crate::value::Value>,
    ) {
        for (name, value) in env {
            let sym = self.symbols.intern(name);
            let binding = self.bind_by_sym(sym, BindingScope::Local);
            self.arena.get_mut(binding).is_immutable = true;
            self.primitive_values.insert(binding, *value);
        }
    }

    // === Scope Management ===
}

#[cfg(test)]
mod tests;
