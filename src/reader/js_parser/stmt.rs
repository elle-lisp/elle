use super::*;

impl JsParser {
    pub(super) fn parse_statement(&mut self) -> Result<Syntax, String> {
        let loc = self.peek_loc().loc.clone();
        match self.peek().clone() {
            JsToken::If => self.parse_if(),
            JsToken::While => self.parse_while(),
            JsToken::For => self.parse_for(),
            JsToken::Do => self.parse_do_while(),
            JsToken::Break => {
                self.advance();
                self.eat_semicolon();
                let span = self.span_from(&loc);
                Ok(self.list(vec![self.sym("break", &loc)], span))
            }
            JsToken::Continue => {
                self.advance();
                self.eat_semicolon();
                let span = self.span_from(&loc);
                Ok(self.list(vec![self.sym("continue", &loc)], span))
            }
            JsToken::Throw => {
                self.advance();
                let val = self.parse_expr()?;
                self.eat_semicolon();
                let span = self.span_from(&loc);
                Ok(self.list(vec![self.sym("error", &loc), val], span))
            }
            JsToken::Try => self.parse_try(),
            JsToken::Function => {
                self.advance();
                if let JsToken::Ident(_) = self.peek() {
                    let name = self.expect_ident()?;
                    let func = self.parse_function_body(&loc)?;
                    let span = func.span.clone();
                    Ok(self.list(
                        vec![self.sym("def", &loc), self.sym(&name, &loc), func],
                        span,
                    ))
                } else {
                    self.parse_function_body(&loc)
                }
            }
            _ => {
                let expr = self.parse_expr()?;

                // Check for assignment operators
                match self.peek().clone() {
                    JsToken::Assign => {
                        self.advance();
                        let rhs = self.parse_expr()?;
                        self.eat_semicolon();
                        let span = expr.span.merge(&rhs.span);
                        // Field/index assignment
                        if let SyntaxKind::List(ref items) = expr.kind {
                            if items.len() == 3 && items[0].is_symbol("get") {
                                return Ok(self.list(
                                    vec![
                                        self.sym("put", &loc),
                                        items[1].clone(),
                                        items[2].clone(),
                                        rhs,
                                    ],
                                    span,
                                ));
                            }
                        }
                        Ok(self.list(vec![self.sym("assign", &loc), expr, rhs], span))
                    }
                    JsToken::PlusAssign => {
                        self.advance();
                        let rhs = self.parse_expr()?;
                        self.eat_semicolon();
                        let span = expr.span.merge(&rhs.span);
                        let add =
                            self.list(vec![self.sym("+", &loc), expr.clone(), rhs], span.clone());
                        Ok(self.list(vec![self.sym("assign", &loc), expr, add], span))
                    }
                    JsToken::MinusAssign => {
                        self.advance();
                        let rhs = self.parse_expr()?;
                        self.eat_semicolon();
                        let span = expr.span.merge(&rhs.span);
                        let sub =
                            self.list(vec![self.sym("-", &loc), expr.clone(), rhs], span.clone());
                        Ok(self.list(vec![self.sym("assign", &loc), expr, sub], span))
                    }
                    JsToken::StarAssign => {
                        self.advance();
                        let rhs = self.parse_expr()?;
                        self.eat_semicolon();
                        let span = expr.span.merge(&rhs.span);
                        let mul =
                            self.list(vec![self.sym("*", &loc), expr.clone(), rhs], span.clone());
                        Ok(self.list(vec![self.sym("assign", &loc), expr, mul], span))
                    }
                    JsToken::SlashAssign => {
                        self.advance();
                        let rhs = self.parse_expr()?;
                        self.eat_semicolon();
                        let span = expr.span.merge(&rhs.span);
                        let div =
                            self.list(vec![self.sym("/", &loc), expr.clone(), rhs], span.clone());
                        Ok(self.list(vec![self.sym("assign", &loc), expr, div], span))
                    }
                    _ => {
                        self.eat_semicolon();
                        Ok(expr)
                    }
                }
            }
        }
    }

    /// Parse `const name = expr` or `let name = expr`.
    /// Also handles destructuring: `const [a, b] = expr`, `const {x, y} = expr`.
    pub(super) fn parse_binding(
        &mut self,
        bind_kind: &str,
        loc: &crate::reader::token::SourceLoc,
    ) -> Result<Syntax, String> {
        // Check for destructuring
        match self.peek().clone() {
            JsToken::LBracket => {
                // Array destructuring: const [a, b] = expr
                self.advance();
                let mut names = Vec::new();
                while *self.peek() != JsToken::RBracket {
                    let name = self.expect_ident()?;
                    names.push(self.sym(&name, loc));
                    if *self.peek() == JsToken::Comma {
                        self.advance();
                    }
                }
                self.expect(&JsToken::RBracket)?;
                self.expect(&JsToken::Assign)?;
                let value = self.parse_expr()?;
                let span = value.span.clone();
                let pattern = Syntax::new(SyntaxKind::Array(names), self.span_from(loc));
                Ok(self.list(vec![self.sym(bind_kind, loc), pattern, value], span))
            }
            JsToken::LBrace => {
                // Object destructuring: const {x, y} = expr
                self.advance();
                let mut names = Vec::new();
                while *self.peek() != JsToken::RBrace {
                    let name = self.expect_ident()?;
                    names.push(self.sym(&name, loc));
                    if *self.peek() == JsToken::Comma {
                        self.advance();
                    }
                }
                self.expect(&JsToken::RBrace)?;
                self.expect(&JsToken::Assign)?;
                let value = self.parse_expr()?;
                let span = value.span.clone();
                let pattern = Syntax::new(SyntaxKind::Array(names), self.span_from(loc));
                Ok(self.list(vec![self.sym(bind_kind, loc), pattern, value], span))
            }
            _ => {
                let name = self.expect_ident()?;
                let value = if *self.peek() == JsToken::Assign {
                    self.advance();
                    self.parse_expr()?
                } else {
                    self.nil_syntax(loc)
                };
                let span = value.span.clone();
                Ok(self.list(
                    vec![self.sym(bind_kind, loc), self.sym(&name, loc), value],
                    span,
                ))
            }
        }
    }

    /// `if (cond) { body } else if (cond) { body } else { body }`
    pub(super) fn parse_if(&mut self) -> Result<Syntax, String> {
        let loc = self.peek_loc().loc.clone();
        self.advance(); // consume `if`
        self.expect(&JsToken::LParen)?;
        let cond = self.parse_expr()?;
        self.expect(&JsToken::RParen)?;
        let then_body = self.parse_brace_block()?;

        let else_body = if *self.peek() == JsToken::Else {
            self.advance();
            if *self.peek() == JsToken::If {
                // else if → nested if
                self.parse_if()?
            } else {
                self.parse_brace_block()?
            }
        } else {
            self.nil_syntax(&loc)
        };

        let span = self.span_from(&loc);
        Ok(self.list(vec![self.sym("if", &loc), cond, then_body, else_body], span))
    }

    /// `while (cond) { body }`
    pub(super) fn parse_while(&mut self) -> Result<Syntax, String> {
        let loc = self.peek_loc().loc.clone();
        self.advance(); // consume `while`
        self.expect(&JsToken::LParen)?;
        let cond = self.parse_expr()?;
        self.expect(&JsToken::RParen)?;
        let body = self.parse_brace_block()?;

        let span = self.span_from(&loc);
        Ok(self.list(vec![self.sym("while", &loc), cond, body], span))
    }

    /// `for (const x of iter) { body }` → `(each x in iter body)`
    /// `for (let i = 0; i < n; i++) { body }` → desugar to while
    pub(super) fn parse_for(&mut self) -> Result<Syntax, String> {
        let loc = self.peek_loc().loc.clone();
        self.advance(); // consume `for`
        self.expect(&JsToken::LParen)?;

        // Check if this is for...of or C-style for
        match self.peek().clone() {
            JsToken::Const | JsToken::Let | JsToken::Var => {
                let saved_pos = self.cursor.pos();
                self.advance(); // skip const/let/var
                let name = self.expect_ident()?;

                if *self.peek() == JsToken::Of {
                    // for (const x of iter)
                    self.advance(); // skip `of`
                    let iter = self.parse_expr()?;
                    self.expect(&JsToken::RParen)?;
                    let body = self.parse_brace_block()?;

                    let span = self.span_from(&loc);
                    return Ok(self.list(
                        vec![
                            self.sym("each", &loc),
                            self.sym(&name, &loc),
                            self.sym("in", &loc),
                            iter,
                            body,
                        ],
                        span,
                    ));
                }

                if *self.peek() == JsToken::In {
                    // for (const x in obj) — iterate keys
                    self.advance(); // skip `in`
                    let obj = self.parse_expr()?;
                    self.expect(&JsToken::RParen)?;
                    let body = self.parse_brace_block()?;

                    let span = self.span_from(&loc);
                    let keys_call = self.list(vec![self.sym("keys", &loc), obj], span.clone());
                    return Ok(self.list(
                        vec![
                            self.sym("each", &loc),
                            self.sym(&name, &loc),
                            self.sym("in", &loc),
                            keys_call,
                            body,
                        ],
                        span,
                    ));
                }

                // C-style for: for (let i = 0; i < n; i++)
                // We already consumed `let i`, backtrack by restoring position
                self.cursor.seek(saved_pos);
                self.parse_c_style_for(&loc)
            }
            _ => self.parse_c_style_for(&loc),
        }
    }

    /// Parse C-style `for (init; cond; update) { body }`
    /// Desugar to: (block init (while cond (begin body update)))
    pub(super) fn parse_c_style_for(
        &mut self,
        loc: &crate::reader::token::SourceLoc,
    ) -> Result<Syntax, String> {
        // Parse init
        let init = match self.peek().clone() {
            JsToken::Const => {
                self.advance();
                self.parse_binding("def", loc)?
            }
            JsToken::Let | JsToken::Var => {
                self.advance();
                self.parse_binding("var", loc)?
            }
            JsToken::Semicolon => self.nil_syntax(loc),
            _ => self.parse_expr()?,
        };
        self.expect(&JsToken::Semicolon)?;

        // Parse condition
        let cond = if *self.peek() == JsToken::Semicolon {
            Syntax::new(SyntaxKind::Bool(true), self.span_from(loc))
        } else {
            self.parse_expr()?
        };
        self.expect(&JsToken::Semicolon)?;

        // Parse update
        let update = if *self.peek() == JsToken::RParen {
            self.nil_syntax(loc)
        } else {
            self.parse_update_expr()?
        };
        self.expect(&JsToken::RParen)?;

        let body = self.parse_brace_block()?;
        let span = self.span_from(loc);

        let while_body = self.list(vec![self.sym("begin", loc), body, update], span.clone());
        let while_form = self.list(vec![self.sym("while", loc), cond, while_body], span.clone());
        Ok(self.list(vec![self.sym("block", loc), init, while_form], span))
    }

    /// Parse an update expression like `i++`, `i--`, `i += 1`.
    pub(super) fn parse_update_expr(&mut self) -> Result<Syntax, String> {
        let loc = self.peek_loc().loc.clone();
        let expr = self.parse_expr()?;

        match self.peek().clone() {
            JsToken::PlusPlus => {
                self.advance();
                let span = self.span_from(&loc);
                let add = self.list(
                    vec![
                        self.sym("+", &loc),
                        expr.clone(),
                        Syntax::new(SyntaxKind::Int(1), span.clone()),
                    ],
                    span.clone(),
                );
                Ok(self.list(vec![self.sym("assign", &loc), expr, add], span))
            }
            JsToken::MinusMinus => {
                self.advance();
                let span = self.span_from(&loc);
                let sub = self.list(
                    vec![
                        self.sym("-", &loc),
                        expr.clone(),
                        Syntax::new(SyntaxKind::Int(1), span.clone()),
                    ],
                    span.clone(),
                );
                Ok(self.list(vec![self.sym("assign", &loc), expr, sub], span))
            }
            JsToken::PlusAssign => {
                self.advance();
                let rhs = self.parse_expr()?;
                let span = self.span_from(&loc);
                let add = self.list(vec![self.sym("+", &loc), expr.clone(), rhs], span.clone());
                Ok(self.list(vec![self.sym("assign", &loc), expr, add], span))
            }
            JsToken::MinusAssign => {
                self.advance();
                let rhs = self.parse_expr()?;
                let span = self.span_from(&loc);
                let sub = self.list(vec![self.sym("-", &loc), expr.clone(), rhs], span.clone());
                Ok(self.list(vec![self.sym("assign", &loc), expr, sub], span))
            }
            _ => Ok(expr),
        }
    }

    /// `do { body } while (cond);`
    /// → `(forever (begin body (if (not cond) (break) nil)))`
    pub(super) fn parse_do_while(&mut self) -> Result<Syntax, String> {
        let loc = self.peek_loc().loc.clone();
        self.advance(); // consume `do`
        let body = self.parse_brace_block()?;
        self.expect(&JsToken::While)?;
        self.expect(&JsToken::LParen)?;
        let cond = self.parse_expr()?;
        self.expect(&JsToken::RParen)?;
        self.eat_semicolon();

        let span = self.span_from(&loc);
        let not_cond = self.list(vec![self.sym("not", &loc), cond], span.clone());
        let break_call = self.list(vec![self.sym("break", &loc)], span.clone());
        let check = self.list(
            vec![
                self.sym("if", &loc),
                not_cond,
                break_call,
                self.nil_syntax(&loc),
            ],
            span.clone(),
        );
        let loop_body = self.list(vec![self.sym("begin", &loc), body, check], span.clone());
        Ok(self.list(vec![self.sym("forever", &loc), loop_body], span))
    }

    /// `try { body } catch (e) { handler }`
    /// → `(let (([__ok __val] (protect ((fn () body)))))
    ///      (if __ok __val ((fn (e) handler) __val)))`
    pub(super) fn parse_try(&mut self) -> Result<Syntax, String> {
        let loc = self.peek_loc().loc.clone();
        self.advance(); // consume `try`
        let try_body = self.parse_brace_block()?;

        // Parse catch clause
        self.expect(&JsToken::Catch)?;
        self.expect(&JsToken::LParen)?;
        let err_name = self.expect_ident()?;
        self.expect(&JsToken::RParen)?;
        let catch_body = self.parse_brace_block()?;

        // Optional finally (we just inline it after)
        let finally_body = if *self.peek() == JsToken::Finally {
            self.advance();
            Some(self.parse_brace_block()?)
        } else {
            None
        };

        let span = self.span_from(&loc);

        // Build: (protect ((fn () try_body)))
        let try_fn = self.list(
            vec![
                self.sym("fn", &loc),
                self.list(vec![], span.clone()),
                try_body,
            ],
            span.clone(),
        );
        let protect_call = self.list(
            vec![
                self.sym("protect", &loc),
                self.list(vec![try_fn], span.clone()),
            ],
            span.clone(),
        );

        // Build result pattern [__ok __val]
        let ok_sym = self.sym(TRY_OK, &loc);
        let val_sym = self.sym(TRY_VAL, &loc);
        let pattern = Syntax::new(SyntaxKind::Array(vec![ok_sym, val_sym]), span.clone());

        // Build catch handler
        let catch_fn = self.list(
            vec![
                self.sym("fn", &loc),
                self.list(vec![self.sym(&err_name, &loc)], span.clone()),
                catch_body,
            ],
            span.clone(),
        );
        let catch_call = self.list(vec![catch_fn, self.sym(TRY_VAL, &loc)], span.clone());

        // Build if expression
        let if_expr = self.list(
            vec![
                self.sym("if", &loc),
                self.sym(TRY_OK, &loc),
                self.sym(TRY_VAL, &loc),
                catch_call,
            ],
            span.clone(),
        );

        // Build let binding
        let binding = self.list(vec![pattern, protect_call], span.clone());
        let bindings = self.list(vec![binding], span.clone());

        let mut let_items = vec![self.sym("let", &loc), bindings, if_expr];
        if let Some(fin) = finally_body {
            // Wrap in begin to add finally
            let inner = self.list(let_items, span.clone());
            let_items = vec![self.sym("begin", &loc), inner, fin];
        }

        Ok(self.list(let_items, span))
    }

    pub(super) fn parse_function_body(
        &mut self,
        loc: &crate::reader::token::SourceLoc,
    ) -> Result<Syntax, String> {
        self.expect(&JsToken::LParen)?;
        let params = self.parse_params()?;
        self.expect(&JsToken::RParen)?;
        let body = self.parse_brace_block()?;

        let span = self.span_from(loc);
        let param_list = self.list(params, span.clone());
        Ok(self.list(vec![self.sym("fn", loc), param_list, body], span))
    }

    pub(super) fn parse_params(&mut self) -> Result<Vec<Syntax>, String> {
        let loc = self.peek_loc().loc.clone();
        let mut params = Vec::new();
        if *self.peek() == JsToken::RParen {
            return Ok(params);
        }

        // Check for rest parameter
        if *self.peek() == JsToken::DotDotDot {
            self.advance();
            let name = self.expect_ident()?;
            params.push(self.sym(REST_PARAM, &loc));
            params.push(self.sym(&name, &loc));
            return Ok(params);
        }

        let name = self.expect_ident()?;
        params.push(self.sym(&name, &loc));

        while *self.peek() == JsToken::Comma {
            self.advance();
            if *self.peek() == JsToken::DotDotDot {
                self.advance();
                let name = self.expect_ident()?;
                params.push(self.sym(REST_PARAM, &loc));
                params.push(self.sym(&name, &loc));
                break;
            }
            let name = self.expect_ident()?;
            params.push(self.sym(&name, &loc));
        }

        Ok(params)
    }

    // ── Expression parsing (Pratt) ────────────────────────────────────
}
