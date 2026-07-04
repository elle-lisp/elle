//! File-scope letrec compilation for top-level forms.

use super::*;
use crate::syntax::{ScopeId, Syntax, SyntaxKind};

/// Classify a macro-expanded top-level form for `analyze_file_letrec`:
/// `(def name value)` / `(var name value)` are bindings, `(signal :kw)` is a
/// signal declaration, and everything else is a bare expression (gensym-named).
/// Used by the file front-end and by the `%file-body` thunk-body form so both
/// see identical file-scope semantics.
pub fn classify_form(syntax: &Syntax) -> FileForm<'_> {
    if let SyntaxKind::List(items) = &syntax.kind {
        if items.len() == 2 {
            if let Some(head) = items[0].as_symbol() {
                if head == "signal" {
                    return FileForm::Signal(&items[1]);
                }
            }
        }
        if items.len() == 3 {
            if let Some(head) = items[0].as_symbol() {
                match head {
                    "def" => return FileForm::Def(&items[1], &items[2]),
                    "var" => return FileForm::Var(&items[1], &items[2]),
                    _ => {}
                }
            }
        }
    }
    FileForm::Expr(syntax)
}

/// Intermediate classification for file-scope letrec Pass 1.
/// Each form is pre-bound as either a simple name or a destructure pattern.
enum PreBound<'s> {
    Simple {
        binding: Binding,
        value_syntax: &'s Syntax,
        /// Name and scopes for deferred bindings (duplicate names).
        /// When set, Pass 2 registers this binding in the scope
        /// AFTER analyzing the value, so the RHS sees the previous
        /// binding rather than the new uninitialized one.
        deferred: Option<(String, Vec<ScopeId>)>,
    },
    Destructure {
        pattern_syntax: &'s Syntax,
        value_syntax: &'s Syntax,
        immutable: bool,
        /// Pre-created bindings from pass 1, keyed by name.
        /// Passed to `analyze_destructure_pattern` in pass 2 to
        /// ensure binding identity matches.
        leaf_bindings: HashMap<String, Binding>,
        /// Leaf bindings that were deferred (duplicate names).
        /// Maps name → (scopes, binding) for registration in Pass 2
        /// AFTER analyzing the value.
        deferred_leaves: Vec<(String, Vec<ScopeId>, Binding)>,
    },
}

mod letrec;

impl<'a> Analyzer<'a> {
    /// Analyze a list of top-level forms as a synthetic letrec.
    ///
    /// Each form is classified as `Def` (immutable), `Var` (mutable), or
    /// `Expr` (gensym-named dummy binding). Three-pass analysis:
    /// - Pass 1: pre-bind all names (enables mutual recursion)
    /// - Pass 2: analyze initializers sequentially
    /// - Pass 3: fixpoint loop for signal propagation through mutual recursion
    ///
    /// Duplicate names use sequential shadowing: the RHS of a redefinition
    /// sees the previous binding, and subsequent forms see the new one.
    ///
    /// Returns a single `HirKind::Letrec` node. The body is a reference
    /// to the last binding (the file's return value).
    /// Analyze an internal `(%file-body form…)` node: the body of a whole-module
    /// test thunk. Classifies its forms and runs the SAME `analyze_file_letrec`
    /// a real file gets, so def/var forward references, mutual recursion, and def
    /// REDEFINITION (the redefinition's RHS sees the previous binding) behave
    /// identically to a direct `elle FILE` run rather than fn-body
    /// internal-define semantics. Inserted by
    /// `pipeline::compile::whole_module_transform`; never written by users.
    pub(crate) fn analyze_file_body(
        &mut self,
        items: &[Syntax],
        span: Span,
    ) -> Result<Hir, String> {
        let forms: Vec<FileForm> = items.iter().map(classify_form).collect();
        self.analyze_file_letrec(forms, span)
    }

    /// Pass 1 helper: pre-bind a simple (non-destructuring) name for file-scope letrec.
    ///
    /// Creates the binding and seeds signal/arity tracking for lambda forms.
    /// Duplicate names are deferred to Pass 2 for sequential shadowing.
    fn prebind_simple<'s>(
        &mut self,
        raw_name: &str,
        name_syntax: &'s Syntax,
        value_syntax: &'s Syntax,
        immutable: bool,
        seen_names: &mut HashSet<String>,
    ) -> PreBound<'s> {
        let (name, at_mutable) = super::strip_at_prefix(raw_name);
        let is_duplicate = !seen_names.insert(name.to_string());
        let (binding, deferred) = if is_duplicate {
            // Duplicate name: create binding but don't register in scope yet.
            // Pass 2 will register it via register_binding AFTER analyzing
            // the RHS, so the RHS sees the previous binding.
            let sym = self.symbols.intern(name);
            let b = self.arena.alloc(sym, BindingScope::Local);
            (b, Some((name.to_string(), name_syntax.scopes.clone())))
        } else {
            let b = self.bind(name, name_syntax.scopes.as_slice(), BindingScope::Local);
            (b, None)
        };

        self.arena.get_mut(binding).is_prebound = true;
        if immutable && self.immutable_by_default && !at_mutable {
            self.arena.get_mut(binding).is_immutable = true;
        }

        // Seed signal_env and arity_env for lambda forms so self-recursive
        // calls don't default to Yields during analysis.
        if Self::is_lambda_syntax(value_syntax) {
            self.signal_env.insert(binding, Signal::silent());
            if let Some(list) = value_syntax.as_list() {
                if let Some(params_syn) = list.get(1).and_then(|s| s.as_list_or_tuple()) {
                    self.arity_env
                        .insert(binding, Self::arity_from_syntax_params(params_syn));
                }
            }
        }

        PreBound::Simple {
            binding,
            value_syntax,
            deferred,
        }
    }

    /// Pass 1 helper: pre-bind destructure leaf names for file-scope letrec.
    ///
    /// Extracts leaf names from the pattern and creates bindings for each.
    /// Duplicate names are deferred to Pass 2 for sequential shadowing.
    fn prebind_destructure<'s>(
        &mut self,
        pattern_syntax: &'s Syntax,
        value_syntax: &'s Syntax,
        immutable: bool,
        seen_names: &mut HashSet<String>,
    ) -> PreBound<'s> {
        let mut names = Vec::new();
        Self::extract_pattern_names(pattern_syntax, &mut names);
        let mut leaf_bindings = HashMap::new();
        let mut deferred_leaves = Vec::new();

        for (name, name_scopes) in &names {
            if *name == "_" {
                continue;
            }
            let is_dup = !seen_names.insert(name.to_string());
            let b = if is_dup {
                // Duplicate: create binding without scope registration.
                // register_binding in Pass 2 handles slot allocation.
                let sym = self.symbols.intern(name);
                let b = self.arena.alloc(sym, BindingScope::Local);
                deferred_leaves.push((name.to_string(), name_scopes.to_vec(), b));
                b
            } else {
                self.bind(name, name_scopes, BindingScope::Local)
            };
            self.arena.get_mut(b).is_prebound = true;
            if immutable && self.immutable_by_default {
                self.arena.get_mut(b).is_immutable = true;
            }
            leaf_bindings.insert(name.to_string(), b);
        }

        PreBound::Destructure {
            pattern_syntax,
            value_syntax,
            immutable,
            leaf_bindings,
            deferred_leaves,
        }
    }

    /// Check if a syntax node is a lambda form: `(fn ...)`.
    fn is_lambda_syntax(syntax: &Syntax) -> bool {
        if let Some(list) = syntax.as_list() {
            list.first()
                .and_then(|s| s.as_symbol())
                .is_some_and(|s| s == "fn")
        } else {
            false
        }
    }

    /// Compute a signal projection for a file's return expression.
    ///
    /// A signal projection maps keyword field names to the signals of the
    /// closures they hold. This enables cross-file signal inference: when
    /// an importing file accesses `module:field`, the analyzer uses the
    /// projected signal instead of the conservative `Polymorphic` fallback.
    ///
    /// The return expression is the last binding's init value. We unwrap
    /// through Lambda bodies and Begin blocks to find the struct literal.
    pub(crate) fn compute_signal_projection(
        &self,
        hir: &crate::hir::expr::Hir,
    ) -> Option<HashMap<String, Signal>> {
        self.extract_struct_projection(hir)
    }

    /// Extract field→signal mapping from an expression, unwrapping through
    /// Lambda, Begin, If, and Let/Letrec bodies.
    fn extract_struct_projection(
        &self,
        hir: &crate::hir::expr::Hir,
    ) -> Option<HashMap<String, Signal>> {
        use crate::hir::expr::HirKind;
        match &hir.kind {
            // Struct literal: (struct :key1 val1 :key2 val2 ...)
            HirKind::Call { func, args, .. } => {
                if let HirKind::Var(binding) = &func.kind {
                    let name = self.symbols.name(self.arena.get(*binding).name)?;
                    if name != "struct" {
                        return None;
                    }
                    // Parse alternating keyword-value pairs
                    let mut projection = HashMap::new();
                    let mut i = 0;
                    while i + 1 < args.len() {
                        if let HirKind::Keyword(key) = &args[i].expr.kind {
                            let val = &args[i + 1].expr;
                            let sig = self.hir_signal(val);
                            projection.insert(key.clone(), sig);
                        }
                        i += 2;
                    }
                    if projection.is_empty() {
                        None
                    } else {
                        Some(projection)
                    }
                } else {
                    None
                }
            }
            // Lambda: unwrap body
            HirKind::Lambda { body, .. } => self.extract_struct_projection(body),
            // Begin: unwrap last expression
            HirKind::Begin(exprs) => exprs.last().and_then(|e| self.extract_struct_projection(e)),
            // Let/Letrec: unwrap body
            HirKind::Let { body, .. } | HirKind::Letrec { body, .. } => {
                self.extract_struct_projection(body)
            }
            // If: union of both branches
            HirKind::If {
                then_branch,
                else_branch,
                ..
            } => {
                let a = self.extract_struct_projection(then_branch);
                let b = self.extract_struct_projection(else_branch);
                match (a, b) {
                    (Some(mut a), Some(b)) => {
                        for (k, sig) in b {
                            let entry = a.entry(k).or_insert(Signal::silent());
                            *entry = entry.combine(sig);
                        }
                        Some(a)
                    }
                    (Some(a), None) | (None, Some(a)) => Some(a),
                    (None, None) => None,
                }
            }
            _ => None,
        }
    }

    /// Get the signal of an HIR value expression — either its inferred
    /// signal (for lambdas) or its binding's signal from signal_env.
    fn hir_signal(&self, hir: &crate::hir::expr::Hir) -> Signal {
        use crate::hir::expr::HirKind;
        match &hir.kind {
            HirKind::Lambda {
                inferred_signals, ..
            } => *inferred_signals,
            HirKind::Var(binding) => self
                .signal_env
                .get(binding)
                .copied()
                .unwrap_or(Signal::yields()),
            _ => Signal::yields(),
        }
    }
}
