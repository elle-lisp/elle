// audited: 2026-11-24
//! The `letrec` binding form: pre-bind every name, then analyze the initializers in order and the body last.
//!
//! docs/bindings.md
//! docs/pipeline.md

use super::*;
use crate::hir::pattern::HirPattern;
use crate::syntax::{Syntax, SyntaxKind};

/// What a `letrec`'s bindings analyze to, before its body.
pub(super) struct LetrecGroup {
    /// Each binding with its initializer, in order. A pattern's leaves are
    /// here initialized to nil, after the temporary that holds its value.
    pub(super) bindings: Vec<(Binding, Hir)>,
    /// Each pattern, with the temporary its value is in, for the destructure
    /// that runs once every initializer has.
    pub(super) destructures: Vec<(HirPattern, Binding)>,
    /// The initializers' combined signal.
    pub(super) signal: Signal,
}

impl<'a> Analyzer<'a> {
    pub(crate) fn analyze_letrec(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        let bindings_syntax = Self::letrec_bindings(items, span)?;
        self.push_scope(false);
        let group = self.bind_letrec(bindings_syntax, span)?;
        let body = self.analyze_body(&items[2..], span)?;
        self.pop_scope();

        let mut signal = group.signal.combine(body.signal);

        // If there are destructures, wrap the body with Destructure nodes
        let final_body = if group.destructures.is_empty() {
            body
        } else {
            let mut exprs: Vec<Hir> = Vec::with_capacity(group.destructures.len() + 1);
            for (pattern, tmp) in group.destructures {
                let destr =
                    self.destructure(pattern, Hir::silent(HirKind::Var(tmp), span), true, span);
                signal = signal.combine(destr.signal);
                exprs.push(destr);
            }
            exprs.push(body);
            Hir::new(HirKind::Begin(exprs), span, signal)
        };

        Ok(Hir::new(
            HirKind::Letrec {
                bindings: group.bindings,
                body: Box::new(final_body),
            },
            span,
            signal,
        ))
    }

    /// The bindings list of the `letrec` form `items`, which also holds a body.
    pub(super) fn letrec_bindings(items: &[Syntax], span: Span) -> Result<&[Syntax], String> {
        if items.len() < 3 {
            return Err(format!("{}: letrec requires bindings and body", span));
        }
        let bindings = items[1].as_list_or_tuple().ok_or_else(|| {
            if matches!(items[1].kind, SyntaxKind::ArrayMut(_)) {
                format!(
                    "{}: letrec bindings must use (...) or [...], not @[...]",
                    items[1].span
                )
            } else {
                format!(
                    "{}: letrec bindings must be a list (...) or [...], got {}",
                    items[1].span,
                    items[1].kind_label()
                )
            }
        })?;
        if bindings.len() % 2 != 0 {
            return Err(format!(
                "{}: letrec bindings must have an even number of forms (name/value pairs)",
                span
            ));
        }
        Ok(bindings)
    }

    /// Bind a `letrec`'s names in the current scope and analyze its
    /// initializers. Bindings are flat pairs: [name1 value1 name2 value2 ...].
    pub(super) fn bind_letrec(
        &mut self,
        bindings_syntax: &[Syntax],
        span: Span,
    ) -> Result<LetrecGroup, String> {
        // Pass 1: Classify each binding. Pre-bind ALL names for mutual
        // visibility — simple symbols AND destructure leaf names.
        // Destructure leaf names are pre-bound so that other initializers
        // (for example, recursive functions) can reference them.
        //
        // A duplicate definition (the same name and scope set) is a compile
        // error.
        //
        // Pass 2 hands each pattern's leaf bindings to
        // analyze_destructure_pattern through `pre_bindings`, so the pattern
        // reuses them rather than binding the names a second time.
        enum LetrecEntry<'s> {
            Simple(Binding, &'s Syntax),
            Destructure {
                pattern: &'s Syntax,
                value: &'s Syntax,
                leaf_bindings: HashMap<String, Binding>,
            },
        }
        let mut entries = Vec::new();
        let mut duplicates = super::scopes::DuplicateGuard::default();

        let mut i = 0;
        while i < bindings_syntax.len() {
            let name_syn = &bindings_syntax[i];
            let value_syn = &bindings_syntax[i + 1];

            if let Some(raw_name) = name_syn.as_symbol() {
                // Simple binding — bind immediately for mutual recursion.
                // Marked prebound: may be captured before initialization.
                // Bound with the name's hygiene scopes (like a `def`), so a
                // macro-template binder and a user binder of the same
                // spelling are distinct identities.
                let (name, is_mutable) = super::strip_at_prefix(raw_name);
                let sym = self.symbols.intern(name);
                duplicates.check(sym, name, name_syn.scopes.as_slice(), &name_syn.span)?;
                let b = self.bind_written(name, name_syn, BindingScope::Local);
                let fn_depth = self.fn_depth;
                let inner = self.arena.get_mut(b);
                inner.is_prebound = true;
                inner.init_pending = true;
                inner.prebind_fn_depth = fn_depth;
                if self.immutable_by_default && !is_mutable {
                    self.arena.get_mut(b).is_immutable = true;
                }
                entries.push(LetrecEntry::Simple(b, value_syn));
            } else if Self::is_destructure_pattern(name_syn) {
                // Destructure pattern — pre-bind leaf names for mutual visibility
                let mut names = Vec::new();
                Self::extract_pattern_names(name_syn, &mut names);
                let mut leaf_bindings = HashMap::new();
                for (name, name_scopes) in &names {
                    if *name != "_" {
                        let sym = self.symbols.intern(name);
                        duplicates.check(sym, name, name_scopes, &name_syn.span)?;
                        let b = self.bind(name, name_scopes, BindingScope::Local);
                        let fn_depth = self.fn_depth;
                        let inner = self.arena.get_mut(b);
                        inner.is_prebound = true;
                        inner.init_pending = true;
                        inner.prebind_fn_depth = fn_depth;
                        // Immutability set later by analyze_destructure_pattern
                        leaf_bindings.insert(name.to_string(), b);
                    }
                }
                entries.push(LetrecEntry::Destructure {
                    pattern: name_syn,
                    value: value_syn,
                    leaf_bindings,
                });
            } else {
                return Err(format!(
                    "{}: letrec binding name must be a symbol or destructure pattern",
                    span
                ));
            }
            i += 2;
        }

        // Second pass: analyze values and build the output.
        // Simple bindings go into the Letrec node's bindings vec.
        // Destructured bindings: the temp binding AND all leaf bindings
        // go into the Letrec bindings vec (leaf bindings initialized to
        // nil). So the lowerer allocates slots for all bindings
        // before lowering any lambda values — lambdas may capture
        // destructured leaf bindings. Destructure nodes in the body then
        // update the leaf binding slots.
        //
        // Seed signal_env for all pre-bound simple bindings with Silent.
        // Without this, a forward-referenced letrec sibling reads as
        // Signal::unknown() (the fallback in get_raw_callee_signal), and its
        // calls lower to spurious SuspendingCall instructions.
        for entry in &entries {
            if let LetrecEntry::Simple(binding, value_syntax) = entry {
                self.signal_env.insert(*binding, Signal::silent());
                self.seed_lambda_facts(*binding, value_syntax);
            }
        }

        let mut bindings = Vec::new();
        let mut destructures = Vec::new();
        let mut signal = Signal::silent();

        for entry in &entries {
            match entry {
                LetrecEntry::Simple(binding, value_syntax) => {
                    // Set the self-recursion context to this binding so a self-edge
                    // inside its lambda classifies `CaptureKind::Recursive`.
                    let value = self.analyze_initializer(*binding, value_syntax)?;
                    // Initializer analyzed: later initializers and the body
                    // may now read this binding's value.
                    self.arena.get_mut(*binding).init_pending = false;
                    // Track signal and arity for interprocedural analysis,
                    // and the value a call on an immediate reads
                    self.record_lambda_facts(*binding, &value);
                    self.record_constant(*binding, &value);
                    self.apply_transient_binding_state(*binding);
                    bindings.push((*binding, value));
                }
                LetrecEntry::Destructure {
                    pattern: pattern_syntax,
                    value: value_syntax,
                    leaf_bindings,
                } => {
                    let value = self.analyze_expr(value_syntax)?;
                    // Create a temp binding for the value in the Letrec bindings
                    let tmp = self.bind("__destructure_tmp", &[], BindingScope::Local);
                    bindings.push((tmp, value));
                    // Analyze the pattern using pre-created bindings from pass 1
                    // Immutable by default; individual leaves with @ opt into mutability
                    self.pre_bindings.clone_from(leaf_bindings);
                    let pattern = self.analyze_destructure_pattern(
                        pattern_syntax,
                        BindingScope::Local,
                        self.immutable_by_default,
                        &span,
                    )?;
                    self.pre_bindings.clear();
                    // Initializer analyzed: the destructured leaves are
                    // initialized once the pattern runs.
                    for leaf in leaf_bindings.values() {
                        self.arena.get_mut(*leaf).init_pending = false;
                    }
                    // Add leaf bindings to the Letrec bindings vec (initialized
                    // to nil) so the lowerer allocates slots for them before
                    // lowering any lambda values that might capture them.
                    for leaf_binding in &pattern.bindings().bindings {
                        bindings.push((*leaf_binding, Hir::silent(HirKind::Nil, span)));
                    }
                    destructures.push((pattern, tmp));
                }
            }
        }

        for (_, value) in &bindings {
            signal = signal.combine(value.signal);
        }
        Ok(LetrecGroup {
            bindings,
            destructures,
            signal,
        })
    }
}
