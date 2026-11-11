// audited: 2026-11-09
//! The special forms that desugar in the analyzer: `environment`, `parameterize` and `cond`.
//!
//! docs/impl/hir.md
//! docs/signals/inference.md
//! docs/impl/symbol.md

use super::*;

impl<'a> Analyzer<'a> {
    /// `(environment)` — reify the current lexical scope as a struct.
    ///
    /// Desugars into `(struct 'x x 'y y ...)` for all lexical bindings
    /// in scope. A binding is reified exactly when a reference written at
    /// the `(environment)` site would resolve to it:
    /// - candidates are filtered by scope-subset against the form's own
    ///   scope set, so macro-introduced (scope-stamped) bindings are
    ///   excluded the same way ordinary resolution excludes them;
    /// - compiler temporaries are excluded by their structural
    ///   `is_synthetic` flag (never by name — a user binding spelled
    ///   `__anything` is an ordinary binding);
    /// - primitives are excluded (eval binds those itself).
    pub(crate) fn analyze_environment(
        &mut self,
        items: &[Syntax],
        span: Span,
    ) -> Result<Hir, String> {
        if items.len() != 1 {
            return Err(format!("{}: environment takes no arguments", span));
        }

        // The scope set a user-written reference at this site would carry.
        let ref_scopes: &[crate::syntax::ScopeId] = &items[0].scopes;

        // Collect all lexical (non-primitive) bindings from all scopes.
        // Inner scopes shadow outer: track seen ids.
        let mut seen = std::collections::HashSet::new();
        let mut pairs: Vec<(SymbolId, Binding)> = Vec::new();

        for scope in self.scopes.iter().rev() {
            // The binding map's order is its hasher's, which differs between
            // compiles; the struct's argument order is bytecode, so it follows
            // the symbol ids instead.
            let mut in_scope: Vec<_> = scope.bindings.iter().collect();
            in_scope.sort_unstable_by_key(|(sym, _)| **sym);
            for (&sym, candidates) in in_scope {
                if seen.contains(&sym) {
                    continue;
                }
                // Resolve like `lookup`: the binding's scopes must be a
                // subset of the reference's; the largest scope set wins,
                // ties going to the most recently bound.
                // A body's `def` the analysis has not reached is not yet
                // visible here.
                let winner = candidates
                    .iter()
                    .filter(|c| c.state == super::super::EntryState::Visible)
                    .filter(|c| super::super::is_scope_subset(&c.scopes, ref_scopes))
                    .max_by_key(|c| c.scopes.len());
                if let Some(winner) = winner {
                    let binding = winner.binding;
                    self.defs.uses.note_read(binding);
                    // Skip primitives, compiler temporaries, and a merge that
                    // may stand for a mutable binding.
                    if self.primitive_values.contains_key(&binding)
                        || self.arena.get(binding).is_synthetic
                        || self.defs.uses.poisoned.contains(&binding)
                    {
                        seen.insert(sym);
                        continue;
                    }
                    pairs.push((sym, binding));
                    seen.insert(sym);
                }
            }
        }

        // Build: (struct 'sym1 sym1 'sym2 sym2 ...)
        let struct_binding = self.resolve_primitive("struct");
        let func = Hir::new(HirKind::Var(struct_binding), span, Signal::silent());

        let mut args = Vec::new();
        for &(sym_id, binding) in &pairs {
            // quoted symbol key
            let key = Hir::silent(HirKind::Quote(Value::symbol(sym_id)), span);
            args.push(crate::hir::expr::CallArg {
                expr: key,
                spliced: false,
            });
            // variable reference
            let var = Hir::silent(HirKind::Var(binding), span);
            args.push(crate::hir::expr::CallArg {
                expr: var,
                spliced: false,
            });
        }

        Ok(Hir::new(
            HirKind::Call {
                func: Box::new(func),
                args,
                is_tail: false,
            },
            span,
            Signal::silent(),
        ))
    }
    pub(crate) fn analyze_parameterize(
        &mut self,
        items: &[Syntax],
        span: Span,
    ) -> Result<Hir, String> {
        // (parameterize ((param1 val1) (param2 val2) ...) body ...)
        if items.len() < 3 {
            return Err(format!(
                "{}: parameterize requires bindings and at least one body expression",
                span
            ));
        }

        let bindings_syntax = items[1]
            .as_list_or_tuple()
            .ok_or_else(|| format!("{}: parameterize bindings must be a list", span))?;

        if bindings_syntax.len() > 255 {
            return Err(format!(
                "{}: parameterize supports at most 255 bindings, got {}",
                span,
                bindings_syntax.len()
            ));
        }

        let mut bindings = Vec::new();
        let mut signal = Signal::silent();

        for pair_syntax in bindings_syntax {
            let pair = pair_syntax.as_list_or_tuple().ok_or_else(|| {
                format!(
                    "{}: parameterize binding must be (param value), got {}",
                    pair_syntax.span,
                    pair_syntax.kind_label()
                )
            })?;
            if pair.len() != 2 {
                return Err(format!(
                    "{}: parameterize binding must be (param value), got {} elements",
                    pair_syntax.span,
                    pair.len()
                ));
            }
            let param = self.analyze_expr(&pair[0])?;
            let value = self.analyze_expr(&pair[1])?;
            signal = signal.combine(param.signal).combine(value.signal);
            bindings.push((param, value));
        }

        let body = self.analyze_body(&items[2..], span)?;
        signal = signal.combine(body.signal);

        // The VM checks at run time that each binding names a parameter.
        self.add_inherent_bits(crate::value::SIG_ERROR);
        signal = signal.combine(Signal::errors());

        Ok(Hir::new(
            HirKind::Parameterize {
                bindings,
                body: Box::new(body),
            },
            span,
            signal,
        ))
    }
    pub(crate) fn analyze_cond(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        if items.len() < 2 {
            return Ok(Hir::silent(HirKind::Nil, span));
        }

        // Flat pairs: (cond test1 body1 test2 body2 ... [default])
        let args = &items[1..];
        // A clause may skip a `def` another runs, so a `cond` that holds one
        // is a chain of `if` forms, each a merge frame (`merge.rs`).
        let mut reached = Vec::new();
        for arg in args {
            super::super::owned::reach(arg, false, &mut reached);
        }
        if !reached.is_empty() {
            return self.analyze_cond_chain(args, span);
        }

        let mut clauses = Vec::new();
        let mut else_branch = None;
        let mut signal = Signal::silent();
        let mut i = 0;
        while i < args.len() {
            if i + 1 >= args.len() {
                // Odd trailing element = default branch
                let body = self.analyze_expr(&args[i])?;
                signal = signal.combine(body.signal);
                else_branch = Some(Box::new(body));
                break;
            }

            let test = self.analyze_expr(&args[i])?;
            let body = self.analyze_expr(&args[i + 1])?;
            signal = signal.combine(test.signal).combine(body.signal);
            clauses.push((test, body));
            i += 2;
        }

        Ok(Hir::new(
            HirKind::Cond {
                clauses,
                else_branch,
            },
            span,
            signal,
        ))
    }
    /// The `cond` clauses `args` as nested `if` forms.
    fn analyze_cond_chain(&mut self, args: &[Syntax], span: Span) -> Result<Hir, String> {
        match args {
            [] => Ok(Hir::silent(HirKind::Nil, span)),
            [default] => self.analyze_expr(default),
            [test, body, rest @ ..] => {
                self.open_merge();
                let analyzed =
                    self.analyze_branches(test, body, span, |a| a.analyze_cond_chain(rest, span));
                self.end_merge(analyzed)
            }
        }
    }
}
