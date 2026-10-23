// audited: 2026-10-21
//! A function literal as the analyzer leaves it: its parameters, captures and body, and what a call to it signals.
//!
//! docs/impl/hir.md

use super::Hir;
use crate::hir::binding::{Binding, CaptureInfo};
use crate::hir::fragment::LambdaFragment;
use crate::signals::Signal;
use crate::syntax::Span;
use crate::value::Arity;
use std::rc::Rc;

/// A declared signal bound on a function parameter.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ParamBound {
    pub binding: Binding,
    pub signal: Signal,
}

/// How extra arguments beyond fixed params are collected.
/// Only meaningful when `rest_param` is `Some`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum VarargKind {
    /// Collect into a list (`&`).
    List,
    /// Collect into an immutable struct (`&keys`)
    Struct,
    /// Collect into an immutable struct (`&named`) with strict key validation.
    /// Contains the set of valid keyword names.
    StrictStruct(Vec<String>),
}

/// The payload of `HirKind::Lambda`. Build one with [`Lambda::new`] and the
/// setters beside it, and swap its body with [`Lambda::with_body`].
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Lambda {
    pub params: Vec<Binding>,
    /// Number of required parameters (before &opt).
    /// When no &opt, equals the count of fixed params
    /// (params.len() if no rest_param, params.len() - 1 if rest_param).
    pub num_required: usize,
    /// If present, this function is variadic: extra args are collected
    /// into a list or struct and bound to this parameter.
    pub rest_param: Option<Binding>,
    /// How the rest parameter's args are collected.
    /// Only meaningful when rest_param is Some.
    pub vararg_kind: VarargKind,
    pub captures: Vec<CaptureInfo>,
    pub body: Hir,
    /// Number of local slots needed (params + locals)
    pub num_locals: u16,
    /// The inferred signal of a call to this lambda, which can differ from
    /// `body.signal`. A call to unbounded parameter i sets bit i of
    /// `propagates` instead of adding bits. A call to a parameter bounded by
    /// `(bound p spec)` adds the bound's bits.
    pub inferred_signals: Signal,
    /// Declared signal bounds for parameters (from `(bound param spec)`).
    /// Only parameters with explicit bounds appear here. The bounds feed
    /// `inferred_signals`, and the function checks each on entry
    /// (`CheckSignalBound`).
    pub param_bounds: Vec<ParamBound>,
    /// The docstring taken from the lambda body, if it has one. Plain
    /// compile-time string data, not a heap `Value`. The lowerer copies it to
    /// the function's template, and `(doc f)` builds a fresh string from it.
    #[serde(skip)]
    pub doc: Option<Rc<str>>,
    /// Where this lambda was written, for `(meta/origin f)`. A span, not
    /// the lambda's syntax tree: `meta/origin` reads a file, a line, and a
    /// column, and nothing reads the tree (docs/impl/syntax.md).
    #[serde(skip)]
    pub origin: Option<Span>,
    /// True if the function body contains `(numeric!)` assertion.
    /// The lowerer checks `is_gpu_eligible()` after lowering.
    pub assert_numeric: bool,
    /// The body closed for a later graft or fused loop, when it closes.
    /// Attached before functionalization, and encoded by the lowerer onto
    /// the function's code payload (docs/impl/graft.md).
    #[serde(skip)]
    pub fragment: Option<Rc<LambdaFragment>>,
}

impl Lambda {
    /// A lambda over `params` with `body`. It starts with no captures, zero
    /// local slots, a silent call and no bounds, docstring, origin,
    /// `(numeric!)` or fragment. The setters below fill in what the analyzer
    /// found.
    pub fn new(
        params: Vec<Binding>,
        num_required: usize,
        rest_param: Option<Binding>,
        vararg_kind: VarargKind,
        body: Hir,
    ) -> Self {
        Lambda {
            params,
            num_required,
            rest_param,
            vararg_kind,
            captures: Vec::new(),
            body,
            num_locals: 0,
            inferred_signals: Signal::silent(),
            param_bounds: Vec::new(),
            doc: None,
            origin: None,
            assert_numeric: false,
            fragment: None,
        }
    }

    /// What the lambda captures, and how many local slots its frame needs.
    pub fn captures(mut self, captures: Vec<CaptureInfo>, num_locals: u16) -> Self {
        self.captures = captures;
        self.num_locals = num_locals;
        self
    }

    /// The signal a call to the lambda emits, and the `(bound p spec)` bounds
    /// that signal was computed under.
    pub fn signal(mut self, inferred_signals: Signal, param_bounds: Vec<ParamBound>) -> Self {
        self.inferred_signals = inferred_signals;
        self.param_bounds = param_bounds;
        self
    }

    pub fn doc(mut self, doc: Option<Rc<str>>) -> Self {
        self.doc = doc;
        self
    }

    pub fn origin(mut self, origin: Option<Span>) -> Self {
        self.origin = origin;
        self
    }

    pub fn numeric(mut self, assert_numeric: bool) -> Self {
        self.assert_numeric = assert_numeric;
        self
    }

    /// How many arguments a call to this lambda accepts.
    pub fn arity(&self) -> Arity {
        Arity::for_lambda(
            self.rest_param.is_some(),
            self.num_required,
            self.params.len(),
        )
    }

    /// This lambda with `body` in place of its own, every other field kept.
    /// A pass that rewrites bodies rebuilds a lambda through this, so a field
    /// added here reaches every rebuilt lambda without the pass naming it.
    pub fn with_body(&self, body: Hir) -> Self {
        Lambda {
            params: self.params.clone(),
            num_required: self.num_required,
            rest_param: self.rest_param,
            vararg_kind: self.vararg_kind.clone(),
            captures: self.captures.clone(),
            body,
            num_locals: self.num_locals,
            inferred_signals: self.inferred_signals,
            param_bounds: self.param_bounds.clone(),
            doc: self.doc.clone(),
            origin: self.origin,
            assert_numeric: self.assert_numeric,
            fragment: self.fragment.clone(),
        }
    }
}
