//! Recursive-descent + Pratt parser for Lua surface syntax.
//!
//! Parses Lua source into `Vec<Syntax>` — the same trees the s-expression
//! reader produces. The rest of the pipeline (expander → analyzer → lowerer →
//! emitter → VM) is unchanged.

use super::cursor::TokenCursor;
use super::lua_lexer::{LuaLexer, LuaToken, LuaTokenLoc};
use super::nav::{Located, Nav, EOF_FILE};
use super::synbuild::{SynBuild, REST_PARAM};
use super::token::SourceLoc;
use crate::syntax::{Span, Syntax, SyntaxKind};

/// Synthetic name for the collected variadic-args list in a `function(...)`
/// body. One spelling, shared by the expression and statement parsers.
const VARARGS: &str = "__varargs";
/// Prefix for the synthetic temporaries multiple-assignment desugaring
/// introduces; suffixed with an index (`__lua_t0`, `__lua_t1`, …).
const TEMP_PREFIX: &str = "__lua_t";

/// The `Eof` sentinel returned by `peek_loc`/`advance` once the cursor is past
/// the last token.
static LUA_EOF_LOC: std::sync::LazyLock<LuaTokenLoc> =
    std::sync::LazyLock::new(|| LuaTokenLoc::new(LuaToken::Eof, SourceLoc::new(EOF_FILE, 0, 0), 0));

impl Located for LuaTokenLoc {
    type Tok = LuaToken;
    fn token(&self) -> &LuaToken {
        &self.token
    }
    fn loc(&self) -> &SourceLoc {
        &self.loc
    }
    fn eof() -> &'static Self {
        &LUA_EOF_LOC
    }
    fn eof_token() -> &'static LuaToken {
        &LuaToken::Eof
    }
}

impl Nav for LuaParser {
    type Loc = LuaTokenLoc;
    fn cursor(&self) -> &TokenCursor<LuaTokenLoc> {
        &self.cursor
    }
    fn cursor_mut(&mut self) -> &mut TokenCursor<LuaTokenLoc> {
        &mut self.cursor
    }
}

impl SynBuild for LuaParser {}

/// Lua compatibility prelude, compiled into the binary.
/// These definitions are prepended to every .lua file so that
/// Lua standard library functions (math.sqrt, table.insert, etc.) are available.
const LUA_PRELUDE: &str = include_str!("../lua_prelude.lisp");

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
    cursor: TokenCursor<LuaTokenLoc>,
    file: String,
}

impl LuaParser {
    fn new(tokens: Vec<LuaTokenLoc>, file: &str) -> Self {
        LuaParser {
            cursor: TokenCursor::new(tokens),
            file: file.to_string(),
        }
    }

    // ── Token navigation ──────────────────────────────────────────────
    // peek / peek_loc / advance / expect come from the shared `Nav` trait;
    // only the token-specific helpers below live here.

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

    // make_span / span_from / sym / list / nil_syntax / stmts_to_block come
    // from the shared `SynBuild` trait.

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

    // stmts_to_block (fold a block into block/begin, scoping var/def locals)
    // comes from the shared `SynBuild` trait.

    // ── Statement parsing ─────────────────────────────────────────────
}

mod expr;
mod stmt;
#[cfg(test)]
mod tests;
