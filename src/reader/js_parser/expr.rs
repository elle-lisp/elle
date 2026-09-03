use super::*;

impl JsParser {
    pub(super) fn parse_expr(&mut self) -> Result<Syntax, String> {
        self.parse_pratt(0)
    }

    /// Pratt expression parser.  Precedence levels (low → high):
    ///  0: ternary ?:
    ///  1: ||
    ///  2: &&
    ///  3: |  (bitwise)
    ///  4: ^  (bitwise)
    ///  5: &  (bitwise)
    ///  6: === !== == !=
    ///  7: < > <= >=
    ///  8: << >>
    ///  9: + -
    /// 10: * / %
    /// 11: **  (right-assoc)
    /// 12: unary ! - ~ typeof
    /// 13: atoms, calls, field access
    pub(super) fn parse_pratt(&mut self, min_prec: u8) -> Result<Syntax, String> {
        let mut lhs = self.parse_unary()?;

        loop {
            let (op_name, prec, right_assoc) = match self.peek() {
                JsToken::Or => ("or", 1, false),
                JsToken::And => ("and", 2, false),
                JsToken::Pipe => ("bit/or", 3, false),
                JsToken::Caret => ("bit/xor", 4, false),
                JsToken::Ampersand => ("bit/and", 5, false),
                JsToken::Eq => ("=", 6, false),
                JsToken::Neq => ("neq", 6, false),
                JsToken::EqLoose => ("=", 6, false),
                JsToken::NeqLoose => ("neq", 6, false),
                JsToken::Lt => ("<", 7, false),
                JsToken::Gt => (">", 7, false),
                JsToken::Le => ("<=", 7, false),
                JsToken::Ge => (">=", 7, false),
                JsToken::ShiftLeft => ("bit/shift-left", 8, false),
                JsToken::ShiftRight => ("bit/shift-right", 8, false),
                JsToken::Plus => ("+", 9, false),
                JsToken::Minus => ("-", 9, false),
                JsToken::Star => ("*", 10, false),
                JsToken::Slash => ("/", 10, false),
                JsToken::Percent => ("%", 10, false),
                JsToken::StarStar => ("math/pow", 11, true),
                _ => break,
            };

            if prec < min_prec {
                break;
            }

            let loc = self.peek_loc().loc.clone();
            let is_neq = matches!(self.peek(), JsToken::Neq | JsToken::NeqLoose);
            self.advance();

            let next_prec = if right_assoc { prec } else { prec + 1 };
            let rhs = self.parse_pratt(next_prec)?;

            let span = lhs.span.merge(&rhs.span);
            if is_neq {
                let eq = self.list(vec![self.sym("=", &loc), lhs, rhs], span);
                lhs = self.list(vec![self.sym("not", &loc), eq], span);
            } else {
                lhs = self.list(vec![self.sym(op_name, &loc), lhs, rhs], span);
            }
        }

        // Ternary: expr ? then : else → (if expr then else)
        // Precedence 0 (lowest), right-associative
        if min_prec == 0 && *self.peek() == JsToken::Question {
            let loc = self.peek_loc().loc.clone();
            self.advance();
            let then_expr = self.parse_pratt(0)?;
            self.expect(&JsToken::Colon)?;
            let else_expr = self.parse_pratt(0)?;
            let span = lhs.span.merge(&else_expr.span);
            lhs = self.list(vec![self.sym("if", &loc), lhs, then_expr, else_expr], span);
        }

        Ok(lhs)
    }

    pub(super) fn parse_unary(&mut self) -> Result<Syntax, String> {
        let loc = self.peek_loc().loc.clone();
        match self.peek().clone() {
            JsToken::Not => {
                self.advance();
                let operand = self.parse_pratt(12)?;
                let span = self.span_from(&loc).merge(&operand.span);
                Ok(self.list(vec![self.sym("not", &loc), operand], span))
            }
            JsToken::Minus => {
                self.advance();
                let operand = self.parse_pratt(12)?;
                let span = self.span_from(&loc).merge(&operand.span);
                Ok(self.list(
                    vec![
                        self.sym("-", &loc),
                        Syntax::new(SyntaxKind::Int(0), self.span_from(&loc)),
                        operand,
                    ],
                    span,
                ))
            }
            JsToken::Tilde => {
                self.advance();
                let operand = self.parse_pratt(12)?;
                let span = self.span_from(&loc).merge(&operand.span);
                Ok(self.list(vec![self.sym("bit/not", &loc), operand], span))
            }
            JsToken::Typeof => {
                self.advance();
                let operand = self.parse_pratt(12)?;
                let span = self.span_from(&loc).merge(&operand.span);
                Ok(self.list(vec![self.sym("type-of", &loc), operand], span))
            }
            JsToken::PlusPlus => {
                // Pre-increment: ++x → (assign x (+ x 1))
                self.advance();
                let operand = self.parse_pratt(12)?;
                let span = self.span_from(&loc).merge(&operand.span);
                let add = self.list(
                    vec![
                        self.sym("+", &loc),
                        operand,
                        Syntax::new(SyntaxKind::Int(1), span),
                    ],
                    span,
                );
                Ok(self.list(vec![self.sym("assign", &loc), operand, add], span))
            }
            JsToken::MinusMinus => {
                // Pre-decrement: --x → (assign x (- x 1))
                self.advance();
                let operand = self.parse_pratt(12)?;
                let span = self.span_from(&loc).merge(&operand.span);
                let sub = self.list(
                    vec![
                        self.sym("-", &loc),
                        operand,
                        Syntax::new(SyntaxKind::Int(1), span),
                    ],
                    span,
                );
                Ok(self.list(vec![self.sym("assign", &loc), operand, sub], span))
            }
            _ => self.parse_postfix(),
        }
    }

    pub(super) fn parse_postfix(&mut self) -> Result<Syntax, String> {
        let mut expr = self.parse_atom()?;

        loop {
            match self.peek().clone() {
                // Function call: f(args)
                JsToken::LParen => {
                    expr = self.parse_call(expr)?;
                }
                // Field access: obj.field → (get obj :field)
                JsToken::Dot => {
                    let loc = self.peek_loc().loc.clone();
                    self.advance();
                    let field = self.expect_ident()?;
                    let span = expr.span.merge(&self.span_from(&loc));
                    let kw = Syntax::new(
                        SyntaxKind::Keyword(self.arena.text(&field)),
                        self.span_from(&loc),
                    );
                    expr = self.list(vec![self.sym("get", &loc), expr, kw], span);
                }
                // Index access: obj[key] → (get obj key)
                JsToken::LBracket => {
                    let loc = self.peek_loc().loc.clone();
                    self.advance();
                    let key = self.parse_expr()?;
                    self.expect(&JsToken::RBracket)?;
                    let span = expr.span.merge(&key.span);
                    expr = self.list(vec![self.sym("get", &loc), expr, key], span);
                }
                // Post-increment: x++ → (assign x (+ x 1)), returns old value
                // For simplicity we treat it as pre-increment (same side effect)
                JsToken::PlusPlus => {
                    let loc = self.peek_loc().loc.clone();
                    self.advance();
                    let span = self.span_from(&loc);
                    let add = self.list(
                        vec![
                            self.sym("+", &loc),
                            expr,
                            Syntax::new(SyntaxKind::Int(1), span),
                        ],
                        span,
                    );
                    expr = self.list(vec![self.sym("assign", &loc), expr, add], span);
                }
                JsToken::MinusMinus => {
                    let loc = self.peek_loc().loc.clone();
                    self.advance();
                    let span = self.span_from(&loc);
                    let sub = self.list(
                        vec![
                            self.sym("-", &loc),
                            expr,
                            Syntax::new(SyntaxKind::Int(1), span),
                        ],
                        span,
                    );
                    expr = self.list(vec![self.sym("assign", &loc), expr, sub], span);
                }
                _ => break,
            }
        }

        Ok(expr)
    }

    pub(super) fn parse_call(&mut self, func: Syntax) -> Result<Syntax, String> {
        let args = self.parse_arglist()?;
        let loc = func.span;
        let span = Span::new(loc.start as usize, loc.end as usize, loc.line, loc.col)
            .with_file(&self.file);
        let mut items = vec![func];
        items.extend(args);
        Ok(self.list(items, span))
    }

    pub(super) fn parse_arglist(&mut self) -> Result<Vec<Syntax>, String> {
        self.expect(&JsToken::LParen)?;
        let mut args = Vec::new();
        if *self.peek() != JsToken::RParen {
            // Handle spread: ...expr → (splice expr)
            if *self.peek() == JsToken::DotDotDot {
                let loc = self.peek_loc().loc.clone();
                self.advance();
                let expr = self.parse_expr()?;
                let span = self.span_from(&loc);
                args.push(Syntax::new(SyntaxKind::Splice(self.arena.node(expr)), span));
            } else {
                args.push(self.parse_expr()?);
            }
            while *self.peek() == JsToken::Comma {
                self.advance();
                if *self.peek() == JsToken::DotDotDot {
                    let loc = self.peek_loc().loc.clone();
                    self.advance();
                    let expr = self.parse_expr()?;
                    let span = self.span_from(&loc);
                    args.push(Syntax::new(SyntaxKind::Splice(self.arena.node(expr)), span));
                } else {
                    args.push(self.parse_expr()?);
                }
            }
        }
        self.expect(&JsToken::RParen)?;
        Ok(args)
    }

    pub(super) fn parse_atom(&mut self) -> Result<Syntax, String> {
        let loc = self.peek_loc().loc.clone();
        let len = self.peek_loc().len;
        match self.peek().clone() {
            JsToken::Int(n) => {
                self.advance();
                Ok(Syntax::new(SyntaxKind::Int(n), self.make_span(&loc, len)))
            }
            JsToken::Float(f) => {
                self.advance();
                Ok(Syntax::new(SyntaxKind::Float(f), self.make_span(&loc, len)))
            }
            JsToken::String(s) => {
                self.advance();
                Ok(Syntax::new(
                    SyntaxKind::String(self.arena.text(&s)),
                    self.make_span(&loc, len),
                ))
            }
            JsToken::TemplateNoSub(s) => {
                self.advance();
                Ok(Syntax::new(
                    SyntaxKind::String(self.arena.text(&s)),
                    self.make_span(&loc, len),
                ))
            }
            JsToken::TemplateHead(head) => {
                self.advance();
                self.parse_template_expr(head, &loc)
            }
            JsToken::True => {
                self.advance();
                Ok(Syntax::new(
                    SyntaxKind::Bool(true),
                    self.make_span(&loc, len),
                ))
            }
            JsToken::False => {
                self.advance();
                Ok(Syntax::new(
                    SyntaxKind::Bool(false),
                    self.make_span(&loc, len),
                ))
            }
            JsToken::Null | JsToken::Undefined => {
                self.advance();
                Ok(Syntax::new(SyntaxKind::Nil, self.make_span(&loc, len)))
            }
            JsToken::Ident(name) => {
                self.advance();
                // Check for arrow function: name => expr
                if *self.peek() == JsToken::Arrow {
                    self.advance();
                    return self.parse_arrow_body(&[name], &loc);
                }
                Ok(Syntax::new(
                    SyntaxKind::Symbol(self.arena.text(&name)),
                    self.make_span(&loc, len),
                ))
            }

            // Grouping or arrow function params
            JsToken::LParen => {
                // Try to detect arrow function: (...) => ...
                if self.is_arrow_params() {
                    return self.parse_arrow_function(&loc);
                }
                self.advance();
                let expr = self.parse_expr()?;
                self.expect(&JsToken::RParen)?;
                Ok(expr)
            }

            // Array literal: [1, 2, 3]
            JsToken::LBracket => self.parse_array_literal(),

            // Object literal: {key: val, ...}
            JsToken::LBrace => self.parse_object_literal(),

            // Function expression
            JsToken::Function => {
                self.advance();
                // Optional name (ignored for expressions)
                if let JsToken::Ident(_) = self.peek() {
                    self.advance();
                }
                self.parse_function_body(&loc)
            }

            // Spread in array context: ...expr
            JsToken::DotDotDot => {
                self.advance();
                let expr = self.parse_pratt(12)?;
                let span = self.span_from(&loc);
                Ok(Syntax::new(SyntaxKind::Splice(self.arena.node(expr)), span))
            }

            _ => Err(format!(
                "{}: unexpected token {:?}",
                loc.position(),
                self.peek()
            )),
        }
    }

    /// Check if the current `(` starts arrow function parameters.
    /// Heuristic: scan forward for matching `)` then check for `=>`.
    pub(super) fn is_arrow_params(&self) -> bool {
        if self.cursor.current().map(|t| &t.token) != Some(&JsToken::LParen) {
            return false;
        }
        let mut depth = 1u32;
        // Offset of the lookahead token from the current `(`.
        let mut off = 1usize;
        while depth > 0 {
            match self.cursor.nth(off).map(|t| &t.token) {
                Some(JsToken::LParen) => depth += 1,
                Some(JsToken::RParen) => depth -= 1,
                Some(JsToken::Eof) => return false,
                Some(_) => {}
                None => break,
            }
            off += 1;
        }
        // `off` now points to the token after the matching `)`.
        matches!(self.cursor.nth(off).map(|t| &t.token), Some(JsToken::Arrow))
    }

    pub(super) fn parse_arrow_function(
        &mut self,
        loc: &crate::reader::token::SourceLoc,
    ) -> Result<Syntax, String> {
        self.expect(&JsToken::LParen)?;
        let mut param_names = Vec::new();
        if *self.peek() != JsToken::RParen {
            if *self.peek() == JsToken::DotDotDot {
                self.advance();
                let name = self.expect_ident()?;
                param_names.push(format!("&{}", name)); // marker for rest
            } else {
                param_names.push(self.expect_ident()?);
            }
            while *self.peek() == JsToken::Comma {
                self.advance();
                if *self.peek() == JsToken::DotDotDot {
                    self.advance();
                    let name = self.expect_ident()?;
                    param_names.push(format!("&{}", name));
                    break;
                }
                param_names.push(self.expect_ident()?);
            }
        }
        self.expect(&JsToken::RParen)?;
        self.expect(&JsToken::Arrow)?;

        // Build param names, expanding rest markers
        let mut final_names: Vec<String> = Vec::new();
        for p in &param_names {
            if let Some(rest) = p.strip_prefix('&') {
                final_names.push("&".to_string());
                final_names.push(rest.to_string());
            } else {
                final_names.push(p.clone());
            }
        }

        self.parse_arrow_body(&final_names, loc)
    }

    pub(super) fn parse_arrow_body(
        &mut self,
        param_names: &[String],
        loc: &crate::reader::token::SourceLoc,
    ) -> Result<Syntax, String> {
        let body = if *self.peek() == JsToken::LBrace {
            self.parse_brace_block()?
        } else {
            self.parse_expr()?
        };

        let span = self.span_from(loc);
        let params: Vec<Syntax> = param_names.iter().map(|n| self.sym(n, loc)).collect();
        let param_list = self.list(params, span);
        Ok(self.list(vec![self.sym("fn", loc), param_list, body], span))
    }

    /// Parse template literal interpolation.
    /// `hello ${expr} world` → `(string "hello " expr " world")`
    pub(super) fn parse_template_expr(
        &mut self,
        head: String,
        loc: &crate::reader::token::SourceLoc,
    ) -> Result<Syntax, String> {
        let span = self.span_from(loc);
        let mut parts: Vec<Syntax> = vec![self.sym("string", loc)];
        if !head.is_empty() {
            parts.push(Syntax::new(
                SyntaxKind::String(self.arena.text(&head)),
                span,
            ));
        }

        // Parse the interpolated expression
        let expr = self.parse_expr()?;
        parts.push(expr);

        // Continue reading template segments
        loop {
            match self.peek().clone() {
                JsToken::TemplateTail(tail) => {
                    self.advance();
                    if !tail.is_empty() {
                        parts.push(Syntax::new(
                            SyntaxKind::String(self.arena.text(&tail)),
                            span,
                        ));
                    }
                    break;
                }
                JsToken::TemplateMiddle(mid) => {
                    self.advance();
                    if !mid.is_empty() {
                        parts.push(Syntax::new(SyntaxKind::String(self.arena.text(&mid)), span));
                    }
                    let expr = self.parse_expr()?;
                    parts.push(expr);
                }
                _ => {
                    return Err(format!(
                        "{}: expected template continuation, got {:?}",
                        loc.position(),
                        self.peek()
                    ));
                }
            }
        }

        Ok(self.list(parts, span))
    }

    // ── Array and object literals ─────────────────────────────────────

    pub(super) fn parse_array_literal(&mut self) -> Result<Syntax, String> {
        let loc = self.peek_loc().loc.clone();
        self.expect(&JsToken::LBracket)?;
        let mut elements = Vec::new();
        while *self.peek() != JsToken::RBracket {
            if *self.peek() == JsToken::DotDotDot {
                // Spread: [...arr]
                let spread_loc = self.peek_loc().loc.clone();
                self.advance();
                let expr = self.parse_expr()?;
                elements.push(Syntax::new(
                    SyntaxKind::Splice(self.arena.node(expr)),
                    self.span_from(&spread_loc),
                ));
            } else {
                elements.push(self.parse_expr()?);
            }
            if *self.peek() == JsToken::Comma {
                self.advance();
            }
        }
        self.expect(&JsToken::RBracket)?;
        // JS arrays are mutable → @array
        Ok(Syntax::new(
            SyntaxKind::ArrayMut(self.arena.nodes(&elements)),
            self.span_from(&loc),
        ))
    }

    pub(super) fn parse_object_literal(&mut self) -> Result<Syntax, String> {
        let loc = self.peek_loc().loc.clone();
        self.expect(&JsToken::LBrace)?;
        let mut elements = Vec::new();

        while *self.peek() != JsToken::RBrace {
            if *self.peek() == JsToken::DotDotDot {
                // Spread: {...obj} — for now, skip spread in objects
                // and just parse the expression
                self.advance();
                let _expr = self.parse_expr()?;
                // TODO: handle object spread properly
            } else {
                // key: value  or  shorthand  or  computed [key]: value
                let key = match self.peek().clone() {
                    JsToken::Ident(name) => {
                        self.advance();
                        if *self.peek() == JsToken::Colon {
                            self.advance();
                            let value = self.parse_expr()?;
                            elements.push(Syntax::new(
                                SyntaxKind::Keyword(self.arena.text(&name)),
                                self.span_from(&loc),
                            ));
                            elements.push(value);
                        } else if *self.peek() == JsToken::LParen {
                            // Method shorthand: name(params) { body }
                            let func = self.parse_function_body(&loc)?;
                            elements.push(Syntax::new(
                                SyntaxKind::Keyword(self.arena.text(&name)),
                                self.span_from(&loc),
                            ));
                            elements.push(func);
                        } else {
                            // Shorthand: {x} → {:x x}
                            elements.push(Syntax::new(
                                SyntaxKind::Keyword(self.arena.text(&name)),
                                self.span_from(&loc),
                            ));
                            elements.push(Syntax::new(
                                SyntaxKind::Symbol(self.arena.text(&name)),
                                self.span_from(&loc),
                            ));
                        }
                        if *self.peek() == JsToken::Comma {
                            self.advance();
                        }
                        continue;
                    }
                    JsToken::String(s) => {
                        self.advance();
                        s
                    }
                    _ => {
                        return Err(format!(
                            "{}: expected property name, got {:?}",
                            self.peek_loc().loc.position(),
                            self.peek()
                        ));
                    }
                };
                self.expect(&JsToken::Colon)?;
                let value = self.parse_expr()?;
                elements.push(Syntax::new(
                    SyntaxKind::Keyword(self.arena.text(&key)),
                    self.span_from(&loc),
                ));
                elements.push(value);
            }

            if *self.peek() == JsToken::Comma {
                self.advance();
            }
        }
        self.expect(&JsToken::RBrace)?;
        // JS objects are mutable → @struct
        Ok(Syntax::new(
            SyntaxKind::StructMut(self.arena.nodes(&elements)),
            self.span_from(&loc),
        ))
    }
}
