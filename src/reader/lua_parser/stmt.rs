use super::*;

impl LuaParser {
    pub(super) fn parse_statement(&mut self) -> Result<Syntax, String> {
        let loc = self.peek_loc().loc.clone();
        match self.peek().clone() {
            LuaToken::If => self.parse_if(),
            LuaToken::While => self.parse_while(),
            LuaToken::For => self.parse_for(),
            LuaToken::Do => self.parse_do(),
            LuaToken::Repeat => self.parse_repeat(),
            LuaToken::Break => {
                self.advance();
                let span = self.span_from(&loc);
                Ok(self.list(vec![self.sym("break", &loc)], span))
            }
            LuaToken::Function => {
                // Named function as statement (or method definition)
                self.advance();
                if let LuaToken::Ident(_) = self.peek() {
                    let name = self.expect_ident()?;
                    if *self.peek() == LuaToken::Colon {
                        self.advance();
                        let method = self.expect_ident()?;
                        let func = self.parse_method_body(&loc)?;
                        let span = func.span.clone();
                        let kw = Syntax::new(SyntaxKind::Keyword(method), self.span_from(&loc));
                        return Ok(self.list(
                            vec![self.sym("put", &loc), self.sym(&name, &loc), kw, func],
                            span,
                        ));
                    }
                    let func = self.parse_function_body(&loc)?;
                    let span = func.span.clone();
                    // assignment: (assign name func)
                    Ok(self.list(
                        vec![self.sym("assign", &loc), self.sym(&name, &loc), func],
                        span,
                    ))
                } else {
                    // anonymous function expression
                    self.parse_function_body(&loc)
                }
            }
            _ => {
                // Expression or assignment
                let expr = self.parse_expr()?;
                if *self.peek() == LuaToken::Comma {
                    // Multiple assignment: a, b = x, y
                    // → (begin (def [__t0 __t1] [x y]) (assign a __t0) (assign b __t1))
                    let mut lhs = vec![expr];
                    while *self.peek() == LuaToken::Comma {
                        self.advance();
                        lhs.push(self.parse_expr()?);
                    }
                    self.expect(&LuaToken::Assign)?;
                    let mut rhs = vec![self.parse_expr()?];
                    while *self.peek() == LuaToken::Comma {
                        self.advance();
                        rhs.push(self.parse_expr()?);
                    }
                    let span = self.span_from(&loc);
                    let mut temps = Vec::new();
                    let mut temp_names = Vec::new();
                    for i in 0..lhs.len() {
                        let tname = format!("{TEMP_PREFIX}{i}");
                        temp_names.push(tname.clone());
                        temps.push(self.sym(&tname, &loc));
                    }
                    let temp_pat = Syntax::new(SyntaxKind::Array(temps), span.clone());
                    let rhs_arr = Syntax::new(SyntaxKind::Array(rhs), span.clone());
                    let bind =
                        self.list(vec![self.sym("def", &loc), temp_pat, rhs_arr], span.clone());
                    let mut stmts = vec![self.sym("begin", &loc), bind];
                    for (i, lval) in lhs.into_iter().enumerate() {
                        let assign = self.list(
                            vec![
                                self.sym("assign", &loc),
                                lval,
                                self.sym(&temp_names[i], &loc),
                            ],
                            span.clone(),
                        );
                        stmts.push(assign);
                    }
                    Ok(self.list(stmts, span))
                } else if *self.peek() == LuaToken::Assign {
                    self.advance();
                    let rhs = self.parse_expr()?;
                    let span = expr.span.merge(&rhs.span);
                    // Field/index assignment: t.foo = v → (put t :foo v)
                    //                         t[k] = v → (put t k v)
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
                    // Plain variable assignment: x = v → (assign x v)
                    Ok(self.list(vec![self.sym("assign", &loc), expr, rhs], span))
                } else {
                    Ok(expr)
                }
            }
        }
    }

    /// Parse `local name = expr` or `local a, b, c = expr` (destructuring).
    /// Returns a `(var name expr)` or `(var [a b c] expr)` form.
    pub(super) fn parse_local_binding(
        &mut self,
        loc: &crate::reader::token::SourceLoc,
    ) -> Result<Syntax, String> {
        let first_name = self.expect_ident()?;

        if *self.peek() == LuaToken::Comma {
            // Multiple names: local a, b, c = expr → (var [a b c] expr)
            let mut names = vec![self.sym(&first_name, loc)];
            while *self.peek() == LuaToken::Comma {
                self.advance();
                let name = self.expect_ident()?;
                names.push(self.sym(&name, loc));
            }
            let value = if *self.peek() == LuaToken::Assign {
                self.advance();
                self.parse_expr()?
            } else {
                self.nil_syntax(loc)
            };
            let span = value.span.clone();
            let pattern = Syntax::new(SyntaxKind::Array(names), self.span_from(loc));
            Ok(self.list(vec![self.sym("var", loc), pattern, value], span))
        } else {
            // Single name: local x = expr → (var x expr)
            let value = if *self.peek() == LuaToken::Assign {
                self.advance();
                self.parse_expr()?
            } else {
                self.nil_syntax(loc)
            };
            let span = value.span.clone();
            Ok(self.list(
                vec![self.sym("var", loc), self.sym(&first_name, loc), value],
                span,
            ))
        }
    }

    pub(super) fn parse_if(&mut self) -> Result<Syntax, String> {
        let loc = self.peek_loc().loc.clone();
        // Consume `if` or `elseif`
        self.advance();
        let cond = self.parse_expr()?;
        self.expect(&LuaToken::Then)?;
        let then_body = self.parse_block()?;

        let else_body = match self.peek().clone() {
            LuaToken::ElseIf => {
                // Recurse — `elseif` becomes nested `if`
                self.parse_if()?
            }
            LuaToken::Else => {
                self.advance();
                let body = self.parse_block()?;
                self.expect(&LuaToken::End)?;
                body
            }
            LuaToken::End => {
                self.advance();
                self.nil_syntax(&loc)
            }
            _ => {
                return Err(format!(
                    "{}: expected 'end', 'else', or 'elseif' in if-statement",
                    loc.position()
                ));
            }
        };

        let span = self.span_from(&loc);
        Ok(self.list(vec![self.sym("if", &loc), cond, then_body, else_body], span))
    }

    pub(super) fn parse_while(&mut self) -> Result<Syntax, String> {
        let loc = self.peek_loc().loc.clone();
        self.expect(&LuaToken::While)?;
        let cond = self.parse_expr()?;
        self.expect(&LuaToken::Do)?;
        let body = self.parse_block()?;
        self.expect(&LuaToken::End)?;

        let span = self.span_from(&loc);
        Ok(self.list(vec![self.sym("while", &loc), cond, body], span))
    }

    pub(super) fn parse_for(&mut self) -> Result<Syntax, String> {
        let loc = self.peek_loc().loc.clone();
        self.expect(&LuaToken::For)?;
        let first_name = self.expect_ident()?;

        // Dispatch: `for x = ...` (numeric) vs `for x in ...` / `for k, v in ...` (generic)
        if *self.peek() == LuaToken::Comma || *self.peek() == LuaToken::In {
            return self.parse_for_in(first_name, &loc);
        }

        // Numeric for: for i = start, stop[, step] do ... end
        self.expect(&LuaToken::Assign)?;
        let var_name = first_name;

        let start = self.parse_expr()?;
        self.expect(&LuaToken::Comma)?;
        let stop = self.parse_expr()?;

        let step = if *self.peek() == LuaToken::Comma {
            self.advance();
            self.parse_expr()?
        } else {
            Syntax::new(SyntaxKind::Int(1), self.span_from(&loc))
        };

        self.expect(&LuaToken::Do)?;
        let body = self.parse_block()?;
        self.expect(&LuaToken::End)?;

        // Desugar:
        // (let ((i__end stop))
        //   (var i start)
        //   (while (<= i i__end)
        //     (begin body (assign i (+ i step)))))
        let end_var = format!("{}__end", var_name);
        let span = self.span_from(&loc);

        let end_binding = self.list(vec![self.sym(&end_var, &loc), stop], span.clone());
        let bindings = self.list(vec![end_binding], span.clone());

        let var_decl = self.list(
            vec![self.sym("var", &loc), self.sym(&var_name, &loc), start],
            span.clone(),
        );

        let cond = self.list(
            vec![
                self.sym("<=", &loc),
                self.sym(&var_name, &loc),
                self.sym(&end_var, &loc),
            ],
            span.clone(),
        );

        let incr = self.list(
            vec![
                self.sym("assign", &loc),
                self.sym(&var_name, &loc),
                self.list(
                    vec![self.sym("+", &loc), self.sym(&var_name, &loc), step],
                    span.clone(),
                ),
            ],
            span.clone(),
        );

        let while_body = self.list(vec![self.sym("begin", &loc), body, incr], span.clone());

        let while_form = self.list(
            vec![self.sym("while", &loc), cond, while_body],
            span.clone(),
        );

        let let_form = self.list(
            vec![self.sym("let", &loc), bindings, var_decl, while_form],
            span,
        );

        Ok(let_form)
    }

    /// Parse `for x in iter do ... end` or `for k, v in iter do ... end`
    /// → `(each x in iter body)` or `(each (k v) in iter body)`
    pub(super) fn parse_for_in(
        &mut self,
        first_name: String,
        loc: &crate::reader::token::SourceLoc,
    ) -> Result<Syntax, String> {
        let mut names = vec![first_name];
        while *self.peek() == LuaToken::Comma {
            self.advance();
            names.push(self.expect_ident()?);
        }
        self.expect(&LuaToken::In)?;
        let iter = self.parse_expr()?;
        self.expect(&LuaToken::Do)?;
        let body = self.parse_block()?;
        self.expect(&LuaToken::End)?;

        let span = self.span_from(loc);
        let binding = if names.len() == 1 {
            self.sym(&names[0], loc)
        } else {
            let name_syms: Vec<Syntax> = names.iter().map(|n| self.sym(n, loc)).collect();
            self.list(name_syms, span.clone())
        };

        Ok(self.list(
            vec![
                self.sym("each", loc),
                binding,
                self.sym("in", loc),
                iter,
                body,
            ],
            span,
        ))
    }

    /// `repeat body until cond` → `(forever (begin body (if cond (break) nil)))`
    pub(super) fn parse_repeat(&mut self) -> Result<Syntax, String> {
        let loc = self.peek_loc().loc.clone();
        self.advance(); // consume `repeat`
        let body = self.parse_block()?;
        self.expect(&LuaToken::Until)?;
        let cond = self.parse_expr()?;

        let span = self.span_from(&loc);
        let break_call = self.list(vec![self.sym("break", &loc)], span.clone());
        let check = self.list(
            vec![
                self.sym("if", &loc),
                cond,
                break_call,
                self.nil_syntax(&loc),
            ],
            span.clone(),
        );
        let loop_body = self.list(vec![self.sym("begin", &loc), body, check], span.clone());
        Ok(self.list(vec![self.sym("forever", &loc), loop_body], span))
    }

    pub(super) fn parse_do(&mut self) -> Result<Syntax, String> {
        let loc = self.peek_loc().loc.clone();
        self.expect(&LuaToken::Do)?;
        let body = self.parse_block()?;
        self.expect(&LuaToken::End)?;
        let span = self.span_from(&loc);
        Ok(self.list(vec![self.sym("begin", &loc), body], span))
    }

    /// Like `parse_function_body` but prepends implicit `self` parameter.
    pub(super) fn parse_method_body(
        &mut self,
        loc: &crate::reader::token::SourceLoc,
    ) -> Result<Syntax, String> {
        self.expect(&LuaToken::LParen)?;
        let mut params = vec![self.sym("self", loc)];
        if *self.peek() != LuaToken::RParen {
            if *self.peek() == LuaToken::DotDotDot {
                self.advance();
                params.push(self.sym(REST_PARAM, loc));
                params.push(self.sym(VARARGS, loc));
            } else {
                let name = self.expect_ident()?;
                params.push(self.sym(&name, loc));
                while *self.peek() == LuaToken::Comma {
                    self.advance();
                    if *self.peek() == LuaToken::DotDotDot {
                        self.advance();
                        params.push(self.sym(REST_PARAM, loc));
                        params.push(self.sym(VARARGS, loc));
                        break;
                    }
                    let name = self.expect_ident()?;
                    params.push(self.sym(&name, loc));
                }
            }
        }
        self.expect(&LuaToken::RParen)?;
        let body = self.parse_block()?;
        self.expect(&LuaToken::End)?;

        let span = self.span_from(loc);
        let param_list = self.list(params, span.clone());
        Ok(self.list(vec![self.sym("fn", loc), param_list, body], span))
    }

    pub(super) fn parse_function_body(
        &mut self,
        loc: &crate::reader::token::SourceLoc,
    ) -> Result<Syntax, String> {
        self.expect(&LuaToken::LParen)?;
        let mut params = Vec::new();
        if *self.peek() != LuaToken::RParen {
            if *self.peek() == LuaToken::DotDotDot {
                // function(...) — varargs only
                self.advance();
                params.push(self.sym(REST_PARAM, loc));
                params.push(self.sym(VARARGS, loc));
            } else {
                let name = self.expect_ident()?;
                params.push(self.sym(&name, loc));
                while *self.peek() == LuaToken::Comma {
                    self.advance();
                    if *self.peek() == LuaToken::DotDotDot {
                        // function(a, b, ...) — named params + varargs
                        self.advance();
                        params.push(self.sym(REST_PARAM, loc));
                        params.push(self.sym(VARARGS, loc));
                        break;
                    }
                    let name = self.expect_ident()?;
                    params.push(self.sym(&name, loc));
                }
            }
        }
        self.expect(&LuaToken::RParen)?;
        let body = self.parse_block()?;
        self.expect(&LuaToken::End)?;

        let span = self.span_from(loc);
        let param_list = self.list(params, span.clone());
        Ok(self.list(vec![self.sym("fn", loc), param_list, body], span))
    }

    // ── Expression parsing (Pratt) ────────────────────────────────────
}
