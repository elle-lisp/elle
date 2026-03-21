//! Recursive-descent + Pratt parser for Lua surface syntax.
//!
//! Parses Lua source into `Vec<Syntax>` — the same trees the s-expression
//! reader produces. The rest of the pipeline (expander → analyzer → lowerer →
//! emitter → VM) is unchanged.

use super::lua_lexer::{LuaLexer, LuaToken, LuaTokenLoc};
use crate::syntax::{Span, Syntax, SyntaxKind};

/// Lua compatibility prelude, compiled into the binary.
/// These definitions are prepended to every .lua file so that
/// Lua standard library functions (math.sqrt, table.insert, etc.) are available.
const LUA_PRELUDE: &str = include_str!("lua_prelude.lisp");

/// Parse a `.lua` file into top-level `Syntax` forms.
/// Automatically prepends the Lua compat prelude definitions.
pub fn parse_lua_file(input: &str, source_name: &str) -> Result<Vec<Syntax>, String> {
    // Strip shebang if present
    let input_clean = if input.starts_with("#!") {
        input.lines().skip(1).collect::<Vec<_>>().join("\n")
    } else {
        input.to_string()
    };

    // Parse the prelude as s-expressions
    let mut prelude_forms = crate::reader::read_syntax_all(LUA_PRELUDE, "<lua-prelude>")?;

    let mut lexer = LuaLexer::new(&input_clean, source_name);
    let tokens = lexer.tokenize()?;
    let mut parser = LuaParser::new(tokens, source_name);
    let user_forms = parser.parse_file()?;

    prelude_forms.extend(user_forms);
    Ok(prelude_forms)
}

struct LuaParser {
    tokens: Vec<LuaTokenLoc>,
    pos: usize,
    file: String,
}

impl LuaParser {
    fn new(tokens: Vec<LuaTokenLoc>, file: &str) -> Self {
        LuaParser {
            tokens,
            pos: 0,
            file: file.to_string(),
        }
    }

    // ── Token navigation ──────────────────────────────────────────────

    fn peek(&self) -> &LuaToken {
        self.tokens
            .get(self.pos)
            .map(|t| &t.token)
            .unwrap_or(&LuaToken::Eof)
    }

    fn peek_loc(&self) -> &LuaTokenLoc {
        static EOF_LOC: std::sync::LazyLock<LuaTokenLoc> =
            std::sync::LazyLock::new(|| LuaTokenLoc {
                token: LuaToken::Eof,
                loc: super::token::SourceLoc::new("<eof>", 0, 0),
                len: 0,
            });
        self.tokens.get(self.pos).unwrap_or(&EOF_LOC)
    }

    fn advance(&mut self) -> &LuaTokenLoc {
        let t = &self.tokens[self.pos];
        self.pos += 1;
        t
    }

    fn expect(&mut self, expected: &LuaToken) -> Result<&LuaTokenLoc, String> {
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
            LuaToken::Ident(name) => {
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

    fn at_block_end(&self) -> bool {
        matches!(
            self.peek(),
            LuaToken::End | LuaToken::Else | LuaToken::ElseIf | LuaToken::Until | LuaToken::Eof
        )
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
        while *self.peek() != LuaToken::Eof {
            // Skip semicolons
            if *self.peek() == LuaToken::Semicolon {
                self.advance();
                continue;
            }
            let stmt = self.parse_top_level_statement()?;
            forms.extend(stmt);
        }
        Ok(forms)
    }

    /// Parse a top-level statement, producing one or more top-level forms.
    /// Lua locals are always mutable, so we emit `(var name value)`.
    /// Top-level `function` also uses `(def name (fn ...))` (immutable).
    fn parse_top_level_statement(&mut self) -> Result<Vec<Syntax>, String> {
        let loc = self.peek_loc().loc.clone();
        match self.peek().clone() {
            // `function foo(params) body end` → (def foo (fn (params) body))
            // `function obj:method(params) body end` → (put obj :method (fn (self params) body))
            LuaToken::Function => {
                self.advance();
                let name = self.expect_ident()?;
                if *self.peek() == LuaToken::Colon {
                    // Method definition: function obj:method(...)
                    // → (put obj :method (fn (self ...) body))
                    self.advance();
                    let method = self.expect_ident()?;
                    let func = self.parse_method_body(&loc)?;
                    let span = func.span.clone();
                    let kw = Syntax::new(SyntaxKind::Keyword(method), self.span_from(&loc));
                    let put = self.list(
                        vec![self.sym("put", &loc), self.sym(&name, &loc), kw, func],
                        span,
                    );
                    Ok(vec![put])
                } else {
                    let func = self.parse_function_body(&loc)?;
                    let span = func.span.clone();
                    let def = self.list(
                        vec![self.sym("def", &loc), self.sym(&name, &loc), func],
                        span,
                    );
                    Ok(vec![def])
                }
            }

            // `local x = expr` → (var x expr) — mutable
            // `local a, b = expr` → (var (a b) expr) — destructuring
            // `local function f(params) body end` → (def f (fn (params) body))
            LuaToken::Local => {
                self.advance();
                if *self.peek() == LuaToken::Function {
                    self.advance();
                    let name = self.expect_ident()?;
                    let func = self.parse_function_body(&loc)?;
                    let span = func.span.clone();
                    let def = self.list(
                        vec![self.sym("def", &loc), self.sym(&name, &loc), func],
                        span,
                    );
                    Ok(vec![def])
                } else {
                    Ok(vec![self.parse_local_binding(&loc)?])
                }
            }

            _ => {
                let expr = self.parse_statement()?;
                Ok(vec![expr])
            }
        }
    }

    // ── Block parsing ─────────────────────────────────────────────────

    /// Parse a block (sequence of statements) until a block-terminating keyword.
    /// `local` bindings are nested as `let` wrapping the rest of the block.
    fn parse_block(&mut self) -> Result<Syntax, String> {
        let mut stmts: Vec<Syntax> = Vec::new();
        let block_loc = self.peek_loc().loc.clone();

        while !self.at_block_end() {
            if *self.peek() == LuaToken::Semicolon {
                self.advance();
                continue;
            }

            let loc = self.peek_loc().loc.clone();
            match self.peek().clone() {
                LuaToken::Local => {
                    self.advance();
                    if *self.peek() == LuaToken::Function {
                        self.advance();
                        let name = self.expect_ident()?;
                        let func = self.parse_function_body(&loc)?;
                        // Emit (def name func) as a statement, then continue block
                        let def = self.list(
                            vec![self.sym("def", &loc), self.sym(&name, &loc), func],
                            self.span_from(&loc),
                        );
                        stmts.push(def);
                    } else {
                        stmts.push(self.parse_local_binding(&loc)?);
                    }
                }

                LuaToken::Return => {
                    self.advance();
                    let val = if self.at_block_end() || *self.peek() == LuaToken::Semicolon {
                        self.nil_syntax(&loc)
                    } else {
                        let first = self.parse_expr()?;
                        if *self.peek() == LuaToken::Comma {
                            // return a, b, c → [a b c]
                            let mut vals = vec![first];
                            while *self.peek() == LuaToken::Comma {
                                self.advance();
                                vals.push(self.parse_expr()?);
                            }
                            Syntax::new(SyntaxKind::Array(vals), self.span_from(&loc))
                        } else {
                            first
                        }
                    };
                    // Optional trailing semicolon after return
                    if *self.peek() == LuaToken::Semicolon {
                        self.advance();
                    }
                    stmts.push(val);
                    // return terminates the block
                    return Ok(self.stmts_to_block(stmts, &block_loc));
                }

                _ => {
                    let stmt = self.parse_statement()?;
                    stmts.push(stmt);
                }
            }
        }

        Ok(self.stmts_to_block(stmts, &block_loc))
    }

    /// Convert a list of statements into a single expression.
    /// Uses `block` (which opens a scope) so that `var`/`def` forms
    /// from Lua `local` are properly scoped.
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
                        let tname = format!("__lua_t{}", i);
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
    fn parse_local_binding(&mut self, loc: &super::token::SourceLoc) -> Result<Syntax, String> {
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

    fn parse_if(&mut self) -> Result<Syntax, String> {
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

    fn parse_while(&mut self) -> Result<Syntax, String> {
        let loc = self.peek_loc().loc.clone();
        self.expect(&LuaToken::While)?;
        let cond = self.parse_expr()?;
        self.expect(&LuaToken::Do)?;
        let body = self.parse_block()?;
        self.expect(&LuaToken::End)?;

        let span = self.span_from(&loc);
        Ok(self.list(vec![self.sym("while", &loc), cond, body], span))
    }

    fn parse_for(&mut self) -> Result<Syntax, String> {
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
    fn parse_for_in(
        &mut self,
        first_name: String,
        loc: &super::token::SourceLoc,
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
    fn parse_repeat(&mut self) -> Result<Syntax, String> {
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

    fn parse_do(&mut self) -> Result<Syntax, String> {
        let loc = self.peek_loc().loc.clone();
        self.expect(&LuaToken::Do)?;
        let body = self.parse_block()?;
        self.expect(&LuaToken::End)?;
        let span = self.span_from(&loc);
        Ok(self.list(vec![self.sym("begin", &loc), body], span))
    }

    /// Like `parse_function_body` but prepends implicit `self` parameter.
    fn parse_method_body(&mut self, loc: &super::token::SourceLoc) -> Result<Syntax, String> {
        self.expect(&LuaToken::LParen)?;
        let mut params = vec![self.sym("self", loc)];
        if *self.peek() != LuaToken::RParen {
            if *self.peek() == LuaToken::DotDotDot {
                self.advance();
                params.push(self.sym("&", loc));
                params.push(self.sym("__varargs", loc));
            } else {
                let name = self.expect_ident()?;
                params.push(self.sym(&name, loc));
                while *self.peek() == LuaToken::Comma {
                    self.advance();
                    if *self.peek() == LuaToken::DotDotDot {
                        self.advance();
                        params.push(self.sym("&", loc));
                        params.push(self.sym("__varargs", loc));
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

    fn parse_function_body(&mut self, loc: &super::token::SourceLoc) -> Result<Syntax, String> {
        self.expect(&LuaToken::LParen)?;
        let mut params = Vec::new();
        if *self.peek() != LuaToken::RParen {
            if *self.peek() == LuaToken::DotDotDot {
                // function(...) — varargs only
                self.advance();
                params.push(self.sym("&", loc));
                params.push(self.sym("__varargs", loc));
            } else {
                let name = self.expect_ident()?;
                params.push(self.sym(&name, loc));
                while *self.peek() == LuaToken::Comma {
                    self.advance();
                    if *self.peek() == LuaToken::DotDotDot {
                        // function(a, b, ...) — named params + varargs
                        self.advance();
                        params.push(self.sym("&", loc));
                        params.push(self.sym("__varargs", loc));
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

    fn parse_expr(&mut self) -> Result<Syntax, String> {
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
    fn parse_pratt(&mut self, min_prec: u8) -> Result<Syntax, String> {
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
                let eq = self.list(vec![self.sym("=", &loc), lhs, rhs], span.clone());
                lhs = self.list(vec![self.sym("not", &loc), eq], span);
            } else {
                lhs = self.list(vec![self.sym(op_name, &loc), lhs, rhs], span);
            }
        }

        Ok(lhs)
    }

    fn parse_unary(&mut self) -> Result<Syntax, String> {
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

    fn parse_postfix(&mut self) -> Result<Syntax, String> {
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
                    let kw = Syntax::new(SyntaxKind::Keyword(field), self.span_from(&loc));
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
                        let kw = Syntax::new(SyntaxKind::Keyword(method), self.span_from(&loc));
                        let getter =
                            self.list(vec![self.sym("get", &loc), expr.clone(), kw], span.clone());
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

    fn parse_call(&mut self, func: Syntax) -> Result<Syntax, String> {
        let args = self.parse_arglist()?;
        let loc = &func.span.clone();
        let span = Span::new(loc.start, loc.end, loc.line, loc.col).with_file(self.file.clone());
        let mut items = vec![func];
        items.extend(args);
        Ok(self.list(items, span))
    }

    fn parse_arglist(&mut self) -> Result<Vec<Syntax>, String> {
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

    fn parse_atom(&mut self) -> Result<Syntax, String> {
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
                    SyntaxKind::String(s),
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
                    SyntaxKind::Symbol(name),
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
                let inner = self.sym("__varargs", &loc);
                let span = self.make_span(&loc, 3);
                Ok(Syntax::new(SyntaxKind::Splice(Box::new(inner)), span))
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

    fn parse_table(&mut self) -> Result<Syntax, String> {
        let loc = self.peek_loc().loc.clone();
        self.expect(&LuaToken::LBrace)?;

        if *self.peek() == LuaToken::RBrace {
            self.advance();
            // Empty table → empty mutable struct (works as Lua "object")
            return Ok(Syntax::new(
                SyntaxKind::StructMut(Vec::new()),
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

    fn looks_like_struct_entry(&self) -> bool {
        // Check if current token is Ident and next is Assign
        if let Some(t1) = self.tokens.get(self.pos) {
            if let LuaToken::Ident(_) = &t1.token {
                if let Some(t2) = self.tokens.get(self.pos + 1) {
                    return t2.token == LuaToken::Assign;
                }
            }
        }
        false
    }

    fn parse_struct_table(&mut self, loc: &super::token::SourceLoc) -> Result<Syntax, String> {
        let mut elements = Vec::new();
        loop {
            if *self.peek() == LuaToken::RBrace {
                break;
            }
            let key = self.expect_ident()?;
            self.expect(&LuaToken::Assign)?;
            let value = self.parse_expr()?;
            elements.push(Syntax::new(SyntaxKind::Keyword(key), self.span_from(loc)));
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
            SyntaxKind::StructMut(elements),
            self.span_from(loc),
        ))
    }

    fn parse_array_table(&mut self, loc: &super::token::SourceLoc) -> Result<Syntax, String> {
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
            SyntaxKind::ArrayMut(elements),
            self.span_from(loc),
        ))
    }

    // ── Backtick s-expr escape ────────────────────────────────────────

    fn parse_sexpr_escape(&mut self) -> Result<Syntax, String> {
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
        let syntaxes = crate::reader::read_syntax(&sexpr_text, &self.file)?;
        Ok(syntaxes)
    }

    fn token_to_text(&self) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Parse without the prelude (for unit-testing the parser itself)
    fn parse(input: &str) -> Vec<Syntax> {
        let mut lexer = LuaLexer::new(input, "<test>");
        let tokens = lexer.tokenize().expect("lex failed");
        let mut parser = LuaParser::new(tokens, "<test>");
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
    fn test_local_binding() {
        let form = parse_one("local x = 42");
        assert!(is_def(&form, "x"));
    }

    #[test]
    fn test_function_def() {
        let form = parse_one("function add(a, b) return a + b end");
        assert!(is_def(&form, "add"));
    }

    #[test]
    fn test_local_function() {
        let form = parse_one("local function f(x) return x end");
        assert!(is_def(&form, "f"));
    }

    #[test]
    fn test_arithmetic() {
        let forms = parse("local x = 1 + 2 * 3");
        let form = &forms[0];
        // (def x (+ 1 (* 2 3)))
        assert!(is_def(form, "x"));
    }

    #[test]
    fn test_if_elseif_else() {
        let forms = parse("if true then return 1 elseif false then return 2 else return 3 end");
        assert_eq!(forms.len(), 1);
        // Should be (if true 1 (if false 2 3))
        if let SyntaxKind::List(items) = &forms[0].kind {
            assert!(items[0].is_symbol("if"));
        } else {
            panic!("expected list");
        }
    }

    #[test]
    fn test_while_loop() {
        let form = parse_one("while true do break end");
        if let SyntaxKind::List(items) = &form.kind {
            assert!(items[0].is_symbol("while"));
        } else {
            panic!("expected list");
        }
    }

    #[test]
    fn test_table_array() {
        let form = parse_one("local t = {1, 2, 3}");
        assert!(is_def(&form, "t"));
        // The value should be ArrayMut
        if let SyntaxKind::List(items) = &form.kind {
            assert!(matches!(&items[2].kind, SyntaxKind::ArrayMut(elems) if elems.len() == 3));
        }
    }

    #[test]
    fn test_table_struct() {
        let form = parse_one("local t = {x = 1, y = 2}");
        assert!(is_def(&form, "t"));
        if let SyntaxKind::List(items) = &form.kind {
            assert!(matches!(&items[2].kind, SyntaxKind::StructMut(elems) if elems.len() == 4));
        }
    }

    #[test]
    fn test_string_concat() {
        let form = parse_one("local s = \"hello\" .. \" world\"");
        assert!(is_def(&form, "s"));
        // Should be (var s (string "hello" " world"))
        if let SyntaxKind::List(items) = &form.kind {
            if let SyntaxKind::List(op_items) = &items[2].kind {
                assert!(op_items[0].is_symbol("string"));
            }
        }
    }

    #[test]
    fn test_neq() {
        let form = parse_one("local b = 1 ~= 2");
        assert!(is_def(&form, "b"));
        // Should be (def b (not (= 1 2)))
    }

    #[test]
    fn test_field_access() {
        let forms = parse("local v = t.foo");
        let form = &forms[0];
        // (def v (get t :foo))
        assert!(is_def(form, "v"));
    }

    #[test]
    fn test_for_loop() {
        let form = parse_one("for i = 1, 10 do print(i) end");
        // Should desugar to let + var + while
        if let SyntaxKind::List(items) = &form.kind {
            assert!(items[0].is_symbol("let"));
        } else {
            panic!("expected let form");
        }
    }

    #[test]
    fn test_empty_file() {
        let forms = parse("");
        assert!(forms.is_empty());
    }

    #[test]
    fn test_comment_only() {
        let forms = parse("-- just a comment\n");
        assert!(forms.is_empty());
    }
}
