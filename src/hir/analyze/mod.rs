// audited: 2026-10-25
//! Syntax to HIR analysis
//!
//! docs/impl/hir.md
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
//! - `assign` invalidates signal tracking for the mutated binding
//! - Once the pass is over, the unit's signals are solved together (src/hir/analyze/settle.rs)

mod binding;
mod call;
mod destructure;
mod env;
mod facts;
mod fileletrec;
mod letrec;
pub use fileletrec::classify_form;
pub(crate) mod forms;
mod lambda;
mod settle;
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
/// these variants before passing them to `Analyzer::analyze_file`.
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
    /// Accumulated non-fatal errors: the undefined names
    pub errors: Vec<LError>,
}

/// Tracks signal sources within a lambda body, separating inherent
/// signals (emitted directly or by non-parameter callees) from
/// parameter-dependent signals (polymorphic propagation).
#[derive(Debug, Clone)]
struct SignalSources {
    /// Parameters whose calls contribute signals.
    param_calls: HashSet<Binding>,
    /// Bits the function raises by itself: its direct `emit` forms, a
    /// `match` with no total arm, and each construct docs/signals/inference.md
    /// lists under "What raises".
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

/// One parameter bound as a caller sees it: the argument position it checks,
/// the parameter it binds, and the signals the argument may emit.
#[derive(Debug, Clone, Copy)]
struct ParamBoundAt {
    position: usize,
    param: Binding,
    allowed: crate::value::fiber::SignalBits,
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
    /// Bindings in this scope, keyed by the name's `SymbolId`. Multiple
    /// bindings per name are possible when macro expansion introduces bindings
    /// with different scope sets.
    ///
    /// The key is the id, not the spelling, because resolution is an identity
    /// question: the id is the name's hash, so it answers without a display
    /// memo (docs/impl/symbol.md). A name-keyed map would make resolution
    /// depend on which memo had learned the spelling. `bind_primitives`
    /// receives the primitives as ids from the compile context, whose
    /// spellings this analyzer's memo may never learn.
    bindings: HashMap<SymbolId, Vec<ScopedBinding>>,
    /// Is this a function scope (creates new capture boundary)
    is_function: bool,
    /// Is this a definition-environment frame — the global frame or a file's
    /// top-level letrec frame? These hold the bindings a macro template's
    /// free variables resolve to, so they are exempt from the referential-
    /// transparency rule in `lookup()` (an intro-scoped reference skips
    /// use-site locals but must still see top-level definitions).
    definition_env: bool,
    /// Expansion provenance: the intro scopes carried by the form that
    /// opened this frame (snapshot of `Analyzer::current_form_intros` at
    /// push time). A binding form from a template carries its expansion's
    /// intro scope, so a reference from the same expansion may resolve into
    /// its frame even when the binder lacks the scope (deliberate capture
    /// through datum->syntax). A user-written form has no intro scopes, so
    /// its frame is invisible to template references.
    intro_provenance: Vec<ScopeId>,
    /// Next local index for this scope (used only for tracking local count)
    next_local: u16,
}

impl Scope {
    fn with_start_index(
        is_function: bool,
        start_index: u16,
        intro_provenance: Vec<ScopeId>,
    ) -> Self {
        Scope {
            bindings: HashMap::new(),
            is_function,
            definition_env: false,
            intro_provenance,
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
    /// The intro scopes of the form currently being analyzed (set by
    /// `analyze_expr`, save/restore bracketed). `push_scope` snapshots it
    /// into the new frame's `intro_provenance`.
    current_form_intros: Vec<ScopeId>,
    /// The binding whose initializer is currently being analyzed — the analyzer
    /// analogue of the lowerer's `current_function_binding` (src/lir/lower/mod.rs).
    /// When a lookup inside this initializer's lambda resolves back to this same
    /// binding (a self-edge across the lambda boundary), the capture is classified
    /// `CaptureKind::Recursive` rather than a sibling `Local` (src/hir/analyze/scopes.rs).
    /// Saved/restored around each initializer by `analyze_initializer`, so it names
    /// the nearest enclosing `letrec`/`def` binding. A reference is a self-edge only
    /// from directly inside the initializer lambda, one function level deeper. A
    /// reference from a further-nested lambda is a sibling capture of that lambda.
    current_init_binding: Option<Binding>,
    /// The `fn_depth` at the point `analyze_initializer` set `current_init_binding`
    /// (the depth of the enclosing `letrec`/`def`, one level *outside* the initializer
    /// lambda). A self-edge fires only at this depth plus one, the initializer
    /// lambda's own body.
    current_init_binding_depth: u32,
    /// The known signal of each binding whose value is a callable. A call looks
    /// its callee up here and propagates the signal to the call site.
    signal_env: HashMap<Binding, Signal>,
    /// Maps SymbolId -> Signal for primitive functions, from the
    /// `PrimitiveMeta` the pipeline passes to `new_with_primitives`. Read only
    /// for a binding that is the primitive itself (`primitive_signal_of`).
    primitive_signals: HashMap<SymbolId, Signal>,
    /// Arity environment: maps function bindings to their arity. Populated by
    /// `bind_primitives` for primitives and by the binding forms for a binding
    /// initialized with a lambda. A user shadow is a new binding, so it
    /// carries its own arity or none — never the primitive's.
    arity_env: HashMap<Binding, Arity>,
    /// The bindings initialized with a lambda that collects `&keys` or
    /// `&named`. A call to one checks its keyword arguments at the call, so
    /// the call carries `:error` (docs/signals/inference.md, "What raises").
    keyword_collectors: HashSet<Binding>,
    /// The parameter bounds of each binding initialized with a lambda that
    /// declares one: the parameter's position, its binding and the bound. A
    /// call checks an argument whose signal it knows against them
    /// (docs/signals/inference.md).
    bound_env: HashMap<Binding, Vec<ParamBoundAt>>,

    /// Compile-time squelch result signal. Set during call analysis when
    /// the analyzer meets `(squelch spec f)` or `(attune spec f)` with a
    /// literal spec, and computes the resulting closure's signal statically.
    /// Consumed by binding analysis to seed the binding's signal_env entry.
    last_squelch_signal: Option<Signal>,
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
    /// `immutable_values` map from this, so a primitive reference emits
    /// `LoadConst` and needs no slot.
    primitive_values: HashMap<Binding, Value>,
    /// The parameter bounds `(bound p spec)` declares in the current lambda.
    /// Populated by `analyze_bound`, consumed by `analyze_lambda`.
    current_param_bounds: HashMap<Binding, Signal>,
    /// The ceiling `(attune! spec)` and `(silent!)` declare in the current
    /// lambda, the intersection when there are several. Populated by
    /// `declare_ceiling`, kept by `analyze_lambda` in its `LambdaDecl`, and
    /// checked once the unit is settled.
    current_declared_ceiling: Option<Signal>,
    /// Accumulated non-fatal errors. A recoverable error site, an undefined
    /// name, pushes here and returns `Ok(Hir::error(span))` to continue
    /// analysis. The pipeline checks this after analysis.
    errors: Vec<LError>,
    /// Set by `(numeric!)` assertion form. Consumed by `analyze_lambda`.
    current_numeric_assert: bool,
    /// Set by `(immutable! x)` assertion form. Consumed by `analyze_lambda`.
    current_immutability_asserts: HashSet<Binding>,
    /// When true, bindings without `@` prefix are immutable.
    /// Gated on epoch >= 8; epoch <= 7 files are mutable-by-default.
    immutable_by_default: bool,
    /// The Unicode generation this program is locked to. `(unicode! …)`
    /// declarations are checked against it and the 0-arg query folds to
    /// its version. Wired from the CompileCtx by the pipeline; analyzers
    /// with no VM behind them (LSP, lint) keep the process default.
    unicode_generation: crate::segment::Generation,
    /// Each lambda's declared ceiling and the bits it raises by itself, keyed
    /// by the lambda node. The HIR keeps the inferred signal, which mixes in
    /// what the body's calls raise, and drops the ceiling it was checked
    /// against; a reader that solves signals itself needs both.
    lambda_decls: HashMap<super::expr::HirId, LambdaDecl>,
    /// Each call whose callee bounds a parameter, with the arguments those
    /// bounds check once the unit is solved.
    bound_calls: Vec<call::BoundCall>,
    /// Each call through a binding whose count did not match the binding's
    /// arity, reported once the pass is over unless an `assign` reaches it.
    count_mismatches: Vec<call::CountMismatch>,
}

/// What a lambda's body declared about its own signal, and what it raises by
/// itself.
#[derive(Debug, Clone, Copy)]
pub struct LambdaDecl {
    /// `(silent!)` or `(attune! spec)`: the signals the function may emit.
    pub ceiling: Option<Signal>,
    /// The bits the body raises by itself, whatever it calls: its emits, a
    /// bound's entry check, and each construct docs/signals/inference.md
    /// lists under "What raises".
    pub direct: crate::value::fiber::SignalBits,
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

    /// Create a new analyzer with primitive signals. The arities argument is
    /// unused: `bind_primitives` reads each primitive's arity from its
    /// `PrimitiveMeta`.
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
            current_form_intros: Vec::new(),
            current_init_binding: None,
            current_init_binding_depth: 0,
            signal_env: HashMap::new(),
            primitive_signals,
            arity_env: HashMap::new(),
            keyword_collectors: HashSet::new(),
            bound_env: HashMap::new(),
            last_squelch_signal: None,
            current_signal_sources: SignalSources::default(),
            current_lambda_params: Vec::new(),
            block_contexts: Vec::new(),
            next_block_id: 0,
            fn_depth: 0,
            pre_bindings: HashMap::new(),
            primitive_values: HashMap::new(),
            current_param_bounds: HashMap::new(),
            current_declared_ceiling: None,
            errors: Vec::new(),
            current_numeric_assert: false,
            current_immutability_asserts: HashSet::new(),
            immutable_by_default: true,
            unicode_generation: crate::config::get().unicode_generation(),
            lambda_decls: HashMap::new(),
            bound_calls: Vec::new(),
            count_mismatches: Vec::new(),
        };
        // Initialize with a global scope so top-level bindings can be registered
        analyzer.push_definition_scope();
        analyzer
    }

    /// Declare a user signal `(signal :kw)`. A keyword that is already a user
    /// signal keeps its bit, whichever compile declared it first, so every
    /// declaration of a keyword names one signal; a builtin keyword is an
    /// error. `span` prefixes the error message.
    pub(crate) fn declare_signal(
        &mut self,
        keyword: &str,
        span: &crate::syntax::Span,
    ) -> Result<(), String> {
        let mut reg = crate::signals::registry::global_registry()
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        reg.register_or_get(keyword)
            .map(|_| ())
            .map_err(|e| format!("{}: {}", span, e))
    }

    /// Analyze a syntax tree into HIR, and settle its signals.
    pub fn analyze(&mut self, syntax: &crate::syntax::Syntax) -> Result<AnalysisResult, String> {
        // Keyword pre-pass: keywords reach HIR through many arms (literals,
        // pattern keys, struct literals, quoted data), so their spellings are
        // learned in one walk rather than per arm.
        crate::syntax::convert::learn_keywords(syntax, self.symbols);
        let mut hir = self.analyze_expr(syntax)?;
        self.settle(&mut hir)?;
        let errors = std::mem::take(&mut self.errors);
        Ok(AnalysisResult { hir, errors })
    }

    /// Analyze a file's top-level forms, each classified by `classify_form`,
    /// into one `HirKind::Letrec`, and settle its signals.
    pub fn analyze_file(&mut self, forms: Vec<FileForm>, span: Span) -> Result<Hir, String> {
        let mut hir = self.analyze_file_letrec(forms, span)?;
        self.settle(&mut hir)?;
        Ok(hir)
    }

    /// Analyze a binding's initializer with `current_init_binding` set to `binding`
    /// and `current_init_binding_depth` to the current `fn_depth`. A self-reference
    /// to `binding` from directly inside the initializer's lambda then classifies
    /// `CaptureKind::Recursive` rather than a sibling `Local` (src/hir/analyze/scopes.rs).
    /// `analyze_lambda` leaves both fields untouched, so the depth check keeps a
    /// further-nested lambda's reference a sibling capture of that lambda. Both are
    /// saved and restored, so a nested initializer (an inner `letrec`/`def`) names
    /// its own binding at its own depth.
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
        Hir::error(*span)
    }

    /// Return accumulated errors (for the pipeline to check).
    pub fn take_errors(&mut self) -> Vec<LError> {
        std::mem::take(&mut self.errors)
    }

    /// Take every lambda's declared ceiling and the bits it raises by itself.
    pub fn take_lambda_decls(&mut self) -> HashMap<super::expr::HirId, LambdaDecl> {
        std::mem::take(&mut self.lambda_decls)
    }

    /// Take the bits the code outside every lambda raised by itself, as
    /// `LambdaDecl::direct` holds them for a lambda.
    pub fn take_top_direct(&mut self) -> crate::value::fiber::SignalBits {
        std::mem::replace(
            &mut self.current_signal_sources.direct_bits,
            crate::value::fiber::SignalBits::EMPTY,
        )
    }

    /// Set whether bindings without `@` are immutable by default.
    /// Epoch >= 8 enables this; epoch <= 7 disables it.
    pub fn set_immutable_by_default(&mut self, v: bool) {
        self.immutable_by_default = v;
    }

    /// Set the Unicode generation `(unicode! …)` declarations check against.
    pub fn set_unicode_generation(&mut self, gen: crate::segment::Generation) {
        self.unicode_generation = gen;
    }
}

#[cfg(test)]
mod tests;
