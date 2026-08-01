use super::*;

/// What a collection literal does with a `;splice` among its items.
enum SpliceRule {
    /// The splice spreads into the constructor call: `[1 ;xs 2]` passes the
    /// elements of `xs` as separate arguments.
    Spread,
    /// The literal rejects it. The payload names the construct and the reason,
    /// and completes the sentence "splice is not supported in ".
    Reject(&'static str),
}

/// `{..}` and `@{..}` reject a splice for the same reason and say so in the
/// same words: the reader pairs their items into keys and values, and a spread
/// arrives as a flat run with no pairing.
const STRUCT_SPLICE: SpliceRule =
    SpliceRule::Reject("struct constructors (key-value types require key-value pairs)");

impl<'a> Analyzer<'a> {
    /// Lower a collection literal to a call of the primitive that builds it.
    ///
    /// Every literal form — `[..]`, `@[..]`, `b[..]`, `{..}`, `|..|` and their
    /// mutable twins — is a call to a constructor primitive with the items as
    /// arguments. What differs between them is only which primitive to call
    /// and whether an item may splice: a sequence spreads a splice, while a
    /// struct or a set rejects one, having no positional reading to spread it
    /// into.
    ///
    /// The call's signal is the combination of its items'.
    fn analyze_collection_literal(
        &mut self,
        prim: &str,
        items: &[Syntax],
        splice: SpliceRule,
        span: Span,
    ) -> Result<Hir, String> {
        let mut args = Vec::new();
        let mut signal = Signal::silent();
        for item in items {
            let (inner, spliced) = Self::unwrap_splice(item);
            if spliced {
                if let SpliceRule::Reject(what) = splice {
                    return Err(format!("{}: splice is not supported in {what}", item.span));
                }
            }
            let hir = self.analyze_expr(inner)?;
            signal = signal.combine(hir.signal);
            args.push(CallArg { expr: hir, spliced });
        }
        let binding = self.resolve_primitive(prim);
        let func = Hir::new(HirKind::Var(binding), span.clone(), Signal::silent());
        Ok(Hir::new(
            HirKind::Call {
                func: Box::new(func),
                args,
                is_tail: false,
            },
            span,
            signal,
        ))
    }

    pub(crate) fn analyze_expr(&mut self, syntax: &Syntax) -> Result<Hir, String> {
        // Publish this form's intro scopes for the duration of its analysis:
        // any scope frame its handler pushes snapshots them as the frame's
        // expansion provenance (`Scope::intro_provenance`), which the
        // referential-transparency rule in `lookup()` consults. A
        // template-origin binding form (carrying its expansion's intro
        // scope) thereby opens a frame the same expansion's references may
        // still resolve into; a user-written form opens a strict frame.
        let form_intros: Vec<crate::syntax::ScopeId> = syntax
            .scopes
            .iter()
            .copied()
            .filter(|s| s.is_intro())
            .collect();
        let saved = std::mem::replace(&mut self.current_form_intros, form_intros);
        let result = self.analyze_expr_dispatch(syntax);
        self.current_form_intros = saved;
        result
    }

    fn analyze_expr_dispatch(&mut self, syntax: &Syntax) -> Result<Hir, String> {
        let span = syntax.span.clone();

        match &syntax.kind {
            // Literals
            SyntaxKind::Nil => Ok(Hir::silent(HirKind::Nil, span)),
            SyntaxKind::Bool(b) => Ok(Hir::silent(HirKind::Bool(*b), span)),
            SyntaxKind::Int(n) => Ok(Hir::silent(HirKind::Int(*n), span)),
            SyntaxKind::Float(f) => Ok(Hir::silent(HirKind::Float(*f), span)),
            SyntaxKind::String(s) => Ok(Hir::silent(HirKind::String(s.clone()), span)),
            SyntaxKind::StringMut(_) => {
                // Should never reach HIR — the expander desugars @"..." to (thaw "...")
                unreachable!("StringMut should be desugared by the expander")
            }
            SyntaxKind::Keyword(k) => Ok(Hir::silent(HirKind::Keyword(k.clone()), span)),

            // Variable reference
            SyntaxKind::Symbol(name) => {
                // Qualified symbol: contains ':' but doesn't start with ':'
                // e.g., obj:key -> (get obj :key), a:b:c -> (get (get a :b) :c)
                if !name.starts_with(':') && name.contains(':') {
                    return self.desugar_qualified_symbol(name, &span, syntax.scopes.as_slice());
                }

                match self.lookup(name, syntax.scopes.as_slice()) {
                    Some(binding) => {
                        self.check_initialized(binding, name, &span)?;
                        Ok(Hir::silent(HirKind::Var(binding), span))
                    }
                    None => {
                        // Try with empty scopes — catches primitives with
                        // empty scope sets when the reference has
                        // macro-introduced scopes
                        match self.lookup(name, &[]) {
                            Some(binding) => {
                                self.check_initialized(binding, name, &span)?;
                                Ok(Hir::silent(HirKind::Var(binding), span))
                            }
                            None => {
                                // Undefined variable — accumulate error with suggestions
                                let suggestions = self.suggest_similar(name);
                                let error = span.undefined_var_suggest(name, suggestions);
                                Ok(self.accumulate_error(error, &span))
                            }
                        }
                    }
                }
            }

            // The sequence literals: a splice spreads into the constructor call.
            SyntaxKind::Array(items) => {
                self.analyze_collection_literal("array", items, SpliceRule::Spread, span)
            }
            SyntaxKind::ArrayMut(items) => {
                self.analyze_collection_literal("@array", items, SpliceRule::Spread, span)
            }
            SyntaxKind::Bytes(items) => {
                self.analyze_collection_literal("bytes", items, SpliceRule::Spread, span)
            }
            SyntaxKind::BytesMut(items) => {
                self.analyze_collection_literal("@bytes", items, SpliceRule::Spread, span)
            }

            // The key-value and set literals: a splice has no positional
            // reading to spread into, so it is rejected.
            SyntaxKind::Struct(items) => {
                self.analyze_collection_literal("struct", items, STRUCT_SPLICE, span)
            }
            SyntaxKind::StructMut(items) => {
                self.analyze_collection_literal("@struct", items, STRUCT_SPLICE, span)
            }

            // Quote - convert to a heap-literal template (or immediate) at
            // analysis time, materialized as an ordinary allocation at runtime.
            SyntaxKind::Quote(inner) => self.analyze_quoted_datum(inner, span),

            // Syntax literal — a hygiene-bearing template symbol (always a
            // `Symbol`, produced by quasiquote). Materialize it as an ORDINARY
            // allocation per execution via `QuoteConst`/`ConstTemplate::SyntaxSymbol`
            // (region/model.md, "Constants lower as ordinary allocations"). The
            // non-symbol case cannot arise (quasiquote only wraps symbols), but is
            // handled defensively as an ordinary quoted datum.
            SyntaxKind::SyntaxLiteral(s) => {
                if let SyntaxKind::Symbol(name) = &s.kind {
                    let template = crate::value::ConstTemplate::SyntaxSymbol {
                        name: name.clone(),
                        scopes: s.scopes.iter().map(|sc| sc.0).collect(),
                        span: s.span.clone(),
                        scope_exempt: s.scope_exempt,
                    };
                    Ok(Hir::silent(HirKind::QuoteConst(template), span))
                } else {
                    let template = s.to_const_template();
                    match template.immediate_value(self.symbols) {
                        Some(value) => Ok(Hir::silent(HirKind::Quote(value), span)),
                        None => Ok(Hir::silent(HirKind::QuoteConst(template), span)),
                    }
                }
            }

            // Quasiquote, Unquote, UnquoteSplicing should have been expanded
            SyntaxKind::Quasiquote(_) | SyntaxKind::Unquote(_) | SyntaxKind::UnquoteSplicing(_) => {
                Err(format!(
                    "{}: quasiquote forms should be expanded before analysis",
                    span
                ))
            }

            // Splice outside of call/constructor position is an error
            SyntaxKind::Splice(_) => Err(format!(
                "{}: `;` is the splice operator, not a comment character. Use `#` for comments.",
                span
            )),

            SyntaxKind::Set(items) => self.analyze_collection_literal(
                "set",
                items,
                SpliceRule::Reject("set constructors (unordered collection)"),
                span,
            ),
            SyntaxKind::SetMut(items) => self.analyze_collection_literal(
                "@set",
                items,
                SpliceRule::Reject("mutable set constructors (unordered collection)"),
                span,
            ),

            // List - could be special form or function call
            SyntaxKind::List(items) => {
                if items.is_empty() {
                    return Ok(Hir::silent(HirKind::EmptyList, span));
                }

                // Check for special forms. Uniform forms dispatch through
                // the registry (forms/registry.rs — the single source of
                // truth for special-form names); only forms whose
                // recognition is CONDITIONAL keep guarded arms below, since
                // a name table cannot express their fall-through-to-call
                // semantics.
                if let SyntaxKind::Symbol(name) = &items[0].kind {
                    if let Some(handler) = super::registry::handler_for(name) {
                        return handler(self, items, span);
                    }
                    match name.as_str() {
                        "emit"
                            if (items.len() == 2 || items.len() == 3)
                                && matches!(
                                    items[1].kind,
                                    crate::syntax::SyntaxKind::Keyword(_)
                                        | crate::syntax::SyntaxKind::Set(_)
                                ) =>
                        {
                            return self.analyze_emit(items, span);
                        }

                        // (doc <symbol>) — if the symbol resolves to a closure
                        // (user-defined or stdlib), evaluate it normally so
                        // prim_doc receives the closure value and extracts its
                        // docstring from closure.template.doc.
                        // Otherwise (NativeFn, Parameter, or unresolved symbol
                        // such as a special form like `if`), rewrite to a
                        // string so the VM can look up builtin docs by name
                        // in vm.docs.
                        "doc" if items.len() == 2 => {
                            if let SyntaxKind::Symbol(sym_name) = &items[1].kind {
                                let has_closure_value = self
                                    .lookup(sym_name, &items[1].scopes)
                                    .map(|b| match self.primitive_values.get(&b) {
                                        None => true,                        // user binding — evaluate normally
                                        Some(v) => v.as_closure().is_some(), // stdlib closure — evaluate normally
                                    })
                                    .unwrap_or(false);
                                if !has_closure_value {
                                    let mut rewritten = items.to_vec();
                                    rewritten[1] = Syntax {
                                        kind: SyntaxKind::String(sym_name.clone()),
                                        span: items[1].span.clone(),
                                        scopes: items[1].scopes.clone(),
                                        scope_exempt: items[1].scope_exempt,
                                    };
                                    return self.analyze_call(&rewritten, span);
                                }
                            }
                        }
                        _ => {
                            // %-intrinsic recognition: %name (not bare %).
                            // One lowering per op (docs/intrinsics.md
                            // § Lowering): the storing/removing/copying ops
                            // fall through to analyze_call — the escape-
                            // correct native funnel Call to the registered
                            // NativeFn — while everything else becomes the
                            // opcode Intrinsic node. Both forms carry the
                            // same prove-or-reject operand obligation
                            // (typeinfer/contract.rs).
                            if name.starts_with('%') && name.len() > 1 {
                                use crate::hir::expr::IntrinsicOp;
                                match IntrinsicOp::from_name(name) {
                                    Some(op) if op.routes_native_funnel() => {}
                                    _ => {
                                        return self.analyze_intrinsic(name, &items[1..], span);
                                    }
                                }
                            }
                        }
                    }
                }

                // Regular function call
                self.analyze_call(items, span)
            }
        }
    }
}
