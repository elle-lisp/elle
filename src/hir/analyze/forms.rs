//! Core form analysis: analyze_expr and control flow forms

use super::*;
use crate::hir::expr::CallArg;
use crate::syntax::{Syntax, SyntaxKind};

impl<'a> Analyzer<'a> {
    /// Resolve a primitive name to its binding via scope lookup.
    ///
    /// Used by collection literal desugaring (Array, ArrayMut, Struct, StructMut)
    /// and qualified symbol desugaring to find the primitive binding
    /// registered by `bind_primitives`. Falls back to a fresh binding
    /// if the name isn't in scope (e.g., in tests without primitives).
    fn resolve_primitive(&mut self, name: &str) -> Binding {
        self.lookup(name, &[]).unwrap_or_else(|| {
            let sym = self.symbols.intern(name);
            self.arena.alloc(sym, BindingScope::Local)
        })
    }

    pub(crate) fn analyze_expr(&mut self, syntax: &Syntax) -> Result<Hir, String> {
        let span = syntax.span.clone();

        match &syntax.kind {
            // Literals
            SyntaxKind::Nil => Ok(Hir::silent(HirKind::Nil, span)),
            SyntaxKind::Bool(b) => Ok(Hir::silent(HirKind::Bool(*b), span)),
            SyntaxKind::Int(n) => Ok(Hir::silent(HirKind::Int(*n), span)),
            SyntaxKind::Float(f) => Ok(Hir::silent(HirKind::Float(*f), span)),
            SyntaxKind::String(s) => Ok(Hir::silent(HirKind::String(s.clone()), span)),
            SyntaxKind::Keyword(k) => Ok(Hir::silent(HirKind::Keyword(k.clone()), span)),

            // Variable reference
            SyntaxKind::Symbol(name) => {
                // Qualified symbol: contains ':' but doesn't start with ':'
                // e.g., obj:key -> (get obj :key), a:b:c -> (get (get a :b) :c)
                if !name.starts_with(':') && name.contains(':') {
                    return self.desugar_qualified_symbol(name, &span, syntax.scopes.as_slice());
                }

                match self.lookup(name, syntax.scopes.as_slice()) {
                    Some(binding) => Ok(Hir::silent(HirKind::Var(binding), span)),
                    None => {
                        // Try with empty scopes — catches primitives with
                        // empty scope sets when the reference has
                        // macro-introduced scopes
                        match self.lookup(name, &[]) {
                            Some(binding) => Ok(Hir::silent(HirKind::Var(binding), span)),
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

            // Quote - convert to Value at analysis time
            SyntaxKind::Quote(inner) => {
                let value = (**inner).to_value(self.symbols);
                Ok(Hir::silent(HirKind::Quote(value), span))
            }

            // Syntax literal — pre-computed Value from macro argument passing
            SyntaxKind::SyntaxLiteral(value) => Ok(Hir::silent(HirKind::Quote(*value), span)),

            // Quasiquote, Unquote, UnquoteSplicing should have been expanded
            SyntaxKind::Quasiquote(_) | SyntaxKind::Unquote(_) | SyntaxKind::UnquoteSplicing(_) => {
                Err(format!(
                    "{}: quasiquote forms should be expanded before analysis",
                    span
                ))
            }

            // Splice outside of call/constructor position is an error
            SyntaxKind::Splice(_) => Err(format!(
                "{}: splice can only be used in function call arguments and data constructors",
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

                // Check for special forms
                if let SyntaxKind::Symbol(name) = &items[0].kind {
                    match name.as_str() {
                        "if" => return self.analyze_if(items, span),
                        "let" => return self.analyze_let(items, span),
                        "letrec" => return self.analyze_letrec(items, span),
                        "fn" => return self.analyze_lambda(items, span),
                        "begin" => return self.analyze_begin(&items[1..], span),
                        "block" => return self.analyze_block(&items[1..], span),
                        "break" => return self.analyze_break(&items[1..], span),
                        "var" => return self.analyze_define(items, span),
                        "def" => return self.analyze_const(items, span),
                        "assign" => return self.analyze_assign(items, span),
                        "while" => return self.analyze_while(items, span),

                        "and" => return self.analyze_and(&items[1..], span),
                        "or" => return self.analyze_or(&items[1..], span),
                        "quote" => {
                            if items.len() != 2 {
                                return Err(format!("{}: quote requires 1 argument", span));
                            }
                            let value = items[1].to_value(self.symbols);
                            return Ok(Hir::silent(HirKind::Quote(value), span));
                        }
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
                        "match" => return self.analyze_match(items, span),
                        "cond" => return self.analyze_cond(items, span),
                        "eval" => return self.analyze_eval(items, span),
                        "parameterize" => return self.analyze_parameterize(items, span),

                        "silence" => return self.analyze_silence(items, span),
                        "muffle" => return self.analyze_muffle(items, span),
                        "attune!" => return self.analyze_attune_assert(items, span),

                        "silent!" => return self.analyze_silence_assert(items, span),
                        "numeric!" => return self.analyze_numeric_assert(items, span),
                        "immutable!" => return self.analyze_immutable_assert(items, span),

                        "signal" => {
                            if items.len() != 2 {
                                return Err(format!(
                                    "{}: signal requires exactly 1 argument",
                                    span
                                ));
                            }
                            let keyword = match &items[1].kind {
                                SyntaxKind::Keyword(k) => k.clone(),
                                _ => {
                                    return Err(format!(
                                        "{}: signal requires a keyword argument, got {}",
                                        items[1].span,
                                        items[1].kind_label()
                                    ));
                                }
                            };
                            crate::signals::registry::global_registry()
                                .lock()
                                .unwrap()
                                .register(&keyword)
                                .map_err(|e| format!("{}: {}", items[1].span, e))?;
                            return Ok(Hir::silent(HirKind::Keyword(keyword.to_string()), span));
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
                        "splice" => {
                            return Err(format!(
                                "{}: splice can only be used in function call arguments and data constructors",
                                span
                            ));
                        }
                        _ => {}
                    }
                }

                // Regular function call
                self.analyze_call(items, span)
            }
        }
    }

    pub(crate) fn analyze_if(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        if items.len() < 3 || items.len() > 4 {
            return Err(format!("{}: if requires 2 or 3 arguments", span));
        }

        let cond = self.analyze_expr(&items[1])?;
        let then_branch = self.analyze_expr(&items[2])?;
        let else_branch = if items.len() == 4 {
            self.analyze_expr(&items[3])?
        } else {
            Hir::silent(HirKind::Nil, span.clone())
        };

        let signal = cond
            .signal
            .combine(then_branch.signal)
            .combine(else_branch.signal);

        Ok(Hir::new(
            HirKind::If {
                cond: Box::new(cond),
                then_branch: Box::new(then_branch),
                else_branch: Box::new(else_branch),
            },
            span,
            signal,
        ))
    }

    pub(crate) fn analyze_begin(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        if items.is_empty() {
            return Ok(Hir::silent(HirKind::Nil, span));
        }

        // Check if we're inside a function scope
        let in_function = self.scopes.iter().any(|s| s.is_function);

        if in_function {
            // Two-pass analysis for letrec-style semantics:
            // Pass 1: Create bindings for all defines (without analyzing values)
            for item in items {
                for (name, scopes) in Self::is_define_form(item) {
                    // Create local binding slot, marked prebound so that
                    // needs_capture() knows the binding may be captured before
                    // its initializer runs (self-recursion, forward refs).
                    let binding = self.bind(name, scopes, BindingScope::Local);
                    self.arena.get_mut(binding).is_prebound = true;
                }
            }

            // Pass 2: Analyze all expressions (all bindings now visible)
            let mut exprs = Vec::new();
            let mut signal = Signal::silent();

            for item in items {
                let hir = self.analyze_expr(item)?;
                signal = signal.combine(hir.signal);
                exprs.push(hir);
            }

            Ok(Hir::new(HirKind::Begin(exprs), span, signal))
        } else {
            // At top level, sequential semantics are fine
            let mut exprs = Vec::new();
            let mut signal = Signal::silent();

            for item in items {
                let hir = self.analyze_expr(item)?;
                signal = signal.combine(hir.signal);
                exprs.push(hir);
            }

            Ok(Hir::new(HirKind::Begin(exprs), span, signal))
        }
    }

    pub(crate) fn analyze_block(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        // Check if the first item is a keyword (block name)
        let (name, body_items) = if let Some(first) = items.first() {
            if let SyntaxKind::Keyword(kw) = &first.kind {
                (Some(kw.clone()), &items[1..])
            } else {
                (None, items)
            }
        } else {
            (None, items)
        };

        let block_id = BlockId(self.next_block_id);
        self.next_block_id += 1;

        self.block_contexts.push(BlockContext {
            block_id,
            name: name.clone(),
            fn_depth: self.fn_depth,
        });

        self.push_scope(false);
        let result = self.analyze_begin(body_items, span.clone())?;
        self.pop_scope();

        self.block_contexts.pop();

        let signal = result.signal;
        Ok(Hir::new(
            HirKind::Block {
                name,
                block_id,
                body: vec![result],
            },
            span,
            signal,
        ))
    }

    pub(crate) fn analyze_break(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        // Parse arguments:
        //   (break)           → no name, nil value
        //   (break val)       → no name, has value
        //   (break :name)     → named, nil value
        //   (break :name val) → named, has value
        let (name, value_syntax) = match items.len() {
            0 => (None, None),
            1 => {
                if let SyntaxKind::Keyword(kw) = &items[0].kind {
                    (Some(kw.clone()), None)
                } else {
                    (None, Some(&items[0]))
                }
            }
            2 => {
                if let SyntaxKind::Keyword(kw) = &items[0].kind {
                    (Some(kw.clone()), Some(&items[1]))
                } else {
                    return Err(format!(
                        "{}: break takes at most 2 arguments: optional :name and optional value",
                        span
                    ));
                }
            }
            _ => {
                return Err(format!(
                    "{}: break takes at most 2 arguments: optional :name and optional value",
                    span
                ));
            }
        };

        // Find the target block
        let target = if let Some(ref target_name) = name {
            self.block_contexts
                .iter()
                .rev()
                .find(|ctx| ctx.name.as_deref() == Some(target_name))
                .ok_or_else(|| format!("{}: no block named :{} in scope", span, target_name))?
        } else {
            self.block_contexts
                .last()
                .ok_or_else(|| format!("{}: break outside of any block", span))?
        };

        // Check function boundary
        if target.fn_depth != self.fn_depth {
            return Err(format!("{}: break cannot cross function boundary", span));
        }

        let block_id = target.block_id;

        // Analyze value expression (or nil if absent)
        let value = if let Some(val_syn) = value_syntax {
            self.analyze_expr(val_syn)?
        } else {
            Hir::silent(HirKind::Nil, span.clone())
        };

        let signal = value.signal;
        Ok(Hir::new(
            HirKind::Break {
                block_id,
                value: Box::new(value),
            },
            span,
            signal,
        ))
    }

    pub(crate) fn analyze_body(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        if items.len() == 1 {
            self.analyze_expr(&items[0])
        } else {
            self.analyze_begin(items, span)
        }
    }

    pub(crate) fn analyze_while(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        if items.len() < 3 {
            return Err(format!("{}: while requires condition and body", span));
        }

        // Create an implicit named block so `(break :while val)` works
        let block_id = BlockId(self.next_block_id);
        self.next_block_id += 1;

        self.block_contexts.push(BlockContext {
            block_id,
            name: Some("while".to_string()),
            fn_depth: self.fn_depth,
        });

        let cond = self.analyze_expr(&items[1])?;
        let body = if items.len() == 3 {
            self.analyze_expr(&items[2])?
        } else {
            // Multiple body forms: wrap in implicit begin
            let mut exprs = Vec::new();
            let mut signal = Signal::silent();
            for item in &items[2..] {
                let hir = self.analyze_expr(item)?;
                signal = signal.combine(hir.signal);
                exprs.push(hir);
            }
            Hir::new(HirKind::Begin(exprs), span.clone(), signal)
        };

        self.block_contexts.pop();

        let signal = cond.signal.combine(body.signal);

        let while_node = Hir::new(
            HirKind::While {
                cond: Box::new(cond),
                body: Box::new(body),
            },
            span.clone(),
            signal,
        );

        Ok(Hir::new(
            HirKind::Block {
                name: Some("while".to_string()),
                block_id,
                body: vec![while_node],
            },
            span,
            signal,
        ))
    }

    pub(crate) fn analyze_and(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        if items.is_empty() {
            return Ok(Hir::silent(HirKind::Bool(true), span));
        }

        let mut exprs = Vec::new();
        let mut signal = Signal::silent();

        for item in items {
            let hir = self.analyze_expr(item)?;
            signal = signal.combine(hir.signal);
            exprs.push(hir);
        }

        Ok(Hir::new(HirKind::And(exprs), span, signal))
    }

    pub(crate) fn analyze_or(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        if items.is_empty() {
            return Ok(Hir::silent(HirKind::Bool(false), span));
        }

        let mut exprs = Vec::new();
        let mut signal = Signal::silent();

        for item in items {
            let hir = self.analyze_expr(item)?;
            signal = signal.combine(hir.signal);
            exprs.push(hir);
        }

        Ok(Hir::new(HirKind::Or(exprs), span, signal))
    }

    pub(crate) fn analyze_eval(&mut self, items: &[Syntax], span: Span) -> Result<Hir, String> {
        // (eval expr) or (eval expr env)
        if items.len() < 2 || items.len() > 3 {
            return Err(format!(
                "{}: eval: expected 1 or 2 arguments, got {}",
                span,
                items.len() - 1
            ));
        }
        let expr = self.analyze_expr(&items[1])?;
        let env = if items.len() == 3 {
            self.analyze_expr(&items[2])?
        } else {
            Hir::silent(HirKind::Nil, span.clone())
        };
        let signal = Signal::yields().combine(expr.signal).combine(env.signal);
        Ok(Hir::new(
            HirKind::Eval {
                expr: Box::new(expr),
                env: Box::new(env),
            },
            span,
            signal,
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

        let body = self.analyze_body(&items[2..], span.clone())?;
        signal = signal.combine(body.signal);

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

        let mut clauses = Vec::new();
        let mut else_branch = None;
        let mut signal = Signal::silent();

        // Flat pairs: (cond test1 body1 test2 body2 ... [default])
        let args = &items[1..];
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

    /// Desugar a qualified symbol like `a:b:c` to nested `get` calls:
    /// `(get (get a :b) :c)`.
    ///
    /// The first segment is resolved as a variable (local or global).
    /// Each subsequent segment becomes a keyword argument to `get`.
    /// All synthesized HIR nodes carry the original symbol's span.
    ///
    /// The `get` binding always resolves to the global primitive,
    /// matching the pattern used for array/struct literal
    /// desugaring (see SyntaxKind::Array/ArrayMut/Struct/StructMut arms above).
    fn desugar_qualified_symbol(
        &mut self,
        name: &str,
        span: &Span,
        scopes: &[ScopeId],
    ) -> Result<Hir, String> {
        let segments: Vec<&str> = name.split(':').collect();
        // Reader guarantees: no empty segments, no leading colon (checked above),
        // at least 2 segments (contains ':' is true).

        // First segment: resolve as variable
        let first = segments[0];
        let mut result = match self.lookup(first, scopes) {
            Some(binding) => Hir::silent(HirKind::Var(binding), span.clone()),
            None => match self.lookup(first, &[]) {
                Some(binding) => Hir::silent(HirKind::Var(binding), span.clone()),
                None => {
                    let suggestions = self.suggest_similar(first);
                    let error = span.undefined_var_suggest(first, suggestions);
                    return Ok(self.accumulate_error(error, span));
                }
            },
        };

        // Each subsequent segment: wrap in (get result :segment)
        // Constructs Call nodes directly (not via analyze_call) because
        // get is a pure primitive with known arity Range(2,3).
        let get_binding = self.resolve_primitive("get");
        for segment in &segments[1..] {
            let get_func = Hir::silent(HirKind::Var(get_binding), span.clone());
            let key = Hir::silent(HirKind::Keyword(segment.to_string()), span.clone());
            // Use projected signal if the binding has a projection for this field.
            let call_signal = if let HirKind::Var(binding) = &result.kind {
                if let Some(proj) = self.projection_env.get(binding) {
                    proj.get(*segment).copied().unwrap_or(result.signal)
                } else {
                    result.signal
                }
            } else {
                result.signal
            };
            result = Hir::new(
                HirKind::Call {
                    func: Box::new(get_func),
                    args: vec![
                        CallArg {
                            expr: result,
                            spliced: false,
                        },
                        CallArg {
                            expr: key,
                            spliced: false,
                        },
                    ],
                    is_tail: false,
                },
                span.clone(),
                call_signal,
            );
        }

        Ok(result)
    }
}
