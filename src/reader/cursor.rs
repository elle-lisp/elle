//! Cursor over a token stream, and the token-index newtype it walks.
//!
//! The reader frontends move through two distinct integer index-spaces that
//! must never be confused — the same discipline `docs/impl/region-model.md` applies to
//! region ids, applied here to source positions:
//!
//! * **token-index** — a position in a `Vec<Tok>` token stream, walked by the
//!   recursive-descent parsers: [`TokenIdx`] / [`TokenCursor`].
//! * **char-index** — a position in a `Vec<char>` source buffer, walked by the
//!   hand-written lexers (`CharIdx` / `CharCursor` in `super::scan`).
//!
//! Each has its own newtype, so a token index and a char index are
//! unswappable — passing one where the other is expected is a compile error.
//!
//! [`TokenCursor`] also centralises bounds-safety: [`TokenCursor::advance`]
//! never indexes out of range, so the raw `tokens[pos]` form lives in exactly
//! one bounds-checked place rather than being hand-rolled per parser (where it
//! would panic if driven past the last token).

/// A position in a token stream — an index into a [`TokenCursor`]'s `Vec<Tok>`.
///
/// Opaque on purpose: the inner `usize` is private, so a [`TokenIdx`] cannot be
/// built from, compared against, or used as a raw integer. Parsers obtain one
/// from [`TokenCursor::pos`] (to save a spot for backtracking) and hand it back
/// to [`TokenCursor::seek`]; they never do arithmetic on it.
///
/// A raw integer is not a token index:
///
/// ```compile_fail
/// use elle::reader::cursor::TokenIdx;
/// let raw: usize = 3;
/// let idx: TokenIdx = raw; // ERROR: expected TokenIdx, found usize
/// ```
///
/// (The companion `CharIdx` in `super::scan` is likewise distinct — see its
/// own cross-space `compile_fail` example.)
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct TokenIdx(usize);

/// A bounds-safe cursor over a `Vec<T>` token stream.
///
/// Owns the tokens and the current [`TokenIdx`]. All navigation goes through
/// here, so the panic-prone `let t = &self.tokens[self.pos]; self.pos += 1;`
/// lives in exactly one bounds-checked place ([`Self::advance`]) rather than
/// being inlined into each parser.
pub struct TokenCursor<T> {
    tokens: Vec<T>,
    pos: TokenIdx,
}

impl<T> TokenCursor<T> {
    /// Wrap a token stream, positioned at the first token.
    pub fn new(tokens: Vec<T>) -> Self {
        Self {
            tokens,
            pos: TokenIdx(0),
        }
    }

    /// The current position, for save/restore backtracking (pair with
    /// [`Self::seek`]).
    pub fn pos(&self) -> TokenIdx {
        self.pos
    }

    /// Restore a position previously returned by [`Self::pos`].
    pub fn seek(&mut self, pos: TokenIdx) {
        self.pos = pos;
    }

    /// Total number of tokens in the stream.
    pub fn len(&self) -> usize {
        self.tokens.len()
    }

    /// Whether the stream is empty.
    pub fn is_empty(&self) -> bool {
        self.tokens.is_empty()
    }

    /// Whether every token has been consumed.
    pub fn at_end(&self) -> bool {
        self.pos.0 >= self.tokens.len()
    }

    /// Borrow the token `n` positions ahead of the cursor (`nth(0)` is the
    /// current token), or `None` past the end.
    pub fn nth(&self, n: usize) -> Option<&T> {
        self.tokens.get(self.pos.0.saturating_add(n))
    }

    /// Borrow the current token, or `None` past the end.
    pub fn current(&self) -> Option<&T> {
        self.nth(0)
    }

    /// Consume and return the current token, advancing the cursor.
    ///
    /// Past the end this is a bounds-safe no-op returning `None`; it never
    /// indexes out of range. This is the one place that performs the raw
    /// indexing, with the bounds check that makes it panic-free.
    pub fn advance(&mut self) -> Option<&T> {
        let i = self.pos.0;
        if i < self.tokens.len() {
            self.pos = TokenIdx(i + 1);
            Some(&self.tokens[i])
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests;
