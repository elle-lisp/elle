use super::*;

impl PyParser {
    pub(super) fn parse_statement(&mut self) -> Result<Syntax, String> {
        let loc = self.peek_loc().loc.clone();
        match self.peek().clone() {
            PyToken::Def => {
                self.advance();
                let name = self.expect_ident()?;
                let func = self.parse_function_def(&loc)?;
                let span = func.span;
                Ok(self.list(
                    vec![self.sym("def", &loc), self.sym(&name, &loc), func],
                    span,
                ))
            }
            PyToken::If => self.parse_if(),
            PyToken::While => self.parse_while(),
            PyToken::For => self.parse_for(),
            PyToken::Return => {
                self.advance();
                let val = if matches!(
                    self.peek(),
                    PyToken::Newline | PyToken::Eof | PyToken::Dedent
                ) {
                    self.nil_syntax(&loc)
                } else {
                    self.parse_expr()?
                };
                self.eat_newlines();
                Ok(val)
            }
            PyToken::Break => {
                self.advance();
                self.eat_newlines();
                let span = self.span_from(&loc);
                Ok(self.list(vec![self.sym("break", &loc)], span))
            }
            PyToken::Continue => {
                self.advance();
                self.eat_newlines();
                let span = self.span_from(&loc);
                Ok(self.list(vec![self.sym("continue", &loc)], span))
            }
            PyToken::Pass => {
                self.advance();
                self.eat_newlines();
                Ok(self.nil_syntax(&loc))
            }
            PyToken::Raise => {
                self.advance();
                let val = self.parse_expr()?;
                self.eat_newlines();
                let span = self.span_from(&loc);
                Ok(self.list(vec![self.sym("error", &loc), val], span))
            }
            PyToken::Try => self.parse_try(),
            PyToken::Assert => {
                self.advance();
                let cond = self.parse_expr()?;
                let msg = if *self.peek() == PyToken::Comma {
                    self.advance();
                    self.parse_expr()?
                } else {
                    self.str_lit("assertion failed", self.span_from(&loc))
                };
                self.eat_newlines();
                let span = self.span_from(&loc);
                // (if (not cond) (error {:error :assertion-failed :message msg}) nil)
                let not_cond = self.list(vec![self.sym("not", &loc), cond], span);
                let err_struct = Syntax::new(
                    SyntaxKind::Struct(self.arena.nodes(&[
                        self.kw("error", span),
                        self.kw("assertion-failed", span),
                        self.kw("message", span),
                        msg,
                    ])),
                    span,
                );
                let error_call = self.list(vec![self.sym("error", &loc), err_struct], span);
                Ok(self.list(
                    vec![
                        self.sym("if", &loc),
                        not_cond,
                        error_call,
                        self.nil_syntax(&loc),
                    ],
                    span,
                ))
            }
            PyToken::Import => {
                self.advance();
                let name = self.expect_ident()?;
                self.eat_newlines();
                let span = self.span_from(&loc);
                let import_path = format!("lib/{}", name);
                let import_str = self.str_lit(&import_path, span);
                let import_call = self.list(vec![self.sym("import", &loc), import_str], span);
                Ok(self.list(
                    vec![self.sym("def", &loc), self.sym(&name, &loc), import_call],
                    span,
                ))
            }
            _ => {
                let expr = self.parse_expr()?;
                // Check for assignment
                match self.peek().clone() {
                    PyToken::Assign => {
                        self.advance();
                        let rhs = self.parse_expr()?;
                        self.eat_newlines();
                        let span = expr.span.merge(&rhs.span);
                        // Field/index assignment
                        if let SyntaxKind::List(ref items) = expr.kind {
                            if items.len() == 3 && items[0].is_symbol("get") {
                                return Ok(self.list(
                                    vec![self.sym("put", &loc), items[1], items[2], rhs],
                                    span,
                                ));
                            }
                        }
                        // At top level or function body level (depth <= 1),
                        // `x = val` creates a new mutable binding with `var`.
                        // Inside loops/ifs (depth > 1), use `assign` so that
                        // the mutation reaches the enclosing function scope
                        // (matching Python's function-scoped variables).
                        if matches!(&expr.kind, SyntaxKind::Symbol(_)) && self.depth == 0 {
                            Ok(self.list(vec![self.sym("var", &loc), expr, rhs], span))
                        } else {
                            Ok(self.list(vec![self.sym("assign", &loc), expr, rhs], span))
                        }
                    }
                    PyToken::PlusAssign => {
                        self.advance();
                        let rhs = self.parse_expr()?;
                        self.eat_newlines();
                        let span = expr.span.merge(&rhs.span);
                        let add = self.list(vec![self.sym("+", &loc), expr, rhs], span);
                        Ok(self.list(vec![self.sym("assign", &loc), expr, add], span))
                    }
                    PyToken::MinusAssign => {
                        self.advance();
                        let rhs = self.parse_expr()?;
                        self.eat_newlines();
                        let span = expr.span.merge(&rhs.span);
                        let sub = self.list(vec![self.sym("-", &loc), expr, rhs], span);
                        Ok(self.list(vec![self.sym("assign", &loc), expr, sub], span))
                    }
                    PyToken::StarAssign => {
                        self.advance();
                        let rhs = self.parse_expr()?;
                        self.eat_newlines();
                        let span = expr.span.merge(&rhs.span);
                        let mul = self.list(vec![self.sym("*", &loc), expr, rhs], span);
                        Ok(self.list(vec![self.sym("assign", &loc), expr, mul], span))
                    }
                    PyToken::SlashAssign => {
                        self.advance();
                        let rhs = self.parse_expr()?;
                        self.eat_newlines();
                        let span = expr.span.merge(&rhs.span);
                        let div = self.list(vec![self.sym("/", &loc), expr, rhs], span);
                        Ok(self.list(vec![self.sym("assign", &loc), expr, div], span))
                    }
                    _ => {
                        self.eat_newlines();
                        Ok(expr)
                    }
                }
            }
        }
    }

    pub(super) fn parse_if(&mut self) -> Result<Syntax, String> {
        let loc = self.peek_loc().loc.clone();
        self.advance(); // consume `if`
        let cond = self.parse_expr()?;
        let then_body = self.parse_block()?;

        let else_body = if *self.peek() == PyToken::Elif {
            self.parse_elif()?
        } else if *self.peek() == PyToken::Else {
            self.advance();
            self.parse_block()?
        } else {
            self.nil_syntax(&loc)
        };

        let span = self.span_from(&loc);
        Ok(self.list(vec![self.sym("if", &loc), cond, then_body, else_body], span))
    }

    pub(super) fn parse_elif(&mut self) -> Result<Syntax, String> {
        let loc = self.peek_loc().loc.clone();
        self.advance(); // consume `elif`
        let cond = self.parse_expr()?;
        let then_body = self.parse_block()?;

        let else_body = if *self.peek() == PyToken::Elif {
            self.parse_elif()?
        } else if *self.peek() == PyToken::Else {
            self.advance();
            self.parse_block()?
        } else {
            self.nil_syntax(&loc)
        };

        let span = self.span_from(&loc);
        Ok(self.list(vec![self.sym("if", &loc), cond, then_body, else_body], span))
    }

    pub(super) fn parse_while(&mut self) -> Result<Syntax, String> {
        let loc = self.peek_loc().loc.clone();
        self.advance(); // consume `while`
        let cond = self.parse_expr()?;
        let body = self.parse_block()?;

        let span = self.span_from(&loc);
        Ok(self.list(vec![self.sym("while", &loc), cond, body], span))
    }

    /// `for x in iter:` → `(each x in iter body)`
    pub(super) fn parse_for(&mut self) -> Result<Syntax, String> {
        let loc = self.peek_loc().loc.clone();
        self.advance(); // consume `for`

        // Parse binding(s)
        let mut names = vec![self.expect_ident()?];
        while *self.peek() == PyToken::Comma {
            self.advance();
            names.push(self.expect_ident()?);
        }

        self.expect(&PyToken::In)?;
        let iter = self.parse_expr()?;
        let body = self.parse_block()?;

        let span = self.span_from(&loc);
        let binding = if names.len() == 1 {
            self.sym(&names[0], &loc)
        } else {
            let name_syms: Vec<Syntax> = names.iter().map(|n| self.sym(n, &loc)).collect();
            self.list(name_syms, span)
        };

        Ok(self.list(
            vec![
                self.sym("each", &loc),
                binding,
                self.sym("in", &loc),
                iter,
                body,
            ],
            span,
        ))
    }

    /// `try: ... except Exception as e: ...`
    pub(super) fn parse_try(&mut self) -> Result<Syntax, String> {
        let loc = self.peek_loc().loc.clone();
        self.advance(); // consume `try`
        let try_body = self.parse_block()?;

        self.expect(&PyToken::Except)?;
        // Optional exception type (ignored)
        if let PyToken::Ident(_) = self.peek() {
            self.advance(); // skip exception class name
        }
        // Optional `as name`
        let err_name = if *self.peek() == PyToken::As {
            self.advance();
            self.expect_ident()?
        } else {
            DEFAULT_EXC.to_string()
        };
        let catch_body = self.parse_block()?;

        // Optional finally
        let finally_body = if *self.peek() == PyToken::Finally {
            self.advance();
            Some(self.parse_block()?)
        } else {
            None
        };

        let span = self.span_from(&loc);

        // Build: (let (([__ok __val] (protect ((fn () try_body)))))
        //          (if __ok __val ((fn (err_name) catch_body) __val)))
        let try_fn = self.list(
            vec![self.sym("fn", &loc), self.list(vec![], span), try_body],
            span,
        );
        let protect_call = self.list(
            vec![self.sym("protect", &loc), self.list(vec![try_fn], span)],
            span,
        );

        let ok_sym = self.sym(TRY_OK, &loc);
        let val_sym = self.sym(TRY_VAL, &loc);
        let pattern = self.arr(vec![ok_sym, val_sym], span);

        let catch_fn = self.list(
            vec![
                self.sym("fn", &loc),
                self.list(vec![self.sym(&err_name, &loc)], span),
                catch_body,
            ],
            span,
        );
        let catch_call = self.list(vec![catch_fn, self.sym(TRY_VAL, &loc)], span);

        let if_expr = self.list(
            vec![
                self.sym("if", &loc),
                self.sym(TRY_OK, &loc),
                self.sym(TRY_VAL, &loc),
                catch_call,
            ],
            span,
        );

        let binding = self.list(vec![pattern, protect_call], span);
        let bindings = self.list(vec![binding], span);

        let mut let_items = vec![self.sym("let", &loc), bindings, if_expr];
        if let Some(fin) = finally_body {
            let inner = self.list(let_items, span);
            let_items = vec![self.sym("begin", &loc), inner, fin];
        }

        Ok(self.list(let_items, span))
    }

    pub(super) fn parse_function_def(
        &mut self,
        loc: &crate::reader::token::SourceLoc,
    ) -> Result<Syntax, String> {
        self.expect(&PyToken::LParen)?;
        let params = self.parse_params()?;
        self.expect(&PyToken::RParen)?;
        // Skip optional return type annotation: -> type
        if *self.peek() == PyToken::Arrow {
            self.advance();
            // Skip the type expression (just an identifier or dotted name)
            self.parse_expr()?;
        }
        let body = self.parse_function_block()?;

        let span = self.span_from(loc);
        let param_list = self.list(params, span);
        Ok(self.list(vec![self.sym("fn", loc), param_list, body], span))
    }

    pub(super) fn parse_params(&mut self) -> Result<Vec<Syntax>, String> {
        let loc = self.peek_loc().loc.clone();
        let mut params = Vec::new();
        if *self.peek() == PyToken::RParen {
            return Ok(params);
        }

        // Skip `self` parameter
        if let PyToken::Ident(ref name) = self.peek().clone() {
            if name == "self" {
                self.advance();
                params.push(self.sym("self", &loc));
                if *self.peek() == PyToken::Comma {
                    self.advance();
                }
                if *self.peek() == PyToken::RParen {
                    return Ok(params);
                }
            }
        }

        // Check for *args
        if *self.peek() == PyToken::Star {
            self.advance();
            let name = self.expect_ident()?;
            params.push(self.sym(REST_PARAM, &loc));
            params.push(self.sym(&name, &loc));
            return Ok(params);
        }

        let name = self.expect_ident()?;
        // Skip type annotation: name: type
        if *self.peek() == PyToken::Colon {
            self.advance();
            self.parse_expr()?; // skip type
        }
        // Skip default value: name=value
        if *self.peek() == PyToken::Assign {
            self.advance();
            self.parse_expr()?; // skip default
        }
        params.push(self.sym(&name, &loc));

        while *self.peek() == PyToken::Comma {
            self.advance();
            if *self.peek() == PyToken::RParen {
                break;
            }
            if *self.peek() == PyToken::Star {
                self.advance();
                let name = self.expect_ident()?;
                params.push(self.sym(REST_PARAM, &loc));
                params.push(self.sym(&name, &loc));
                break;
            }
            if *self.peek() == PyToken::StarStar {
                // **kwargs — skip for now
                self.advance();
                self.expect_ident()?;
                break;
            }
            let name = self.expect_ident()?;
            if *self.peek() == PyToken::Colon {
                self.advance();
                self.parse_expr()?;
            }
            if *self.peek() == PyToken::Assign {
                self.advance();
                self.parse_expr()?;
            }
            params.push(self.sym(&name, &loc));
        }

        Ok(params)
    }

    // ── Expression parsing (Pratt) ────────────────────────────────────
}
