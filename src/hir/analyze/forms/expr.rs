use super::*;

impl<'a> Analyzer<'a> {
    pub(crate) fn analyze_expr(&mut self, syntax: &Syntax) -> Result<Hir, String> {
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

            // Immutable array literal [...] - call array primitive
            SyntaxKind::Array(items) => {
                let mut args = Vec::new();
                let mut signal = Signal::silent();
                for item in items {
                    let (inner, spliced) = Self::unwrap_splice(item);
                    let hir = self.analyze_expr(inner)?;
                    signal = signal.combine(hir.signal);
                    args.push(CallArg { expr: hir, spliced });
                }
                let binding = self.resolve_primitive("array");
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

            // Mutable array literal @[...] - call @array primitive
            SyntaxKind::ArrayMut(items) => {
                let mut args = Vec::new();
                let mut signal = Signal::silent();
                for item in items {
                    let (inner, spliced) = Self::unwrap_splice(item);
                    let hir = self.analyze_expr(inner)?;
                    signal = signal.combine(hir.signal);
                    args.push(CallArg { expr: hir, spliced });
                }
                let binding = self.resolve_primitive("@array");
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

            // Immutable bytes literal b[...] - call bytes primitive
            SyntaxKind::Bytes(items) => {
                let mut args = Vec::new();
                let mut signal = Signal::silent();
                for item in items {
                    let (inner, spliced) = Self::unwrap_splice(item);
                    let hir = self.analyze_expr(inner)?;
                    signal = signal.combine(hir.signal);
                    args.push(CallArg { expr: hir, spliced });
                }
                let binding = self.resolve_primitive("bytes");
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

            // Mutable bytes literal @b[...] - call @bytes primitive
            SyntaxKind::BytesMut(items) => {
                let mut args = Vec::new();
                let mut signal = Signal::silent();
                for item in items {
                    let (inner, spliced) = Self::unwrap_splice(item);
                    let hir = self.analyze_expr(inner)?;
                    signal = signal.combine(hir.signal);
                    args.push(CallArg { expr: hir, spliced });
                }
                let binding = self.resolve_primitive("@bytes");
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

            // Struct literal {...} - call struct primitive
            SyntaxKind::Struct(items) => {
                let mut args = Vec::new();
                let mut signal = Signal::silent();
                for item in items {
                    if matches!(&item.kind, SyntaxKind::Splice(_))
                        || (matches!(&item.kind, SyntaxKind::List(elems) if elems.first().is_some_and(|e| e.as_symbol() == Some("splice"))))
                    {
                        return Err(format!(
                            "{}: splice is not supported in struct constructors (key-value types require key-value pairs)",
                            item.span
                        ));
                    }
                    let hir = self.analyze_expr(item)?;
                    signal = signal.combine(hir.signal);
                    args.push(CallArg {
                        expr: hir,
                        spliced: false,
                    });
                }
                let binding = self.resolve_primitive("struct");
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

            // Mutable struct literal @{...} - call @struct primitive
            SyntaxKind::StructMut(items) => {
                let mut args = Vec::new();
                let mut signal = Signal::silent();
                for item in items {
                    if matches!(&item.kind, SyntaxKind::Splice(_))
                        || (matches!(&item.kind, SyntaxKind::List(elems) if elems.first().is_some_and(|e| e.as_symbol() == Some("splice"))))
                    {
                        return Err(format!(
                            "{}: splice is not supported in struct constructors (key-value types require key-value pairs)",
                            item.span
                        ));
                    }
                    let hir = self.analyze_expr(item)?;
                    signal = signal.combine(hir.signal);
                    args.push(CallArg {
                        expr: hir,
                        spliced: false,
                    });
                }
                let binding = self.resolve_primitive("@struct");
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

            // Quote - convert to a heap-literal template (or immediate) at
            // analysis time, materialized as an ordinary allocation at runtime.
            SyntaxKind::Quote(inner) => self.analyze_quoted_datum(inner, span),

            // Syntax literal — a hygiene-bearing template symbol (always a
            // `Symbol`, produced by quasiquote). Materialize it as an ORDINARY
            // allocation per execution via `QuoteConst`/`ConstTemplate::SyntaxSymbol`
            // (region-model.md, "Constants lower as ordinary allocations"). The
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

            // Set literal |...| - call set constructor primitive
            SyntaxKind::Set(items) => {
                let mut args = Vec::new();
                let mut signal = Signal::silent();
                for item in items {
                    if matches!(&item.kind, SyntaxKind::Splice(_))
                        || (matches!(&item.kind, SyntaxKind::List(elems) if elems.first().is_some_and(|e| e.as_symbol() == Some("splice"))))
                    {
                        return Err(format!(
                            "{}: splice is not supported in set constructors (unordered collection)",
                            item.span
                        ));
                    }
                    let hir = self.analyze_expr(item)?;
                    signal = signal.combine(hir.signal);
                    args.push(CallArg {
                        expr: hir,
                        spliced: false,
                    });
                }
                let binding = self.resolve_primitive("set");
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

            // Mutable set literal @|...| - call mutable-set constructor primitive
            SyntaxKind::SetMut(items) => {
                let mut args = Vec::new();
                let mut signal = Signal::silent();
                for item in items {
                    if matches!(&item.kind, SyntaxKind::Splice(_))
                        || (matches!(&item.kind, SyntaxKind::List(elems) if elems.first().is_some_and(|e| e.as_symbol() == Some("splice"))))
                    {
                        return Err(format!(
                            "{}: splice is not supported in mutable set constructors (unordered collection)",
                            item.span
                        ));
                    }
                    let hir = self.analyze_expr(item)?;
                    signal = signal.combine(hir.signal);
                    args.push(CallArg {
                        expr: hir,
                        spliced: false,
                    });
                }
                let binding = self.resolve_primitive("@set");
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
                            // %-intrinsic recognition: %name (not bare %)
                            // When --checked-intrinsics is active, skip inline
                            // recognition — fall through to analyze_call so the
                            // call compiles as Call to the registered NativeFn.
                            if name.starts_with('%')
                                && name.len() > 1
                                && !crate::config::checked_intrinsics()
                            {
                                return self.analyze_intrinsic(name, &items[1..], span);
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
