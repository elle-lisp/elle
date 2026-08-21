//! Token-type-agnostic token navigation shared by the JS/Py/Lua frontends.
//!
//! Each frontend parser walks a `TokenCursor` of its own `*TokenLoc` records.
//! The peek/advance/expect logic over that cursor — including the policy for
//! reading past end-of-input and the one canonical "expected X, got Y" error
//! format — was copy-pasted into all three parsers. Abstracting a located
//! token behind [`Located`] lets that logic live once on [`Nav`], which each
//! parser opts into by supplying just its cursor accessors. The peek/advance/
//! expect methods stay methods, so call sites (`self.peek()`, `self.expect(…)`)
//! are unchanged.

use super::cursor::TokenCursor;
use super::token::SourceLoc;
use std::fmt::Debug;

/// File name on the synthetic Eof location a parser returns once its cursor
/// runs past the last real token. One spelling shared by the three frontends.
pub(super) const EOF_FILE: &str = "<eof>";

/// A located token — the per-language `*TokenLoc` record pairing a token with
/// its source location, plus the shared Eof sentinel used past end-of-input.
///
/// Implemented for `JsTokenLoc` / `PyTokenLoc` / `LuaTokenLoc`; abstracting the
/// three near-identical records behind this trait is what lets [`Nav`] be
/// written once.
pub(super) trait Located: Sized + 'static {
    /// The bare token enum this record wraps (`JsToken`, `PyToken`, …).
    type Tok: PartialEq + Clone + Debug;

    /// The wrapped token.
    fn token(&self) -> &Self::Tok;

    /// The token's source location.
    fn loc(&self) -> &SourceLoc;

    /// The shared static Eof sentinel record, returned past end-of-input.
    fn eof() -> &'static Self;

    /// The Eof token itself, for peeking past end-of-input.
    fn eof_token() -> &'static Self::Tok;
}

/// Bounds-safe token navigation shared by the three frontend parsers.
///
/// A parser supplies the two cursor accessors; the navigation logic is provided
/// here so it cannot drift between frontends.
pub(super) trait Nav {
    /// The located-token record this parser's cursor walks.
    type Loc: Located;

    fn cursor(&self) -> &TokenCursor<Self::Loc>;
    fn cursor_mut(&mut self) -> &mut TokenCursor<Self::Loc>;

    /// The current token, or the Eof token past end-of-input.
    fn peek(&self) -> &<Self::Loc as Located>::Tok {
        // An explicit match (rather than `unwrap_or(eof_token())`) keeps the
        // returned reference tied to `&self`: the 'static Eof sentinel coerces
        // down to that lifetime, whereas `unwrap_or` would pin it to 'static.
        match self.cursor().current() {
            Some(l) => l.token(),
            None => Self::Loc::eof_token(),
        }
    }

    /// The current located token, or the Eof sentinel past end-of-input.
    fn peek_loc(&self) -> &Self::Loc {
        match self.cursor().current() {
            Some(l) => l,
            None => Self::Loc::eof(),
        }
    }

    /// Consume and return the current located token, or the Eof sentinel past
    /// end-of-input (bounds-safe — never indexes out of range).
    fn advance(&mut self) -> &Self::Loc {
        match self.cursor_mut().advance() {
            Some(l) => l,
            None => Self::Loc::eof(),
        }
    }

    /// Consume the current token if it equals `expected`, else report a
    /// located "expected X, got Y" error.
    fn expect(&mut self, expected: &<Self::Loc as Located>::Tok) -> Result<&Self::Loc, String> {
        if self.peek() == expected {
            Ok(self.advance())
        } else {
            let loc = self.peek_loc().loc();
            Err(format!(
                "{}: expected {:?}, got {:?}",
                loc.position(),
                expected,
                self.peek()
            ))
        }
    }
}

#[cfg(test)]
mod tests;
