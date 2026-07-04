//! Recursive-descent + Pratt parser for Python surface syntax.
//!
//! Parses Python source into `Vec<Syntax>` — the same trees the
//! s-expression reader produces.  The rest of the pipeline (expander →
//! analyzer → lowerer → emitter → VM) is unchanged.

use super::cursor::TokenCursor;
use super::nav::{Located, Nav, EOF_FILE};
use super::py_lexer::{FStringPart, PyLexer, PyToken, PyTokenLoc};
use super::synbuild::{SynBuild, REST_PARAM};
use super::token::SourceLoc;
use crate::syntax::{Span, Syntax, SyntaxKind};

/// Synthetic locals the `try`/`except` desugaring introduces: a success flag,
/// the carried value, and the default exception binding when `except` names
/// none. Named once so the bindings and their uses can't drift.
const TRY_OK: &str = "__py_ok";
const TRY_VAL: &str = "__py_val";
const DEFAULT_EXC: &str = "__py_err";

/// The `Eof` sentinel returned by `peek_loc`/`advance` once the cursor is past
/// the last token.
static PY_EOF_LOC: std::sync::LazyLock<PyTokenLoc> =
    std::sync::LazyLock::new(|| PyTokenLoc::new(PyToken::Eof, SourceLoc::new(EOF_FILE, 0, 0), 0));

impl Located for PyTokenLoc {
    type Tok = PyToken;
    fn token(&self) -> &PyToken {
        &self.token
    }
    fn loc(&self) -> &SourceLoc {
        &self.loc
    }
    fn eof() -> &'static Self {
        &PY_EOF_LOC
    }
    fn eof_token() -> &'static PyToken {
        &PyToken::Eof
    }
}

impl Nav for PyParser {
    type Loc = PyTokenLoc;
    fn cursor(&self) -> &TokenCursor<PyTokenLoc> {
        &self.cursor
    }
    fn cursor_mut(&mut self) -> &mut TokenCursor<PyTokenLoc> {
        &mut self.cursor
    }
}

impl SynBuild for PyParser {}

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
    cursor: TokenCursor<PyTokenLoc>,
    file: String,
    /// Nesting depth: 0 = top-level, >0 = inside function/loop/if.
    /// At depth 0, `x = val` emits `(var x val)` (new binding).
    /// At depth >0, `x = val` emits `(assign x val)` (mutation).
    depth: u32,
}

impl PyParser {
    fn new(tokens: Vec<PyTokenLoc>, file: &str) -> Self {
        PyParser {
            cursor: TokenCursor::new(tokens),
            file: file.to_string(),
            depth: 0,
        }
    }

    // ── Token navigation ──────────────────────────────────────────────
    // peek / peek_loc / advance / expect come from the shared `Nav` trait;
    // only the token-specific helpers below live here.

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

    // make_span / span_from / sym / list / nil_syntax come from the shared
    // `SynBuild` trait. (Python's block folding is `stmts_to_body` below, which
    // takes an explicit function-body flag rather than the `var`/`def` sniffing
    // SynBuild::stmts_to_block does.)

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
}

mod expr;
mod stmt;
#[cfg(test)]
mod tests;
