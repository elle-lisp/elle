//! Recursive-descent + Pratt parser for JavaScript surface syntax.
//!
//! Parses JavaScript source into `Vec<Syntax>` — the same trees the
//! s-expression reader produces.  The rest of the pipeline (expander →
//! analyzer → lowerer → emitter → VM) is unchanged.

use super::cursor::TokenCursor;
use super::js_lexer::{JsLexer, JsToken, JsTokenLoc};
use super::nav::{Located, Nav, EOF_FILE};
use super::synbuild::{SynBuild, REST_PARAM};
use super::token::SourceLoc;
use crate::syntax::{Span, Syntax, SyntaxKind};

/// Synthetic locals the `try`/`catch` desugaring introduces: a success flag and
/// the carried value. Named once so the bindings and their uses can't drift.
const TRY_OK: &str = "__js_ok";
const TRY_VAL: &str = "__js_val";

/// The `Eof` sentinel returned by `peek_loc`/`advance` once the cursor is past
/// the last token.
static JS_EOF_LOC: std::sync::LazyLock<JsTokenLoc> =
    std::sync::LazyLock::new(|| JsTokenLoc::new(JsToken::Eof, SourceLoc::new(EOF_FILE, 0, 0), 0));

impl Located for JsTokenLoc {
    type Tok = JsToken;
    fn token(&self) -> &JsToken {
        &self.token
    }
    fn loc(&self) -> &SourceLoc {
        &self.loc
    }
    fn eof() -> &'static Self {
        &JS_EOF_LOC
    }
    fn eof_token() -> &'static JsToken {
        &JsToken::Eof
    }
}

impl Nav for JsParser {
    type Loc = JsTokenLoc;
    fn cursor(&self) -> &TokenCursor<JsTokenLoc> {
        &self.cursor
    }
    fn cursor_mut(&mut self) -> &mut TokenCursor<JsTokenLoc> {
        &mut self.cursor
    }
}

impl SynBuild for JsParser {}

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
    cursor: TokenCursor<JsTokenLoc>,
    file: String,
}

impl JsParser {
    fn new(tokens: Vec<JsTokenLoc>, file: &str) -> Self {
        JsParser {
            cursor: TokenCursor::new(tokens),
            file: file.to_string(),
        }
    }

    // ── Token navigation ──────────────────────────────────────────────
    // peek / peek_loc / advance / expect come from the shared `Nav` trait;
    // only the token-specific helpers below live here.

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

    // make_span / span_from / sym / list / nil_syntax / stmts_to_block come
    // from the shared `SynBuild` trait.

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

    // ── Statement parsing ─────────────────────────────────────────────
}

mod expr;
mod stmt;
#[cfg(test)]
mod tests;
