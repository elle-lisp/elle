// audited: 2026-10-06
//! A file's top-level forms as one letrec: classifying each form, and pre-binding its names.
//!
//! docs/bindings.md
//! docs/signals/inference.md
//!
//! `analyze_file_letrec`, in the `letrec` submodule, analyzes the forms in three
//! passes. Pass 1 pre-binds every name, so forms may refer to each other in any
//! order. Pass 2 analyzes the initializers in order. Pass 3 re-analyzes the
//! lambdas until their signals stop changing. A duplicate name shadows
//! sequentially: the redefinition's value sees the previous binding, and later
//! forms see the new one.

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
        /// Passed to `analyze_destructure_pattern` in pass 2, so the
        /// pattern reuses these bindings.
        leaf_bindings: HashMap<String, Binding>,
        /// Leaf bindings that were deferred (duplicate names), each as
        /// (name, scopes, binding). Pass 2 registers them AFTER analyzing
        /// the value.
        deferred_leaves: Vec<(String, Vec<ScopeId>, Binding)>,
    },
}

mod letrec;

impl<'a> Analyzer<'a> {
    /// Analyze an internal `(%file-body form…)` node: the body of a whole-module
    /// test thunk. Classifies its forms and runs the SAME `analyze_file_letrec`
    /// a real file gets. Forward references, mutual recursion and redefinition
    /// therefore behave as in a direct `elle FILE` run, not as defines in a fn
    /// body. Inserted by `pipeline::compile::transforms::whole_module_transform`;
    /// never written by users.
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
            (b, Some((name.to_string(), name_syntax.scopes().to_vec())))
        } else {
            let b = self.bind(name, name_syntax.scopes(), BindingScope::Local);
            (b, None)
        };

        self.arena.get_mut(binding).is_prebound = true;
        if immutable && self.immutable_by_default && !at_mutable {
            self.arena.get_mut(binding).is_immutable = true;
        }

        // A call that precedes the lambda reads the seed, not the
        // unknown-callee fallback.
        self.seed_lambda_facts(binding, value_syntax);

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
}
