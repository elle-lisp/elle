//! Recursive-descent + Pratt parser for JavaScript surface syntax.
//!
//! Parses JavaScript source into `Vec<Syntax>` — the same trees the
//! s-expression reader produces.  The rest of the pipeline (expander →
//! analyzer → lowerer → emitter → VM) is unchanged.

use super::js_lexer::{JsLexer, JsToken, JsTokenLoc};
use crate::syntax::{Span, Syntax, SyntaxKind};

/// Parse a `.js` file into top-level `Syntax` forms.
pub fn parse_js_file(input: &str, source_name: &str) -> Result<Vec<Syntax>, String> {
    // Strip shebang if present
    let input_clean = if input.starts_with("#!") {
        input.lines().skip(1).collect::<Vec<_>>().join("\n")
    } else {
        input.to_string()
    };

    let mut lexer = JsLexer::new(&input_clean, source_name);
    let tokens = lexer.tokenize()?;
    let mut parser = JsParser::new(tokens, source_name);
    parser.parse_file()
}

struct JsParser {
    tokens: Vec<JsTokenLoc>,
    pos: usize,
    file: String,
}

impl JsParser {
    fn new(tokens: Vec<JsTokenLoc>, file: &str) -> Self {
        JsParser {
            tokens,
            pos: 0,
            file: file.to_string(),
        }
    }

    // ── Token navigation ──────────────────────────────────────────────

    fn peek(&self) -> &JsToken {
        self.tokens
            .get(self.pos)
            .map(|t| &t.token)
            .unwrap_or(&JsToken::Eof)
    }

    fn peek_loc(&self) -> &JsTokenLoc {
        static EOF_LOC: std::sync::LazyLock<JsTokenLoc> = std::sync::LazyLock::new(|| JsTokenLoc {
            token: JsToken::Eof,
            loc: super::token::SourceLoc::new("<eof>", 0, 0),
            len: 0,
        });
        self.tokens.get(self.pos).unwrap_or(&EOF_LOC)
    }

    fn advance(&mut self) -> &JsTokenLoc {
        let t = &self.tokens[self.pos];
        self.pos += 1;
        t
    }

    fn expect(&mut self, expected: &JsToken) -> Result<&JsTokenLoc, String> {
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
            JsToken::Ident(name) => {
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

    /// Skip an optional semicolon (JS semicolons are mostly optional in
    /// our subset, but we accept them when present).
    fn eat_semicolon(&mut self) {
        if *self.peek() == JsToken::Semicolon {
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
        let mut forms = Vec::new();
        while *self.peek() != JsToken::Eof {
            if *self.peek() == JsToken::Semicolon {
                self.advance();
                continue;
            }
            let stmt = self.parse_top_level_statement()?;
            forms.extend(stmt);
        }
        Ok(forms)
    }

    /// Parse a top-level statement, producing one or more top-level forms.
    /// - `const name = expr` → `(def name expr)`
    /// - `let name = expr` → `(var name expr)`
    /// - `function name(params) { body }` → `(def name (fn (params) body))`
    fn parse_top_level_statement(&mut self) -> Result<Vec<Syntax>, String> {
        let loc = self.peek_loc().loc.clone();
        match self.peek().clone() {
            JsToken::Function => {
                self.advance();
                let name = self.expect_ident()?;
                let func = self.parse_function_body(&loc)?;
                let span = func.span.clone();
                let def = self.list(
                    vec![self.sym("def", &loc), self.sym(&name, &loc), func],
                    span,
                );
                Ok(vec![def])
            }

            JsToken::Const => {
                self.advance();
                let form = self.parse_binding("def", &loc)?;
                self.eat_semicolon();
                Ok(vec![form])
            }

            JsToken::Let | JsToken::Var => {
                self.advance();
                let form = self.parse_binding("var", &loc)?;
                self.eat_semicolon();
                Ok(vec![form])
            }

            _ => {
                let expr = self.parse_statement()?;
                Ok(vec![expr])
            }
        }
    }

    // ── Block parsing ─────────────────────────────────────────────────

    /// Parse a brace-delimited block `{ ... }` into a single expression.
    fn parse_brace_block(&mut self) -> Result<Syntax, String> {
        let loc = self.peek_loc().loc.clone();
        self.expect(&JsToken::LBrace)?;
        let mut stmts: Vec<Syntax> = Vec::new();

        while *self.peek() != JsToken::RBrace && *self.peek() != JsToken::Eof {
            if *self.peek() == JsToken::Semicolon {
                self.advance();
                continue;
            }

            let loc_inner = self.peek_loc().loc.clone();
            match self.peek().clone() {
                JsToken::Const => {
                    self.advance();
                    let binding = self.parse_binding("def", &loc_inner)?;
                    self.eat_semicolon();
                    stmts.push(binding);
                }

                JsToken::Let | JsToken::Var => {
                    self.advance();
                    let binding = self.parse_binding("var", &loc_inner)?;
                    self.eat_semicolon();
                    stmts.push(binding);
                }

                JsToken::Return => {
                    self.advance();
                    let val =
                        if *self.peek() == JsToken::Semicolon || *self.peek() == JsToken::RBrace {
                            self.nil_syntax(&loc_inner)
                        } else {
                            self.parse_expr()?
                        };
                    self.eat_semicolon();
                    stmts.push(val);
                    // return terminates the block
                    return self.finish_block(stmts, &loc);
                }

                _ => {
                    let stmt = self.parse_statement()?;
                    stmts.push(stmt);
                }
            }
        }
        self.expect(&JsToken::RBrace)?;
        Ok(self.stmts_to_block(stmts, &loc))
    }

    /// Drain remaining statements in a block after a return.
    fn finish_block(
        &mut self,
        stmts: Vec<Syntax>,
        loc: &super::token::SourceLoc,
    ) -> Result<Syntax, String> {
        // Skip any remaining statements until closing brace
        let mut depth = 1u32;
        while depth > 0 && *self.peek() != JsToken::Eof {
            match self.peek() {
                JsToken::LBrace => {
                    depth += 1;
                    self.advance();
                }
                JsToken::RBrace => {
                    depth -= 1;
                    self.advance();
                }
                _ => {
                    self.advance();
                }
            }
        }
        Ok(self.stmts_to_block(stmts, loc))
    }

    fn stmts_to_block(&self, mut stmts: Vec<Syntax>, loc: &super::token::SourceLoc) -> Syntax {
        match stmts.len() {
            0 => self.nil_syntax(loc),
            1 => stmts.pop().unwrap(),
            _ => {
                let has_locals = stmts.iter().any(|s| {
                    matches!(&s.kind, SyntaxKind::List(items) if !items.is_empty()
                        && (items[0].is_symbol("var") || items[0].is_symbol("def")))
                });
                let head = if has_locals { "block" } else { "begin" };
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
    fn parse_binding(
        &mut self,
        bind_kind: &str,
        loc: &super::token::SourceLoc,
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
    fn parse_if(&mut self) -> Result<Syntax, String> {
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
    fn parse_while(&mut self) -> Result<Syntax, String> {
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
    fn parse_for(&mut self) -> Result<Syntax, String> {
        let loc = self.peek_loc().loc.clone();
        self.advance(); // consume `for`
        self.expect(&JsToken::LParen)?;

        // Check if this is for...of or C-style for
        match self.peek().clone() {
            JsToken::Const | JsToken::Let | JsToken::Var => {
                let saved_pos = self.pos;
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
                self.pos = saved_pos;
                self.parse_c_style_for(&loc)
            }
            _ => self.parse_c_style_for(&loc),
        }
    }

    /// Parse C-style `for (init; cond; update) { body }`
    /// Desugar to: (block init (while cond (begin body update)))
    fn parse_c_style_for(&mut self, loc: &super::token::SourceLoc) -> Result<Syntax, String> {
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
    fn parse_update_expr(&mut self) -> Result<Syntax, String> {
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
    fn parse_do_while(&mut self) -> Result<Syntax, String> {
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
    fn parse_try(&mut self) -> Result<Syntax, String> {
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
        let ok_sym = self.sym("__js_ok", &loc);
        let val_sym = self.sym("__js_val", &loc);
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
        let catch_call = self.list(vec![catch_fn, self.sym("__js_val", &loc)], span.clone());

        // Build if expression
        let if_expr = self.list(
            vec![
                self.sym("if", &loc),
                self.sym("__js_ok", &loc),
                self.sym("__js_val", &loc),
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

    fn parse_function_body(&mut self, loc: &super::token::SourceLoc) -> Result<Syntax, String> {
        self.expect(&JsToken::LParen)?;
        let params = self.parse_params()?;
        self.expect(&JsToken::RParen)?;
        let body = self.parse_brace_block()?;

        let span = self.span_from(loc);
        let param_list = self.list(params, span.clone());
        Ok(self.list(vec![self.sym("fn", loc), param_list, body], span))
    }

    fn parse_params(&mut self) -> Result<Vec<Syntax>, String> {
        let loc = self.peek_loc().loc.clone();
        let mut params = Vec::new();
        if *self.peek() == JsToken::RParen {
            return Ok(params);
        }

        // Check for rest parameter
        if *self.peek() == JsToken::DotDotDot {
            self.advance();
            let name = self.expect_ident()?;
            params.push(self.sym("&", &loc));
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
                params.push(self.sym("&", &loc));
                params.push(self.sym(&name, &loc));
                break;
            }
            let name = self.expect_ident()?;
            params.push(self.sym(&name, &loc));
        }

        Ok(params)
    }

    // ── Expression parsing (Pratt) ────────────────────────────────────

    fn parse_expr(&mut self) -> Result<Syntax, String> {
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
    fn parse_pratt(&mut self, min_prec: u8) -> Result<Syntax, String> {
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
                let eq = self.list(vec![self.sym("=", &loc), lhs, rhs], span.clone());
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

    fn parse_unary(&mut self) -> Result<Syntax, String> {
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
                        operand.clone(),
                        Syntax::new(SyntaxKind::Int(1), span.clone()),
                    ],
                    span.clone(),
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
                        operand.clone(),
                        Syntax::new(SyntaxKind::Int(1), span.clone()),
                    ],
                    span.clone(),
                );
                Ok(self.list(vec![self.sym("assign", &loc), operand, sub], span))
            }
            _ => self.parse_postfix(),
        }
    }

    fn parse_postfix(&mut self) -> Result<Syntax, String> {
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
                    let kw = Syntax::new(SyntaxKind::Keyword(field), self.span_from(&loc));
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
                            expr.clone(),
                            Syntax::new(SyntaxKind::Int(1), span.clone()),
                        ],
                        span.clone(),
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
                            expr.clone(),
                            Syntax::new(SyntaxKind::Int(1), span.clone()),
                        ],
                        span.clone(),
                    );
                    expr = self.list(vec![self.sym("assign", &loc), expr, sub], span);
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
        self.expect(&JsToken::LParen)?;
        let mut args = Vec::new();
        if *self.peek() != JsToken::RParen {
            // Handle spread: ...expr → (splice expr)
            if *self.peek() == JsToken::DotDotDot {
                let loc = self.peek_loc().loc.clone();
                self.advance();
                let expr = self.parse_expr()?;
                let span = self.span_from(&loc);
                args.push(Syntax::new(SyntaxKind::Splice(Box::new(expr)), span));
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
                    args.push(Syntax::new(SyntaxKind::Splice(Box::new(expr)), span));
                } else {
                    args.push(self.parse_expr()?);
                }
            }
        }
        self.expect(&JsToken::RParen)?;
        Ok(args)
    }

    fn parse_atom(&mut self) -> Result<Syntax, String> {
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
                    SyntaxKind::String(s),
                    self.make_span(&loc, len),
                ))
            }
            JsToken::TemplateNoSub(s) => {
                self.advance();
                Ok(Syntax::new(
                    SyntaxKind::String(s),
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
                    SyntaxKind::Symbol(name),
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
                Ok(Syntax::new(SyntaxKind::Splice(Box::new(expr)), span))
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
    fn is_arrow_params(&self) -> bool {
        if self.tokens.get(self.pos).map(|t| &t.token) != Some(&JsToken::LParen) {
            return false;
        }
        let mut depth = 1u32;
        let mut i = self.pos + 1;
        while i < self.tokens.len() && depth > 0 {
            match &self.tokens[i].token {
                JsToken::LParen => depth += 1,
                JsToken::RParen => depth -= 1,
                JsToken::Eof => return false,
                _ => {}
            }
            i += 1;
        }
        // i now points to token after the matching `)`
        matches!(self.tokens.get(i).map(|t| &t.token), Some(JsToken::Arrow))
    }

    fn parse_arrow_function(&mut self, loc: &super::token::SourceLoc) -> Result<Syntax, String> {
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

    fn parse_arrow_body(
        &mut self,
        param_names: &[String],
        loc: &super::token::SourceLoc,
    ) -> Result<Syntax, String> {
        let body = if *self.peek() == JsToken::LBrace {
            self.parse_brace_block()?
        } else {
            self.parse_expr()?
        };

        let span = self.span_from(loc);
        let params: Vec<Syntax> = param_names.iter().map(|n| self.sym(n, loc)).collect();
        let param_list = self.list(params, span.clone());
        Ok(self.list(vec![self.sym("fn", loc), param_list, body], span))
    }

    /// Parse template literal interpolation.
    /// `hello ${expr} world` → `(string "hello " expr " world")`
    fn parse_template_expr(
        &mut self,
        head: String,
        loc: &super::token::SourceLoc,
    ) -> Result<Syntax, String> {
        let span = self.span_from(loc);
        let mut parts: Vec<Syntax> = vec![self.sym("string", loc)];
        if !head.is_empty() {
            parts.push(Syntax::new(SyntaxKind::String(head), span.clone()));
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
                        parts.push(Syntax::new(SyntaxKind::String(tail), span.clone()));
                    }
                    break;
                }
                JsToken::TemplateMiddle(mid) => {
                    self.advance();
                    if !mid.is_empty() {
                        parts.push(Syntax::new(SyntaxKind::String(mid), span.clone()));
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

    fn parse_array_literal(&mut self) -> Result<Syntax, String> {
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
                    SyntaxKind::Splice(Box::new(expr)),
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
            SyntaxKind::ArrayMut(elements),
            self.span_from(&loc),
        ))
    }

    fn parse_object_literal(&mut self) -> Result<Syntax, String> {
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
                            elements
                                .push(Syntax::new(SyntaxKind::Keyword(name), self.span_from(&loc)));
                            elements.push(value);
                        } else if *self.peek() == JsToken::LParen {
                            // Method shorthand: name(params) { body }
                            let func = self.parse_function_body(&loc)?;
                            elements
                                .push(Syntax::new(SyntaxKind::Keyword(name), self.span_from(&loc)));
                            elements.push(func);
                        } else {
                            // Shorthand: {x} → {:x x}
                            elements.push(Syntax::new(
                                SyntaxKind::Keyword(name.clone()),
                                self.span_from(&loc),
                            ));
                            elements
                                .push(Syntax::new(SyntaxKind::Symbol(name), self.span_from(&loc)));
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
                elements.push(Syntax::new(SyntaxKind::Keyword(key), self.span_from(&loc)));
                elements.push(value);
            }

            if *self.peek() == JsToken::Comma {
                self.advance();
            }
        }
        self.expect(&JsToken::RBrace)?;
        // JS objects are mutable → @struct
        Ok(Syntax::new(
            SyntaxKind::StructMut(elements),
            self.span_from(&loc),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Parse without prelude (for unit-testing the parser itself)
    fn parse(input: &str) -> Vec<Syntax> {
        let mut lexer = JsLexer::new(input, "<test>");
        let tokens = lexer.tokenize().expect("lex failed");
        let mut parser = JsParser::new(tokens, "<test>");
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
    fn test_const_binding() {
        let form = parse_one("const x = 42;");
        assert!(is_def(&form, "x"));
    }

    #[test]
    fn test_let_binding() {
        let form = parse_one("let x = 42;");
        assert!(is_def(&form, "x"));
        if let SyntaxKind::List(items) = &form.kind {
            assert!(items[0].is_symbol("var"));
        }
    }

    #[test]
    fn test_function_def() {
        let form = parse_one("function add(a, b) { return a + b; }");
        assert!(is_def(&form, "add"));
    }

    #[test]
    fn test_arrow_function() {
        let form = parse_one("const f = (x) => x + 1;");
        assert!(is_def(&form, "f"));
        // The value should be (fn (x) (+ x 1))
        if let SyntaxKind::List(items) = &form.kind {
            if let SyntaxKind::List(fn_items) = &items[2].kind {
                assert!(fn_items[0].is_symbol("fn"));
            } else {
                panic!("expected fn form");
            }
        }
    }

    #[test]
    fn test_arrow_single_param() {
        let form = parse_one("const f = x => x + 1;");
        assert!(is_def(&form, "f"));
    }

    #[test]
    fn test_arrow_body_block() {
        let form = parse_one("const f = (x) => { return x + 1; };");
        assert!(is_def(&form, "f"));
    }

    #[test]
    fn test_if_else() {
        let form = parse_one("if (x > 0) { return 1; } else { return 0; }");
        if let SyntaxKind::List(items) = &form.kind {
            assert!(items[0].is_symbol("if"));
        } else {
            panic!("expected list");
        }
    }

    #[test]
    fn test_if_else_if() {
        let form =
            parse_one("if (x > 0) { return 1; } else if (x < 0) { return -1; } else { return 0; }");
        if let SyntaxKind::List(items) = &form.kind {
            assert!(items[0].is_symbol("if"));
            // else branch should be nested if
            if let SyntaxKind::List(else_items) = &items[3].kind {
                assert!(else_items[0].is_symbol("if"));
            }
        }
    }

    #[test]
    fn test_while_loop() {
        let form = parse_one("while (x > 0) { x = x - 1; }");
        if let SyntaxKind::List(items) = &form.kind {
            assert!(items[0].is_symbol("while"));
        } else {
            panic!("expected list");
        }
    }

    #[test]
    fn test_for_of() {
        let form = parse_one("for (const x of arr) { println(x); }");
        if let SyntaxKind::List(items) = &form.kind {
            assert!(items[0].is_symbol("each"));
        } else {
            panic!("expected each form");
        }
    }

    #[test]
    fn test_for_c_style() {
        let form = parse_one("for (let i = 0; i < 10; i++) { println(i); }");
        if let SyntaxKind::List(items) = &form.kind {
            assert!(items[0].is_symbol("block"));
        } else {
            panic!("expected block form");
        }
    }

    #[test]
    fn test_arithmetic() {
        let form = parse_one("const x = 1 + 2 * 3;");
        assert!(is_def(&form, "x"));
    }

    #[test]
    fn test_array_literal() {
        let form = parse_one("const a = [1, 2, 3];");
        assert!(is_def(&form, "a"));
        if let SyntaxKind::List(items) = &form.kind {
            assert!(matches!(&items[2].kind, SyntaxKind::ArrayMut(elems) if elems.len() == 3));
        }
    }

    #[test]
    fn test_object_literal() {
        let form = parse_one("const o = {x: 1, y: 2};");
        assert!(is_def(&form, "o"));
        if let SyntaxKind::List(items) = &form.kind {
            assert!(matches!(&items[2].kind, SyntaxKind::StructMut(elems) if elems.len() == 4));
        }
    }

    #[test]
    fn test_ternary() {
        let form = parse_one("const v = x > 0 ? 1 : 0;");
        assert!(is_def(&form, "v"));
        // Value should be (if (> x 0) 1 0)
        if let SyntaxKind::List(items) = &form.kind {
            if let SyntaxKind::List(if_items) = &items[2].kind {
                assert!(if_items[0].is_symbol("if"));
            }
        }
    }

    #[test]
    fn test_dot_access() {
        let form = parse_one("const v = obj.field;");
        assert!(is_def(&form, "v"));
    }

    #[test]
    fn test_index_access() {
        let form = parse_one("const v = arr[0];");
        assert!(is_def(&form, "v"));
    }

    #[test]
    fn test_method_call() {
        let form = parse_one("obj.method(1, 2);");
        // Should be ((get obj :method) 1 2)
        if let SyntaxKind::List(items) = &form.kind {
            if let SyntaxKind::List(getter) = &items[0].kind {
                assert!(getter[0].is_symbol("get"));
            }
        }
    }

    #[test]
    fn test_template_literal() {
        let form = parse_one("const s = `hello ${name}!`;");
        assert!(is_def(&form, "s"));
        // Value should be (string "hello " name "!")
        if let SyntaxKind::List(items) = &form.kind {
            if let SyntaxKind::List(str_items) = &items[2].kind {
                assert!(str_items[0].is_symbol("string"));
            }
        }
    }

    #[test]
    fn test_strict_equality() {
        let forms = parse("const b = 1 === 2;");
        let form = &forms[0];
        assert!(is_def(form, "b"));
    }

    #[test]
    fn test_not_equal() {
        let form = parse_one("const b = 1 !== 2;");
        assert!(is_def(&form, "b"));
        // Should be (def b (not (= 1 2)))
        if let SyntaxKind::List(items) = &form.kind {
            if let SyntaxKind::List(not_items) = &items[2].kind {
                assert!(not_items[0].is_symbol("not"));
            }
        }
    }

    #[test]
    fn test_destructuring_array() {
        let form = parse_one("const [a, b] = pair;");
        if let SyntaxKind::List(items) = &form.kind {
            assert!(items[0].is_symbol("def"));
            assert!(matches!(&items[1].kind, SyntaxKind::Array(_)));
        }
    }

    #[test]
    fn test_rest_params() {
        let form = parse_one("function f(a, ...rest) { return rest; }");
        assert!(is_def(&form, "f"));
    }

    #[test]
    fn test_spread_args() {
        let form = parse_one("f(...args);");
        if let SyntaxKind::List(items) = &form.kind {
            assert!(items[0].is_symbol("f"));
            assert!(matches!(&items[1].kind, SyntaxKind::Splice(_)));
        }
    }

    #[test]
    fn test_empty_file() {
        let forms = parse("");
        assert!(forms.is_empty());
    }

    #[test]
    fn test_comment_only() {
        let forms = parse("// just a comment\n");
        assert!(forms.is_empty());
    }

    #[test]
    fn test_shorthand_object() {
        let form = parse_one("const o = {x, y};");
        assert!(is_def(&form, "o"));
        if let SyntaxKind::List(items) = &form.kind {
            if let SyntaxKind::Struct(elems) = &items[2].kind {
                assert_eq!(elems.len(), 4); // :x x :y y
            }
        }
    }

    #[test]
    fn test_assignment() {
        let form = parse_one("x = 42;");
        if let SyntaxKind::List(items) = &form.kind {
            assert!(items[0].is_symbol("assign"));
        }
    }

    #[test]
    fn test_field_assignment() {
        let form = parse_one("obj.x = 42;");
        if let SyntaxKind::List(items) = &form.kind {
            assert!(items[0].is_symbol("put"));
        }
    }

    #[test]
    fn test_plus_assign() {
        let form = parse_one("x += 1;");
        if let SyntaxKind::List(items) = &form.kind {
            assert!(items[0].is_symbol("assign"));
        }
    }
}
