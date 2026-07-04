use super::*;

impl PyParser {
    pub(super) fn parse_expr(&mut self) -> Result<Syntax, String> {
        // Check for ternary: expr if cond else alt
        let expr = self.parse_pratt(0)?;

        // Python ternary: value if cond else alt
        if *self.peek() == PyToken::If {
            let loc = self.peek_loc().loc.clone();
            self.advance();
            let cond = self.parse_pratt(0)?;
            self.expect(&PyToken::Else)?;
            let alt = self.parse_expr()?;
            let span = expr.span.merge(&alt.span);
            return Ok(self.list(vec![self.sym("if", &loc), cond, expr, alt], span));
        }

        // Lambda: lambda params: expr
        // (handled in parse_atom)

        Ok(expr)
    }

    /// Pratt expression parser.  Precedence levels (low → high):
    ///  0: or
    ///  1: and
    ///  2: not (unary, but handled specially)
    ///  3: in, not in, is, is not, == != < > <= >=
    ///  4: | (bitwise)
    ///  5: ^ (bitwise)
    ///  6: & (bitwise)
    ///  7: << >>
    ///  8: + -
    ///  9: * / // %
    /// 10: ** (right-assoc)
    /// 11: unary + - ~
    /// 12: atoms, calls, field access
    pub(super) fn parse_pratt(&mut self, min_prec: u8) -> Result<Syntax, String> {
        let mut lhs = self.parse_unary()?;

        loop {
            let (op_name, prec, right_assoc) = match self.peek() {
                PyToken::Or => ("or", 0, false),
                PyToken::And => ("and", 1, false),
                PyToken::In => ("contains?", 3, false),
                PyToken::Is => ("=", 3, false), // simplified
                PyToken::Eq => ("=", 3, false),
                PyToken::Neq => ("neq", 3, false),
                PyToken::Lt => ("<", 3, false),
                PyToken::Gt => (">", 3, false),
                PyToken::Le => ("<=", 3, false),
                PyToken::Ge => (">=", 3, false),
                PyToken::Pipe => ("bit/or", 4, false),
                PyToken::Caret => ("bit/xor", 5, false),
                PyToken::Ampersand => ("bit/and", 6, false),
                PyToken::ShiftLeft => ("bit/shift-left", 7, false),
                PyToken::ShiftRight => ("bit/shift-right", 7, false),
                PyToken::Plus => ("+", 8, false),
                PyToken::Minus => ("-", 8, false),
                PyToken::Star => ("*", 9, false),
                PyToken::Slash => ("/", 9, false),
                PyToken::SlashSlash => ("div", 9, false),
                PyToken::Percent => ("%", 9, false),
                PyToken::StarStar => ("math/pow", 10, true),
                _ => break,
            };

            if prec < min_prec {
                break;
            }

            let loc = self.peek_loc().loc.clone();

            // Handle `not in` as a compound operator
            if *self.peek() == PyToken::Not {
                // Check if next is `in` — but `not` at this point means
                // we're seeing it as a binary op which shouldn't happen.
                // `not` as unary is handled in parse_unary.
                break;
            }

            let is_neq = *self.peek() == PyToken::Neq;
            let is_in = *self.peek() == PyToken::In;
            self.advance();

            let next_prec = if right_assoc { prec } else { prec + 1 };
            let rhs = self.parse_pratt(next_prec)?;

            let span = lhs.span.merge(&rhs.span);
            if is_neq {
                let eq = self.list(vec![self.sym("=", &loc), lhs, rhs], span.clone());
                lhs = self.list(vec![self.sym("not", &loc), eq], span);
            } else if is_in {
                // x in y → (contains? y x) — note argument order
                lhs = self.list(vec![self.sym("contains?", &loc), rhs, lhs], span);
            } else {
                lhs = self.list(vec![self.sym(op_name, &loc), lhs, rhs], span);
            }
        }

        // Handle `not in`: expr not in expr
        if *self.peek() == PyToken::Not {
            let saved = self.cursor.pos();
            let loc = self.peek_loc().loc.clone();
            self.advance();
            if *self.peek() == PyToken::In && min_prec <= 3 {
                self.advance();
                let rhs = self.parse_pratt(4)?;
                let span = lhs.span.merge(&rhs.span);
                let contains = self.list(vec![self.sym("contains?", &loc), rhs, lhs], span.clone());
                lhs = self.list(vec![self.sym("not", &loc), contains], span);
            } else {
                self.cursor.seek(saved); // not a `not in`, backtrack
            }
        }

        Ok(lhs)
    }

    pub(super) fn parse_unary(&mut self) -> Result<Syntax, String> {
        let loc = self.peek_loc().loc.clone();
        match self.peek().clone() {
            PyToken::Not => {
                self.advance();
                let operand = self.parse_pratt(2)?;
                let span = self.span_from(&loc).merge(&operand.span);
                Ok(self.list(vec![self.sym("not", &loc), operand], span))
            }
            PyToken::Minus => {
                self.advance();
                let operand = self.parse_pratt(11)?;
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
            PyToken::Plus => {
                self.advance();
                self.parse_pratt(11)
            }
            PyToken::Tilde => {
                self.advance();
                let operand = self.parse_pratt(11)?;
                let span = self.span_from(&loc).merge(&operand.span);
                Ok(self.list(vec![self.sym("bit/not", &loc), operand], span))
            }
            _ => self.parse_postfix(),
        }
    }

    pub(super) fn parse_postfix(&mut self) -> Result<Syntax, String> {
        let mut expr = self.parse_atom()?;

        loop {
            match self.peek().clone() {
                // Function call: f(args)
                PyToken::LParen => {
                    expr = self.parse_call(expr)?;
                }
                // Field access: obj.field → (get obj :field)
                PyToken::Dot => {
                    let loc = self.peek_loc().loc.clone();
                    self.advance();
                    let field = self.expect_ident()?;
                    let span = expr.span.merge(&self.span_from(&loc));
                    let kw = Syntax::new(SyntaxKind::Keyword(field), self.span_from(&loc));
                    expr = self.list(vec![self.sym("get", &loc), expr, kw], span);
                }
                // Index access: obj[key] → (get obj key)
                PyToken::LBracket => {
                    let loc = self.peek_loc().loc.clone();
                    self.advance();
                    let key = self.parse_expr()?;
                    self.expect(&PyToken::RBracket)?;
                    let span = expr.span.merge(&key.span);
                    expr = self.list(vec![self.sym("get", &loc), expr, key], span);
                }
                _ => break,
            }
        }

        Ok(expr)
    }

    pub(super) fn parse_call(&mut self, func: Syntax) -> Result<Syntax, String> {
        let args = self.parse_arglist()?;
        let loc = &func.span.clone();
        let span = Span::new(loc.start, loc.end, loc.line, loc.col).with_file(self.file.clone());
        let mut items = vec![func];
        items.extend(args);
        Ok(self.list(items, span))
    }

    pub(super) fn parse_arglist(&mut self) -> Result<Vec<Syntax>, String> {
        self.expect(&PyToken::LParen)?;
        let mut args = Vec::new();
        if *self.peek() != PyToken::RParen {
            // Handle *args spread
            if *self.peek() == PyToken::Star {
                let loc = self.peek_loc().loc.clone();
                self.advance();
                let expr = self.parse_expr()?;
                let span = self.span_from(&loc);
                args.push(Syntax::new(SyntaxKind::Splice(Box::new(expr)), span));
            } else {
                args.push(self.parse_expr()?);
            }
            while *self.peek() == PyToken::Comma {
                self.advance();
                if *self.peek() == PyToken::RParen {
                    break;
                }
                if *self.peek() == PyToken::Star {
                    let loc = self.peek_loc().loc.clone();
                    self.advance();
                    let expr = self.parse_expr()?;
                    let span = self.span_from(&loc);
                    args.push(Syntax::new(SyntaxKind::Splice(Box::new(expr)), span));
                } else {
                    // Check for keyword arg: name=value — skip name, use value
                    let expr = self.parse_expr()?;
                    if *self.peek() == PyToken::Assign {
                        // keyword arg — skip for now, just use the value
                        self.advance();
                        let val = self.parse_expr()?;
                        args.push(val);
                    } else {
                        args.push(expr);
                    }
                }
            }
        }
        self.expect(&PyToken::RParen)?;
        Ok(args)
    }

    pub(super) fn parse_atom(&mut self) -> Result<Syntax, String> {
        let loc = self.peek_loc().loc.clone();
        let len = self.peek_loc().len;
        match self.peek().clone() {
            PyToken::Int(n) => {
                self.advance();
                Ok(Syntax::new(SyntaxKind::Int(n), self.make_span(&loc, len)))
            }
            PyToken::Float(f) => {
                self.advance();
                Ok(Syntax::new(SyntaxKind::Float(f), self.make_span(&loc, len)))
            }
            PyToken::String(s) => {
                self.advance();
                // Check for implicit string concatenation
                let mut result = s;
                while let PyToken::String(s2) = self.peek().clone() {
                    self.advance();
                    result.push_str(&s2);
                }
                Ok(Syntax::new(
                    SyntaxKind::String(result),
                    self.make_span(&loc, len),
                ))
            }
            PyToken::FString(parts) => {
                self.advance();
                self.build_fstring(parts, &loc)
            }
            PyToken::True => {
                self.advance();
                Ok(Syntax::new(
                    SyntaxKind::Bool(true),
                    self.make_span(&loc, len),
                ))
            }
            PyToken::False => {
                self.advance();
                Ok(Syntax::new(
                    SyntaxKind::Bool(false),
                    self.make_span(&loc, len),
                ))
            }
            PyToken::None => {
                self.advance();
                Ok(Syntax::new(SyntaxKind::Nil, self.make_span(&loc, len)))
            }
            PyToken::Ident(name) => {
                self.advance();
                Ok(Syntax::new(
                    SyntaxKind::Symbol(name),
                    self.make_span(&loc, len),
                ))
            }

            // Grouping or tuple
            PyToken::LParen => {
                self.advance();
                if *self.peek() == PyToken::RParen {
                    // Empty tuple → nil
                    self.advance();
                    return Ok(self.nil_syntax(&loc));
                }
                let expr = self.parse_expr()?;
                if *self.peek() == PyToken::Comma {
                    // Tuple: (a, b, c) → [a b c]
                    let mut elements = vec![expr];
                    while *self.peek() == PyToken::Comma {
                        self.advance();
                        if *self.peek() == PyToken::RParen {
                            break;
                        }
                        elements.push(self.parse_expr()?);
                    }
                    self.expect(&PyToken::RParen)?;
                    return Ok(Syntax::new(
                        SyntaxKind::Array(elements),
                        self.span_from(&loc),
                    ));
                }
                self.expect(&PyToken::RParen)?;
                Ok(expr)
            }

            // List literal: [1, 2, 3]
            PyToken::LBracket => self.parse_list_literal(),

            // Dict literal: {"key": val}
            PyToken::LBrace => self.parse_dict_literal(),

            // Lambda: lambda params: expr
            PyToken::Lambda => {
                self.advance();
                let mut params = Vec::new();
                if *self.peek() != PyToken::Colon {
                    if *self.peek() == PyToken::Star {
                        self.advance();
                        let name = self.expect_ident()?;
                        params.push(self.sym(REST_PARAM, &loc));
                        params.push(self.sym(&name, &loc));
                    } else {
                        let name = self.expect_ident()?;
                        params.push(self.sym(&name, &loc));
                        while *self.peek() == PyToken::Comma {
                            self.advance();
                            if *self.peek() == PyToken::Star {
                                self.advance();
                                let name = self.expect_ident()?;
                                params.push(self.sym(REST_PARAM, &loc));
                                params.push(self.sym(&name, &loc));
                                break;
                            }
                            let name = self.expect_ident()?;
                            params.push(self.sym(&name, &loc));
                        }
                    }
                }
                self.expect(&PyToken::Colon)?;
                let body = self.parse_expr()?;
                let span = self.span_from(&loc);
                let param_list = self.list(params, span.clone());
                Ok(self.list(vec![self.sym("fn", &loc), param_list, body], span))
            }

            _ => Err(format!(
                "{}: unexpected token {:?}",
                loc.position(),
                self.peek()
            )),
        }
    }

    /// Build f-string: f"hello {name}!" → (string "hello " name "!")
    pub(super) fn build_fstring(
        &mut self,
        parts: Vec<FStringPart>,
        loc: &crate::reader::token::SourceLoc,
    ) -> Result<Syntax, String> {
        let span = self.span_from(loc);
        if parts.len() == 1 {
            if let FStringPart::Lit(s) = &parts[0] {
                return Ok(Syntax::new(SyntaxKind::String(s.clone()), span));
            }
        }

        let mut items: Vec<Syntax> = vec![self.sym("string", loc)];
        for part in parts {
            match part {
                FStringPart::Lit(s) => {
                    if !s.is_empty() {
                        items.push(Syntax::new(SyntaxKind::String(s), span.clone()));
                    }
                }
                FStringPart::Expr(expr_str) => {
                    // Parse the expression string
                    let syntax = crate::reader::read_syntax_all_for(&expr_str, &self.file)?;
                    if let Some(s) = syntax.into_iter().next() {
                        items.push(s);
                    }
                }
            }
        }

        Ok(self.list(items, span))
    }

    pub(super) fn parse_list_literal(&mut self) -> Result<Syntax, String> {
        let loc = self.peek_loc().loc.clone();
        self.expect(&PyToken::LBracket)?;
        let mut elements = Vec::new();
        while *self.peek() != PyToken::RBracket {
            if *self.peek() == PyToken::Star {
                // Spread: [*arr]
                let spread_loc = self.peek_loc().loc.clone();
                self.advance();
                let expr = self.parse_expr()?;
                elements.push(Syntax::new(
                    SyntaxKind::Splice(Box::new(expr)),
                    self.span_from(&spread_loc),
                ));
            } else {
                elements.push(self.parse_expr()?);
            }
            if *self.peek() == PyToken::Comma {
                self.advance();
            }
        }
        self.expect(&PyToken::RBracket)?;
        // Python lists are mutable
        Ok(Syntax::new(
            SyntaxKind::ArrayMut(elements),
            self.span_from(&loc),
        ))
    }

    pub(super) fn parse_dict_literal(&mut self) -> Result<Syntax, String> {
        let loc = self.peek_loc().loc.clone();
        self.expect(&PyToken::LBrace)?;
        let mut elements = Vec::new();

        while *self.peek() != PyToken::RBrace {
            let key_expr = self.parse_expr()?;
            self.expect(&PyToken::Colon)?;
            let value = self.parse_expr()?;

            // If key is a string literal, use it as keyword
            match &key_expr.kind {
                SyntaxKind::String(s) => {
                    elements.push(Syntax::new(
                        SyntaxKind::Keyword(s.clone()),
                        self.span_from(&loc),
                    ));
                }
                _ => {
                    // Dynamic key — can't use keyword syntax
                    // Fall back to a different representation
                    elements.push(key_expr);
                }
            }
            elements.push(value);

            if *self.peek() == PyToken::Comma {
                self.advance();
            }
        }
        self.expect(&PyToken::RBrace)?;
        // Python dicts are mutable
        Ok(Syntax::new(
            SyntaxKind::StructMut(elements),
            self.span_from(&loc),
        ))
    }
}
