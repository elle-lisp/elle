//! Arena-backed binding storage for the compilation pipeline.
//!
//! `BindingArena` owns all `BindingInner` values for a compilation unit.
//! It is created by the pipeline entry point, borrowed mutably by the
//! `Analyzer`, and borrowed immutably by the `Lowerer`.
//!
//! `BindingInner` and `BindingScope` are compile-time-only data and do not
//! belong in the runtime value system.

use super::binding::Binding;
use crate::value::SymbolId;

/// Where a binding lives at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingScope {
    /// Lambda parameter
    Parameter,
    /// Local variable (let-bound, define inside function)
    Local,
}

/// Internal binding metadata.
#[derive(Debug)]
pub struct BindingInner {
    /// Original symbol name (for error messages and global lookup)
    pub name: SymbolId,
    /// Where this binding lives
    pub scope: BindingScope,
    /// Whether this binding has been mutated via assign
    pub is_mutated: bool,
    /// Whether this binding is captured by a nested closure. **Module-private**:
    /// the lexical-capture proxy has NO escape authority (escape.md "Lexical
    /// capture is demoted to a structural hint"), so no consumer may name it for an
    /// escape or reachability decision. Written through `mark_captured`; read by a
    /// SINGLE consumer — `needs_capture()`, for cell layout. Escape's capture facet
    /// is flow-true (transitive `lambda_captures` propagation from genuine frontier
    /// seeds, never this proxy), and the region solver's reachability questions read
    /// the HIR capture-graph (`regions::escape::captured_bindings`); neither reads
    /// this field, and there is no getter through which they could.
    is_captured: bool,
    /// Whether this binding is immutable (def)
    pub is_immutable: bool,
    /// Whether this binding was pre-created before its initializer runs
    /// (begin pass 1, letrec pass 1). Pre-bound immutable locals still
    /// need cells because they may be captured before initialization
    /// (self-recursion, forward references).
    pub is_prebound: bool,
    /// Whether this prebound binding's initializer has not yet been
    /// analyzed (letrec*/fn-body Pass 2 clears it after each initializer).
    /// A direct value read while pending at the SAME function depth is a
    /// use-before-init compile error (docs/bindings.md "Use before
    /// initialization is an error"); a read through a lambda (deeper
    /// `fn_depth`) is the legal deferred forward reference.
    pub init_pending: bool,
    /// The analyzer's `fn_depth` at the prebind site, compared against the
    /// reference's depth by the use-before-init check above.
    pub prebind_fn_depth: u32,
    /// Whether this binding is a primitive or stdlib function injected
    /// by `bind_primitives`. Primitive bindings are universally available
    /// in any compilation unit — they never need to be passed as parameters
    /// or captured as upvalues across extraction boundaries.
    pub is_primitive: bool,
    /// Whether this binding is a compiler-generated temporary with no
    /// source-level name the user wrote (file-letrec statement wrappers,
    /// signal-declaration gensyms, destructure temporaries). Synthetic
    /// bindings are excluded from user-facing reification such as
    /// `(environment)`. Classification is structural — this flag — so a user
    /// binding whose spelling happens to start with `__` is not misclassified.
    pub is_synthetic: bool,
    /// Whether a `(numeric!)` declaration floors this binding at Number. Set on
    /// every parameter of a declaring function (the declaration is *about* its
    /// parameters), and read by type inference wherever the binding gets a type,
    /// so the floor discharges a `%`-intrinsic's operand contract in the body.
    ///
    /// It lives on the BINDING, not on the lambda node, because a rewrite may
    /// dissolve the function while keeping its parameter: HOF loop fusion splices
    /// a kernel body into a loop, retyping the parameter as a `let`-bound local
    /// (`typeinfer/fuse.rs`, docs/impl/dissolution.md § "Raw `%`-intrinsic
    /// bodies"). Carried on the binding, the declared floor survives that splice,
    /// so the spliced intrinsic proves exactly as it did inside the function.
    pub declared_numeric: bool,
    /// Whether this binding is a MODULE-SCOPE (file-letrec) name — a direct
    /// binding of `analyze_file_letrec` (top-level `def`/`var`/expr statement).
    /// Such a binding's lifetime is the whole module/program: its demise is the
    /// file-letrec scope-region teardown, NOT a per-activation scope exit, even
    /// when the file-letrec runs inside the synthetic `%file-body` thunk
    /// (`compile/whole-module`, where `in_lambda` is spuriously true). The region
    /// solver uses this to classify a reassigned module mutable as a top-level
    /// (program-extent) 1-slot container rather than a fn-local one — the two
    /// suppress different decrefs (see `record_top_level_reassign`).
    pub is_file_scope: bool,
}

impl BindingInner {
    /// A binding needs a cell if captured (for locals) or mutated (for params).
    ///
    /// Immutable locals skip cell wrapping — they are captured by value.
    /// Exception: pre-bound immutable locals still need cells because they
    /// may be captured before their initializer runs (self-recursion,
    /// forward references).
    pub fn needs_capture(&self) -> bool {
        match self.scope {
            BindingScope::Local => self.is_captured && (!self.is_immutable || self.is_prebound),
            BindingScope::Parameter => self.is_mutated,
        }
    }

    /// A capture cell whose content is RE-STORED over the binding's life — a
    /// `@`-mutable captured local or a mutated captured parameter. A strict subset
    /// of [`needs_capture`](Self::needs_capture): it EXCLUDES the prebound *immutable*
    /// letrec cell (content set once, not re-stored).
    ///
    /// This is the RE-STORE predicate: a whole-value read of such a cell needs a
    /// counted reference because the next re-store (`capture_store_with_rebind`)
    /// decrefs the displaced prior (`RegionInfo::counted_cell_read_sites`,
    /// regions.rs). It is NOT the forest's "capture is a borrow" predicate — that
    /// is the broader [`needs_capture`](Self::needs_capture), which folds in the
    /// prebound letrec cell too (a capture of ANY cell-materialized binding is a
    /// borrow through a separately-owned env cell, never a containment the closure
    /// owns; `regions::ownership::capture::capture_containment_edges`). The letrec
    /// cell is not re-stored, so it needs no counted read, but it IS a cell borrow,
    /// so its capture yields no containment edge — the two concerns split here.
    pub fn is_restorable_capture_cell(&self) -> bool {
        match self.scope {
            BindingScope::Local => self.is_captured && !self.is_immutable,
            BindingScope::Parameter => self.is_mutated,
        }
    }

    /// Mark this binding as captured by a nested closure. The analyzer's sole
    /// writer (`analyze::scopes::lookup`), called when a name resolves across a
    /// function boundary. Routing the write through a setter keeps the field
    /// module-private so a future reader cannot silently re-couple the solver to
    /// the proxy.
    pub(in crate::hir) fn mark_captured(&mut self) {
        self.is_captured = true;
    }

    /// Does a `letrec` binding's forward cell lower as a COMPILED
    /// `MakeCaptureCell` held in the binding's own (stack) slot? True at top
    /// level for every captured binding (the pre-pass cell every sibling
    /// captures), and inside a lambda body for the recursive-closure shape —
    /// immutable, never mutated, lambda-initialized — so the cell is a
    /// static-slot allocation the closure-cycle merge can collapse with its SCC
    /// (docs/impl/region/letrec.md § The letrec closure-cycle merge). Any other
    /// in-lambda captured letrec binding keeps the runtime `populate_env`
    /// env-cell route (`StoreCapture`; docs/impl/region/bindings.md "Env cells
    /// in loops"). The region walk's Letrec arm and `lower_letrec` both read
    /// this one predicate — the walk must mirror the lowerer's `MakeCaptureCell`
    /// sites exactly, or a cell region is a phantom (no allocation) or missing
    /// (an allocation with no region).
    pub fn letrec_compiled_cell(&self, init_is_lambda: bool, in_lambda: bool) -> bool {
        self.needs_capture()
            && (!in_lambda || (init_is_lambda && self.is_immutable && !self.is_mutated))
    }
}

/// Arena for compile-time bindings.
///
/// Bindings are allocated during analysis (`&mut self`) and read during
/// lowering (`&self`). The arena is dropped at the end of the compilation
/// unit — no leaks.
///
/// A `Binding(u32)` index is only valid for the arena that created it.
#[derive(Debug)]
pub struct BindingArena {
    bindings: Vec<BindingInner>,
}

impl BindingArena {
    /// Create an empty arena.
    pub fn new() -> Self {
        Self {
            bindings: Vec::new(),
        }
    }

    /// Allocate a new binding. Analysis phase only.
    pub fn alloc(&mut self, name: SymbolId, scope: BindingScope) -> Binding {
        let index = self.bindings.len() as u32;
        self.bindings.push(BindingInner {
            name,
            scope,
            is_mutated: false,
            is_captured: false,
            is_immutable: false,
            is_prebound: false,
            init_pending: false,
            prebind_fn_depth: 0,
            is_primitive: false,
            is_synthetic: false,
            declared_numeric: false,
            is_file_scope: false,
        });
        Binding(index)
    }

    /// Allocate a synthetic binding with no source-level identity.
    /// Used by compiler passes that need temporaries (e.g., phi-insertion
    /// condition bindings). The name is set to `SymbolId::SYNTHETIC`.
    pub fn gensym(&mut self) -> Binding {
        let b = self.alloc(SymbolId::SYNTHETIC, BindingScope::Local);
        self.get_mut(b).is_synthetic = true;
        b
    }

    /// Read-only access. Available in both analysis and lowering phases.
    pub fn get(&self, binding: Binding) -> &BindingInner {
        &self.bindings[binding.0 as usize]
    }

    /// Mutable access. Analysis phase only (requires `&mut self`).
    pub fn get_mut(&mut self, binding: Binding) -> &mut BindingInner {
        &mut self.bindings[binding.0 as usize]
    }

    /// Number of bindings in the arena.
    pub fn len(&self) -> usize {
        self.bindings.len()
    }

    /// Returns true if the arena contains no bindings.
    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }
}

impl Default for BindingArena {
    fn default() -> Self {
        Self::new()
    }
}
