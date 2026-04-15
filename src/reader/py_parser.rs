//! Recursive-descent + Pratt parser for Python surface syntax.
//!
//! Parses Python source into `Vec<Syntax>` — the same trees the
//! s-expression reader produces.  The rest of the pipeline (expander →
//! analyzer → lowerer → emitter → VM) is unchanged.

use super::py_lexer::{FStringPart, PyLexer, PyToken, PyTokenLoc};
use crate::syntax::{Span, Syntax, SyntaxKind};

/// Parse a `.py` file into top-level `Syntax` forms.
pub fn parse_py_file(input: &str, source_name: &str) -> Result<Vec<Syntax>, String> {
    // Strip shebang if present
    let input_clean = if input.starts_with("#!") {
        input.lines().skip(1).collect::<Vec<_>>().join("\n")
    } else {
        input.to_string()
    };

    let mut lexer = PyLexer::new(&input_clean, source_name);
    let tokens = lexer.tokenize()?;
    let mut parser = PyParser::new(tokens, source_name);
    parser.parse_file()
}

struct PyParser {
    tokens: Vec<PyTokenLoc>,
    pos: usize,
    file: String,
    /// Nesting depth: 0 = top-level, >0 = inside function/loop/if.
    /// At depth 0, `x = val` emits `(var x val)` (new binding).
    /// At depth >0, `x = val` emits `(assign x val)` (mutation).
    depth: u32,
}

impl PyParser {
    fn new(tokens: Vec<PyTokenLoc>, file: &str) -> Self {
        PyParser {
            tokens,
            pos: 0,
            file: file.to_string(),
            depth: 0,
        }
    }

    // ── Token navigation ──────────────────────────────────────────────

    fn peek(&self) -> &PyToken {
        self.tokens
            .get(self.pos)
            .map(|t| &t.token)
            .unwrap_or(&PyToken::Eof)
    }

    fn peek_loc(&self) -> &PyTokenLoc {
        static EOF_LOC: std::sync::LazyLock<PyTokenLoc> = std::sync::LazyLock::new(|| PyTokenLoc {
            token: PyToken::Eof,
            loc: super::token::SourceLoc::new("<eof>", 0, 0),
            len: 0,
        });
        self.tokens.get(self.pos).unwrap_or(&EOF_LOC)
    }

    fn advance(&mut self) -> &PyTokenLoc {
        let t = &self.tokens[self.pos];
        self.pos += 1;
        t
    }

    fn expect(&mut self, expected: &PyToken) -> Result<&PyTokenLoc, String> {
        if self.peek() == expected {
            Ok(self.advance())
        } else {
            let loc = &self.peek_loc().loc;
            Err(format!(
                "{}: expected {:?}, got {:?}",
                loc.position(),
                expected,
                self.peek()
            ))
        }
    }

    fn expect_ident(&mut self) -> Result<String, String> {
        match self.peek().clone() {
            PyToken::Ident(name) => {
                self.advance();
                Ok(name)
            }
            _ => {
                let loc = &self.peek_loc().loc;
                Err(format!(
                    "{}: expected identifier, got {:?}",
                    loc.position(),
                    self.peek()
                ))
            }
        }
    }

    fn eat_newlines(&mut self) {
        while *self.peek() == PyToken::Newline {
            self.advance();
        }
    }

    fn make_span(&self, loc: &super::token::SourceLoc, len: usize) -> Span {
        let mut span = Span::new(0, len, loc.line as u32, loc.col as u32);
        if !loc.is_unknown() {
            span = span.with_file(loc.file.clone());
        }
        span
    }

    fn span_from(&self, loc: &super::token::SourceLoc) -> Span {
        self.make_span(loc, 1)
    }

    fn sym(&self, name: &str, loc: &super::token::SourceLoc) -> Syntax {
        Syntax::new(SyntaxKind::Symbol(name.to_string()), self.span_from(loc))
    }

    fn list(&self, items: Vec<Syntax>, span: Span) -> Syntax {
        Syntax::new(SyntaxKind::List(items), span)
    }

    fn nil_syntax(&self, loc: &super::token::SourceLoc) -> Syntax {
        Syntax::new(SyntaxKind::Nil, self.span_from(loc))
    }

    // ── File-level parsing ────────────────────────────────────────────

    fn parse_file(&mut self) -> Result<Vec<Syntax>, String> {
        self.eat_newlines();
        let mut forms = Vec::new();
        while *self.peek() != PyToken::Eof {
            let stmt = self.parse_top_level_statement()?;
            forms.extend(stmt);
            self.eat_newlines();
        }
        Ok(forms)
    }

    fn parse_top_level_statement(&mut self) -> Result<Vec<Syntax>, String> {
        let loc = self.peek_loc().loc.clone();
        match self.peek().clone() {
            PyToken::Def => {
                self.advance();
                let name = self.expect_ident()?;
                let func = self.parse_function_def(&loc)?;
                let span = func.span.clone();
                let def = self.list(
                    vec![self.sym("def", &loc), self.sym(&name, &loc), func],
                    span,
                );
                Ok(vec![def])
            }
            PyToken::Import => {
                self.advance();
                let name = self.expect_ident()?;
                self.eat_newlines();
                let span = self.span_from(&loc);
                // import foo → (def foo (import "lib/foo"))
                let import_path = format!("lib/{}", name);
                let import_str = Syntax::new(SyntaxKind::String(import_path), span.clone());
                let import_call =
                    self.list(vec![self.sym("import", &loc), import_str], span.clone());
                Ok(vec![self.list(
                    vec![self.sym("def", &loc), self.sym(&name, &loc), import_call],
                    span,
                )])
            }
            _ => {
                let stmt = self.parse_statement()?;
                Ok(vec![stmt])
            }
        }
    }

    // ── Block parsing ─────────────────────────────────────────────────

    /// Parse an indented block after a colon.
    /// Expects: Colon Newline Indent statements... Dedent
    /// `is_function_body`: if true, creates a `block` scope (for def bodies).
    /// Otherwise creates `begin` (for if/while/for — Python has no block scoping).
    fn parse_block(&mut self) -> Result<Syntax, String> {
        self.parse_block_inner(false)
    }

    fn parse_function_block(&mut self) -> Result<Syntax, String> {
        self.parse_block_inner(true)
    }

    fn parse_block_inner(&mut self, is_function_body: bool) -> Result<Syntax, String> {
        let loc = self.peek_loc().loc.clone();
        self.expect(&PyToken::Colon)?;
        self.eat_newlines();
        self.expect(&PyToken::Indent)?;

        self.depth += 1;
        let mut stmts: Vec<Syntax> = Vec::new();

        while *self.peek() != PyToken::Dedent && *self.peek() != PyToken::Eof {
            self.eat_newlines();
            if *self.peek() == PyToken::Dedent || *self.peek() == PyToken::Eof {
                break;
            }
            let stmt = self.parse_statement()?;
            stmts.push(stmt);
            self.eat_newlines();
        }

        if *self.peek() == PyToken::Dedent {
            self.advance();
        }
        self.depth -= 1;

        Ok(self.stmts_to_body(stmts, &loc, is_function_body))
    }

    fn stmts_to_body(
        &self,
        mut stmts: Vec<Syntax>,
        loc: &super::token::SourceLoc,
        is_function_body: bool,
    ) -> Syntax {
        match stmts.len() {
            0 => self.nil_syntax(loc),
            1 => stmts.pop().unwrap(),
            _ => {
                // Function bodies use `block` to create a proper scope.
                // if/while/for bodies use `begin` — Python has function-scoped
                // variables, so assignments inside loops/ifs affect the
                // enclosing function scope.
                let head = if is_function_body { "block" } else { "begin" };
                let mut items = vec![self.sym(head, loc)];
                items.append(&mut stmts);
                self.list(items, self.span_from(loc))
            }
        }
    }

    // ── Statement parsing ─────────────────────────────────────────────

    fn parse_statement(&mut self) -> Result<Syntax, String> {
        let loc = self.peek_loc().loc.clone();
        match self.peek().clone() {
            PyToken::Def => {
                self.advance();
                let name = self.expect_ident()?;
                let func = self.parse_function_def(&loc)?;
                let span = func.span.clone();
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
                    Syntax::new(
                        SyntaxKind::String("assertion failed".to_string()),
                        self.span_from(&loc),
                    )
                };
                self.eat_newlines();
                let span = self.span_from(&loc);
                // (if (not cond) (error {:error :assertion-failed :message msg}) nil)
                let not_cond = self.list(vec![self.sym("not", &loc), cond], span.clone());
                let err_struct = Syntax::new(
                    SyntaxKind::Struct(vec![
                        Syntax::new(SyntaxKind::Keyword("error".into()), span.clone()),
                        Syntax::new(SyntaxKind::Keyword("assertion-failed".into()), span.clone()),
                        Syntax::new(SyntaxKind::Keyword("message".into()), span.clone()),
                        msg,
                    ]),
                    span.clone(),
                );
                let error_call = self.list(vec![self.sym("error", &loc), err_struct], span.clone());
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
                let import_str = Syntax::new(SyntaxKind::String(import_path), span.clone());
                let import_call =
                    self.list(vec![self.sym("import", &loc), import_str], span.clone());
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
                        let add =
                            self.list(vec![self.sym("+", &loc), expr.clone(), rhs], span.clone());
                        Ok(self.list(vec![self.sym("assign", &loc), expr, add], span))
                    }
                    PyToken::MinusAssign => {
                        self.advance();
                        let rhs = self.parse_expr()?;
                        self.eat_newlines();
                        let span = expr.span.merge(&rhs.span);
                        let sub =
                            self.list(vec![self.sym("-", &loc), expr.clone(), rhs], span.clone());
                        Ok(self.list(vec![self.sym("assign", &loc), expr, sub], span))
                    }
                    PyToken::StarAssign => {
                        self.advance();
                        let rhs = self.parse_expr()?;
                        self.eat_newlines();
                        let span = expr.span.merge(&rhs.span);
                        let mul =
                            self.list(vec![self.sym("*", &loc), expr.clone(), rhs], span.clone());
                        Ok(self.list(vec![self.sym("assign", &loc), expr, mul], span))
                    }
                    PyToken::SlashAssign => {
                        self.advance();
                        let rhs = self.parse_expr()?;
                        self.eat_newlines();
                        let span = expr.span.merge(&rhs.span);
                        let div =
                            self.list(vec![self.sym("/", &loc), expr.clone(), rhs], span.clone());
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

    fn parse_if(&mut self) -> Result<Syntax, String> {
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

    fn parse_elif(&mut self) -> Result<Syntax, String> {
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

    fn parse_while(&mut self) -> Result<Syntax, String> {
        let loc = self.peek_loc().loc.clone();
        self.advance(); // consume `while`
        let cond = self.parse_expr()?;
        let body = self.parse_block()?;

        let span = self.span_from(&loc);
        Ok(self.list(vec![self.sym("while", &loc), cond, body], span))
    }

    /// `for x in iter:` → `(each x in iter body)`
    fn parse_for(&mut self) -> Result<Syntax, String> {
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
            self.list(name_syms, span.clone())
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
    fn parse_try(&mut self) -> Result<Syntax, String> {
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
            "__py_err".to_string()
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

        let ok_sym = self.sym("__py_ok", &loc);
        let val_sym = self.sym("__py_val", &loc);
        let pattern = Syntax::new(SyntaxKind::Array(vec![ok_sym, val_sym]), span.clone());

        let catch_fn = self.list(
            vec![
                self.sym("fn", &loc),
                self.list(vec![self.sym(&err_name, &loc)], span.clone()),
                catch_body,
            ],
            span.clone(),
        );
        let catch_call = self.list(vec![catch_fn, self.sym("__py_val", &loc)], span.clone());

        let if_expr = self.list(
            vec![
                self.sym("if", &loc),
                self.sym("__py_ok", &loc),
                self.sym("__py_val", &loc),
                catch_call,
            ],
            span.clone(),
        );

        let binding = self.list(vec![pattern, protect_call], span.clone());
        let bindings = self.list(vec![binding], span.clone());

        let mut let_items = vec![self.sym("let", &loc), bindings, if_expr];
        if let Some(fin) = finally_body {
            let inner = self.list(let_items, span.clone());
            let_items = vec![self.sym("begin", &loc), inner, fin];
        }

        Ok(self.list(let_items, span))
    }

    fn parse_function_def(&mut self, loc: &super::token::SourceLoc) -> Result<Syntax, String> {
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
        let param_list = self.list(params, span.clone());
        Ok(self.list(vec![self.sym("fn", loc), param_list, body], span))
    }

    fn parse_params(&mut self) -> Result<Vec<Syntax>, String> {
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
            params.push(self.sym("&", &loc));
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
                params.push(self.sym("&", &loc));
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

    fn parse_expr(&mut self) -> Result<Syntax, String> {
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
    fn parse_pratt(&mut self, min_prec: u8) -> Result<Syntax, String> {
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
            let saved = self.pos;
            let loc = self.peek_loc().loc.clone();
            self.advance();
            if *self.peek() == PyToken::In && min_prec <= 3 {
                self.advance();
                let rhs = self.parse_pratt(4)?;
                let span = lhs.span.merge(&rhs.span);
                let contains = self.list(vec![self.sym("contains?", &loc), rhs, lhs], span.clone());
                lhs = self.list(vec![self.sym("not", &loc), contains], span);
            } else {
                self.pos = saved; // not a `not in`, backtrack
            }
        }

        Ok(lhs)
    }

    fn parse_unary(&mut self) -> Result<Syntax, String> {
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

    fn parse_postfix(&mut self) -> Result<Syntax, String> {
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

    fn parse_call(&mut self, func: Syntax) -> Result<Syntax, String> {
        let args = self.parse_arglist()?;
        let loc = &func.span.clone();
        let span = Span::new(loc.start, loc.end, loc.line, loc.col).with_file(self.file.clone());
        let mut items = vec![func];
        items.extend(args);
        Ok(self.list(items, span))
    }

    fn parse_arglist(&mut self) -> Result<Vec<Syntax>, String> {
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

    fn parse_atom(&mut self) -> Result<Syntax, String> {
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
                        params.push(self.sym("&", &loc));
                        params.push(self.sym(&name, &loc));
                    } else {
                        let name = self.expect_ident()?;
                        params.push(self.sym(&name, &loc));
                        while *self.peek() == PyToken::Comma {
                            self.advance();
                            if *self.peek() == PyToken::Star {
                                self.advance();
                                let name = self.expect_ident()?;
                                params.push(self.sym("&", &loc));
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
    fn build_fstring(
        &mut self,
        parts: Vec<FStringPart>,
        loc: &super::token::SourceLoc,
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

    fn parse_list_literal(&mut self) -> Result<Syntax, String> {
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

    fn parse_dict_literal(&mut self) -> Result<Syntax, String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(input: &str) -> Vec<Syntax> {
        let mut lexer = PyLexer::new(input, "<test>");
        let tokens = lexer.tokenize().expect("lex failed");
        let mut parser = PyParser::new(tokens, "<test>");
        parser.parse_file().expect("parse failed")
    }

    fn parse_one(input: &str) -> Syntax {
        let mut forms = parse(input);
        assert_eq!(forms.len(), 1, "expected 1 form, got {}", forms.len());
        forms.pop().unwrap()
    }

    fn is_def(s: &Syntax, name: &str) -> bool {
        if let SyntaxKind::List(items) = &s.kind {
            items.len() == 3
                && (items[0].is_symbol("def") || items[0].is_symbol("var"))
                && items[1].is_symbol(name)
        } else {
            false
        }
    }

    #[test]
    fn test_assignment() {
        let form = parse_one("x = 42\n");
        assert!(is_def(&form, "x"));
    }

    #[test]
    fn test_function_def() {
        let form = parse_one("def add(a, b):\n  return a + b\n");
        assert!(is_def(&form, "add"));
    }

    #[test]
    fn test_lambda() {
        let form = parse_one("f = lambda x: x + 1\n");
        assert!(is_def(&form, "f"));
        if let SyntaxKind::List(items) = &form.kind {
            if let SyntaxKind::List(fn_items) = &items[2].kind {
                assert!(fn_items[0].is_symbol("fn"));
            } else {
                panic!("expected fn form");
            }
        }
    }

    #[test]
    fn test_if_elif_else() {
        let form = parse_one("if x > 0:\n  y = 1\nelif x < 0:\n  y = -1\nelse:\n  y = 0\n");
        if let SyntaxKind::List(items) = &form.kind {
            assert!(items[0].is_symbol("if"));
            // else branch should be nested if (from elif)
            if let SyntaxKind::List(else_items) = &items[3].kind {
                assert!(else_items[0].is_symbol("if"));
            }
        }
    }

    #[test]
    fn test_while_loop() {
        let form = parse_one("while x > 0:\n  x = x - 1\n");
        if let SyntaxKind::List(items) = &form.kind {
            assert!(items[0].is_symbol("while"));
        } else {
            panic!("expected list");
        }
    }

    #[test]
    fn test_for_loop() {
        let form = parse_one("for x in arr:\n  println(x)\n");
        if let SyntaxKind::List(items) = &form.kind {
            assert!(items[0].is_symbol("each"));
        } else {
            panic!("expected each form");
        }
    }

    #[test]
    fn test_arithmetic() {
        let form = parse_one("x = 1 + 2 * 3\n");
        assert!(is_def(&form, "x"));
    }

    #[test]
    fn test_list_literal() {
        let form = parse_one("a = [1, 2, 3]\n");
        assert!(is_def(&form, "a"));
        if let SyntaxKind::List(items) = &form.kind {
            assert!(matches!(&items[2].kind, SyntaxKind::ArrayMut(elems) if elems.len() == 3));
        }
    }

    #[test]
    fn test_dict_literal() {
        let form = parse_one("d = {\"x\": 1, \"y\": 2}\n");
        assert!(is_def(&form, "d"));
        if let SyntaxKind::List(items) = &form.kind {
            assert!(matches!(&items[2].kind, SyntaxKind::StructMut(elems) if elems.len() == 4));
        }
    }

    #[test]
    fn test_dot_access() {
        let form = parse_one("v = obj.field\n");
        assert!(is_def(&form, "v"));
    }

    #[test]
    fn test_index_access() {
        let form = parse_one("v = arr[0]\n");
        assert!(is_def(&form, "v"));
    }

    #[test]
    fn test_ternary() {
        let form = parse_one("v = 1 if x > 0 else 0\n");
        assert!(is_def(&form, "v"));
        if let SyntaxKind::List(items) = &form.kind {
            if let SyntaxKind::List(if_items) = &items[2].kind {
                assert!(if_items[0].is_symbol("if"));
            }
        }
    }

    #[test]
    fn test_not_equal() {
        let form = parse_one("b = 1 != 2\n");
        assert!(is_def(&form, "b"));
    }

    #[test]
    fn test_rest_params() {
        let form = parse_one("def f(a, *args):\n  return a\n");
        assert!(is_def(&form, "f"));
    }

    #[test]
    fn test_empty_file() {
        let forms = parse("");
        assert!(forms.is_empty());
    }

    #[test]
    fn test_comment_only() {
        let forms = parse("# just a comment\n");
        assert!(forms.is_empty());
    }

    #[test]
    fn test_pass() {
        let form = parse_one("def f():\n  pass\n");
        assert!(is_def(&form, "f"));
    }

    #[test]
    fn test_string_concat() {
        let form = parse_one("s = \"hello\" \"world\"\n");
        assert!(is_def(&form, "s"));
        if let SyntaxKind::List(items) = &form.kind {
            if let SyntaxKind::String(s) = &items[2].kind {
                assert_eq!(s, "helloworld");
            }
        }
    }

    #[test]
    fn test_field_assignment() {
        let form = parse_one("obj.x = 42\n");
        if let SyntaxKind::List(items) = &form.kind {
            assert!(items[0].is_symbol("put"));
        }
    }

    #[test]
    fn test_plus_assign() {
        let form = parse_one("x += 1\n");
        // x is not a new binding, it's an existing var — but since we see it
        // as a compound assignment, we emit (assign x (+ x 1))
        // Actually the parser sees x as an expr, then +=, so it emits assign
        if let SyntaxKind::List(items) = &form.kind {
            assert!(items[0].is_symbol("assign"));
        }
    }

    #[test]
    fn test_boolean_ops() {
        let form = parse_one("v = a and b or not c\n");
        assert!(is_def(&form, "v"));
    }

    #[test]
    fn test_power() {
        let form = parse_one("v = 2 ** 10\n");
        assert!(is_def(&form, "v"));
    }

    #[test]
    fn test_for_with_assign_and_break() {
        let forms = parse("found = None\nfor i in [1, 2, 3]:\n  found = i\n  if i > 1:\n    break\n\nprintln(found)\n");
        // The for body should use `begin` (not `block`) and `assign` (not `var`)
        let for_form = &forms[1];
        if let SyntaxKind::List(items) = &for_form.kind {
            assert!(items[0].is_symbol("each"));
            let body = &items[4];
            if let SyntaxKind::List(body_items) = &body.kind {
                assert!(
                    body_items[0].is_symbol("begin"),
                    "for body should use begin, got {:?}",
                    body_items[0]
                );
                if let SyntaxKind::List(assign_items) = &body_items[1].kind {
                    assert!(
                        assign_items[0].is_symbol("assign"),
                        "should use assign inside for, got {:?}",
                        assign_items[0]
                    );
                }
            }
        }
    }
}
