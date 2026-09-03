use super::*;

impl LuaParser {
    pub(super) fn parse_expr(&mut self) -> Result<Syntax, String> {
        self.parse_pratt(0)
    }

    /// Pratt expression parser. Precedence levels (low → high):
    /// 0: or
    /// 1: and
    /// 2: < > <= >= ~= ==
    /// 3: .. (right-assoc)
    /// 4: + -
    /// 5: * / %
    /// 6: unary not # -
    /// 7: ^ (right-assoc)
    /// 8: atoms, calls, field access
    pub(super) fn parse_pratt(&mut self, min_prec: u8) -> Result<Syntax, String> {
        let mut lhs = self.parse_unary()?;

        loop {
            let (op_name, prec, right_assoc) = match self.peek() {
                LuaToken::Or => ("or", 0, false),
                LuaToken::And => ("and", 1, false),
                LuaToken::Lt => ("<", 2, false),
                LuaToken::Gt => (">", 2, false),
                LuaToken::Le => ("<=", 2, false),
                LuaToken::Ge => (">=", 2, false),
                LuaToken::Eq => ("=", 2, false),
                LuaToken::Neq => ("neq", 2, false), // special handling below
                LuaToken::DotDot => ("string", 3, true),
                LuaToken::Plus => ("+", 4, false),
                LuaToken::Minus => ("-", 4, false),
                LuaToken::Star => ("*", 5, false),
                LuaToken::Slash => ("/", 5, false),
                LuaToken::Percent => ("%", 5, false),
                LuaToken::Caret => ("math/pow", 7, true),
                _ => break,
            };

            if prec < min_prec {
                break;
            }

            let loc = self.peek_loc().loc.clone();
            let is_neq = *self.peek() == LuaToken::Neq;
            self.advance();

            let next_prec = if right_assoc { prec } else { prec + 1 };
            let rhs = self.parse_pratt(next_prec)?;

            let span = lhs.span.merge(&rhs.span);
            if is_neq {
                // ~= → (not (= lhs rhs))
                let eq = self.list(vec![self.sym("=", &loc), lhs, rhs], span);
                lhs = self.list(vec![self.sym("not", &loc), eq], span);
            } else {
                lhs = self.list(vec![self.sym(op_name, &loc), lhs, rhs], span);
            }
        }

        Ok(lhs)
    }

    pub(super) fn parse_unary(&mut self) -> Result<Syntax, String> {
        let loc = self.peek_loc().loc.clone();
        match self.peek().clone() {
            LuaToken::Not => {
                self.advance();
                let operand = self.parse_pratt(6)?;
                let span = self.span_from(&loc).merge(&operand.span);
                Ok(self.list(vec![self.sym("not", &loc), operand], span))
            }
            LuaToken::Hash => {
                self.advance();
                let operand = self.parse_pratt(6)?;
                let span = self.span_from(&loc).merge(&operand.span);
                Ok(self.list(vec![self.sym("length", &loc), operand], span))
            }
            LuaToken::Minus => {
                // Check if this is unary minus (not binary minus in a binary context)
                // Unary minus: at start, or after operator/delimiter
                self.advance();
                let operand = self.parse_pratt(6)?;
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
            _ => self.parse_postfix(),
        }
    }

    pub(super) fn parse_postfix(&mut self) -> Result<Syntax, String> {
        let mut expr = self.parse_atom()?;

        loop {
            match self.peek().clone() {
                // Function call: f(args)
                LuaToken::LParen => {
                    expr = self.parse_call(expr)?;
                }
                // Call-without-parens: f "hello" or f {1, 2}
                LuaToken::String(_) => {
                    let arg = self.parse_atom()?;
                    let span = expr.span.merge(&arg.span);
                    expr = self.list(vec![expr, arg], span);
                }
                LuaToken::LBrace => {
                    let arg = self.parse_table()?;
                    let span = expr.span.merge(&arg.span);
                    expr = self.list(vec![expr, arg], span);
                }
                // Field access: t.foo → (get t :foo)
                LuaToken::Dot => {
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
                // Index access: t[k] → (get t k)
                LuaToken::LBracket => {
                    let loc = self.peek_loc().loc.clone();
                    self.advance();
                    let key = self.parse_expr()?;
                    self.expect(&LuaToken::RBracket)?;
                    let span = expr.span.merge(&key.span);
                    expr = self.list(vec![self.sym("get", &loc), expr, key], span);
                }
                // Method call: obj:method(args) → (obj:method args...)
                LuaToken::Colon => {
                    let loc = self.peek_loc().loc.clone();
                    self.advance();
                    let method = self.expect_ident()?;
                    // Build qualified symbol "obj_expr:method"
                    // For simple identifiers, produce a qualified symbol;
                    // for complex expressions, desugar to a method call pattern.
                    if let SyntaxKind::Symbol(ref obj_name) = expr.kind {
                        let qualified = format!("{}:{}", obj_name, method);
                        let args = self.parse_arglist()?;
                        let span = self.span_from(&loc);
                        let mut items = vec![self.sym(&qualified, &loc)];
                        items.extend(args);
                        expr = self.list(items, span);
                    } else {
                        // Complex receiver: desugar to ((get obj :method) obj args...)
                        let args = self.parse_arglist()?;
                        let span = self.span_from(&loc);
                        let kw = Syntax::new(
                            SyntaxKind::Keyword(self.arena.text(&method)),
                            self.span_from(&loc),
                        );
                        let getter = self.list(vec![self.sym("get", &loc), expr, kw], span);
                        let mut items = vec![getter, expr];
                        items.extend(args);
                        expr = self.list(items, span);
                    }
                }
                _ => break,
            }
        }

        Ok(expr)
    }

    pub(super) fn parse_call(&mut self, func: Syntax) -> Result<Syntax, String> {
        let args = self.parse_arglist()?;
        let loc = &func.span.clone();
        let span = Span::new(loc.start as usize, loc.end as usize, loc.line, loc.col)
            .with_file(&self.file);
        let mut items = vec![func];
        items.extend(args);
        Ok(self.list(items, span))
    }

    pub(super) fn parse_arglist(&mut self) -> Result<Vec<Syntax>, String> {
        self.expect(&LuaToken::LParen)?;
        let mut args = Vec::new();
        if *self.peek() != LuaToken::RParen {
            args.push(self.parse_expr()?);
            while *self.peek() == LuaToken::Comma {
                self.advance();
                args.push(self.parse_expr()?);
            }
        }
        self.expect(&LuaToken::RParen)?;
        Ok(args)
    }

    pub(super) fn parse_atom(&mut self) -> Result<Syntax, String> {
        let loc = self.peek_loc().loc.clone();
        let len = self.peek_loc().len;
        match self.peek().clone() {
            LuaToken::Int(n) => {
                self.advance();
                Ok(Syntax::new(SyntaxKind::Int(n), self.make_span(&loc, len)))
            }
            LuaToken::Float(f) => {
                self.advance();
                Ok(Syntax::new(SyntaxKind::Float(f), self.make_span(&loc, len)))
            }
            LuaToken::String(s) => {
                self.advance();
                Ok(Syntax::new(
                    SyntaxKind::String(self.arena.text(&s)),
                    self.make_span(&loc, len),
                ))
            }
            LuaToken::True => {
                self.advance();
                Ok(Syntax::new(
                    SyntaxKind::Bool(true),
                    self.make_span(&loc, len),
                ))
            }
            LuaToken::False => {
                self.advance();
                Ok(Syntax::new(
                    SyntaxKind::Bool(false),
                    self.make_span(&loc, len),
                ))
            }
            LuaToken::Nil => {
                self.advance();
                Ok(Syntax::new(SyntaxKind::Nil, self.make_span(&loc, len)))
            }
            LuaToken::Ident(name) => {
                self.advance();
                Ok(Syntax::new(
                    SyntaxKind::Symbol(self.arena.text(&name)),
                    self.make_span(&loc, len),
                ))
            }

            // Grouping
            LuaToken::LParen => {
                self.advance();
                let expr = self.parse_expr()?;
                self.expect(&LuaToken::RParen)?;
                Ok(expr)
            }

            // Varargs: ... → (splice __varargs)
            LuaToken::DotDotDot => {
                self.advance();
                let inner = self.sym(VARARGS, &loc);
                let span = self.make_span(&loc, 3);
                Ok(Syntax::new(
                    SyntaxKind::Splice(self.arena.node(inner)),
                    span,
                ))
            }

            // Table constructor
            LuaToken::LBrace => self.parse_table(),

            // Function literal
            LuaToken::Function => {
                self.advance();
                self.parse_function_body(&loc)
            }

            // Backtick s-expr escape: `(sexpr)`
            LuaToken::Backtick => {
                self.advance();
                self.parse_sexpr_escape()
            }

            _ => Err(format!(
                "{}: unexpected token {:?}",
                loc.position(),
                self.peek()
            )),
        }
    }

    // ── Table constructors ────────────────────────────────────────────

    pub(super) fn parse_table(&mut self) -> Result<Syntax, String> {
        let loc = self.peek_loc().loc.clone();
        self.expect(&LuaToken::LBrace)?;

        if *self.peek() == LuaToken::RBrace {
            self.advance();
            // Empty table → empty mutable struct (works as Lua "object")
            return Ok(Syntax::new(
                SyntaxKind::StructMut(self.arena.nodes(&[])),
                self.span_from(&loc),
            ));
        }

        // Peek at the first entry to decide: if it's `ident =` then struct, else array.
        let is_struct = self.looks_like_struct_entry();

        if is_struct {
            self.parse_struct_table(&loc)
        } else {
            self.parse_array_table(&loc)
        }
    }

    pub(super) fn looks_like_struct_entry(&self) -> bool {
        // Check if current token is Ident and next is Assign
        if let Some(t1) = self.cursor.current() {
            if let LuaToken::Ident(_) = &t1.token {
                if let Some(t2) = self.cursor.nth(1) {
                    return t2.token == LuaToken::Assign;
                }
            }
        }
        false
    }

    pub(super) fn parse_struct_table(
        &mut self,
        loc: &crate::reader::token::SourceLoc,
    ) -> Result<Syntax, String> {
        let mut elements = Vec::new();
        loop {
            if *self.peek() == LuaToken::RBrace {
                break;
            }
            let key = self.expect_ident()?;
            self.expect(&LuaToken::Assign)?;
            let value = self.parse_expr()?;
            elements.push(self.kw(&key, self.span_from(loc)));
            elements.push(value);

            match self.peek() {
                LuaToken::Comma | LuaToken::Semicolon => {
                    self.advance();
                }
                _ => {}
            }
        }
        self.expect(&LuaToken::RBrace)?;
        Ok(Syntax::new(
            SyntaxKind::StructMut(self.arena.nodes(&elements)),
            self.span_from(loc),
        ))
    }

    pub(super) fn parse_array_table(
        &mut self,
        loc: &crate::reader::token::SourceLoc,
    ) -> Result<Syntax, String> {
        let mut elements = Vec::new();
        loop {
            if *self.peek() == LuaToken::RBrace {
                break;
            }
            elements.push(self.parse_expr()?);
            match self.peek() {
                LuaToken::Comma | LuaToken::Semicolon => {
                    self.advance();
                }
                _ => {}
            }
        }
        self.expect(&LuaToken::RBrace)?;
        Ok(Syntax::new(
            SyntaxKind::ArrayMut(self.arena.nodes(&elements)),
            self.span_from(loc),
        ))
    }

    // ── Backtick s-expr escape ────────────────────────────────────────

    pub(super) fn parse_sexpr_escape(&mut self) -> Result<Syntax, String> {
        // After backtick, expect `(` then collect tokens until matching `)`
        let loc = self.peek_loc().loc.clone();
        self.expect(&LuaToken::LParen)?;
        let mut depth = 1u32;
        let mut sexpr_text = String::from("(");

        while depth > 0 {
            match self.peek() {
                LuaToken::Eof => {
                    return Err(format!(
                        "{}: unterminated backtick s-expression",
                        loc.position()
                    ));
                }
                LuaToken::LParen => {
                    depth += 1;
                    sexpr_text.push('(');
                    self.advance();
                }
                LuaToken::RParen => {
                    depth -= 1;
                    sexpr_text.push(')');
                    self.advance();
                }
                _ => {
                    // Reconstruct token text
                    sexpr_text.push_str(&self.token_to_text());
                    sexpr_text.push(' ');
                    self.advance();
                }
            }
        }

        // Parse the collected s-expression using the Elle reader
        let syntaxes = crate::reader::read_syntax(self.arena, &sexpr_text, &self.file)?;
        Ok(syntaxes)
    }

    pub(super) fn token_to_text(&self) -> String {
        match self.peek() {
            LuaToken::Int(n) => n.to_string(),
            LuaToken::Float(f) => f.to_string(),
            LuaToken::String(s) => format!("\"{}\"", s),
            LuaToken::True => "true".to_string(),
            LuaToken::False => "false".to_string(),
            LuaToken::Nil => "nil".to_string(),
            LuaToken::Ident(s) => s.clone(),
            LuaToken::Plus => "+".to_string(),
            LuaToken::Minus => "-".to_string(),
            LuaToken::Star => "*".to_string(),
            LuaToken::Slash => "/".to_string(),
            LuaToken::Percent => "%".to_string(),
            LuaToken::Caret => "^".to_string(),
            LuaToken::Eq => "=".to_string(),
            LuaToken::Neq => "~=".to_string(),
            LuaToken::Lt => "<".to_string(),
            LuaToken::Gt => ">".to_string(),
            LuaToken::Le => "<=".to_string(),
            LuaToken::Ge => ">=".to_string(),
            LuaToken::Assign => "=".to_string(),
            LuaToken::DotDot => "..".to_string(),
            LuaToken::Hash => "#".to_string(),
            LuaToken::Dot => ".".to_string(),
            LuaToken::Colon => ":".to_string(),
            LuaToken::Comma => ",".to_string(),
            LuaToken::Semicolon => ";".to_string(),
            LuaToken::LBracket => "[".to_string(),
            LuaToken::RBracket => "]".to_string(),
            LuaToken::LBrace => "{".to_string(),
            LuaToken::RBrace => "}".to_string(),
            LuaToken::Function => "function".to_string(),
            LuaToken::End => "end".to_string(),
            LuaToken::If => "if".to_string(),
            LuaToken::Then => "then".to_string(),
            LuaToken::Else => "else".to_string(),
            LuaToken::ElseIf => "elseif".to_string(),
            LuaToken::While => "while".to_string(),
            LuaToken::Do => "do".to_string(),
            LuaToken::For => "for".to_string(),
            LuaToken::In => "in".to_string(),
            LuaToken::Local => "local".to_string(),
            LuaToken::Return => "return".to_string(),
            LuaToken::And => "and".to_string(),
            LuaToken::Or => "or".to_string(),
            LuaToken::Not => "not".to_string(),
            LuaToken::Break => "break".to_string(),
            LuaToken::Repeat => "repeat".to_string(),
            LuaToken::Until => "until".to_string(),
            LuaToken::DotDotDot => "...".to_string(),
            LuaToken::Backtick => "`".to_string(),
            LuaToken::LParen => "(".to_string(),
            LuaToken::RParen => ")".to_string(),
            LuaToken::Eof => "".to_string(),
        }
    }
}
