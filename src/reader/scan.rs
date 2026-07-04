//! Char-level scanner over source text, and the char-index newtype it walks.
//!
//! This is the lexer-side companion to [`TokenCursor`](super::cursor::TokenCursor):
//! it walks the **char-index** id-space (a position in a `Vec<char>` source
//! buffer), as distinct from the parser's **token-index** space. See
//! [`super::cursor`] for why the two are separate newtypes.
//!
//! [`CharCursor`] owns the source buffer together with the current line/column,
//! and advances all three in lock-step ([`CharCursor::advance`] is the only
//! mutator). The `pos += 1; if c == '\n' {…}` bookkeeping lives here once,
//! shared by all three lexers, so a position can never be advanced without its
//! line/col being updated to match.

/// A position in a source buffer — an index into a [`CharCursor`]'s `Vec<char>`.
///
/// Opaque on purpose (private inner `usize`): a [`CharIdx`] is produced by
/// [`CharCursor::pos`] (to mark the start of a lexeme) and consumed by
/// [`CharCursor::slice_from`] / [`CharCursor::offset_from`]; lexers never do
/// arithmetic on it directly.
///
/// A raw integer is not a char index:
///
/// ```compile_fail
/// use elle::reader::scan::CharIdx;
/// let raw: usize = 0;
/// let idx: CharIdx = raw; // ERROR: expected CharIdx, found usize
/// ```
///
/// Nor is a token index — the two source-position spaces do not interconvert:
///
/// ```compile_fail
/// fn want_token(_: elle::reader::cursor::TokenIdx) {}
/// want_token(elle::reader::scan::CharIdx::start()); // ERROR: CharIdx is not TokenIdx
/// ```
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct CharIdx(usize);

impl CharIdx {
    /// The index of the first character.
    pub fn start() -> Self {
        CharIdx(0)
    }
}

/// A char-by-char cursor over source text that tracks line/column as it goes.
///
/// All consumption goes through [`Self::advance`], the single place `pos`,
/// `line`, and `col` move — so they cannot drift out of sync.
pub struct CharCursor {
    input: Vec<char>,
    pos: CharIdx,
    line: usize,
    col: usize,
}

impl CharCursor {
    /// Wrap source text, positioned at the first char of line 1, column 1.
    pub fn new(input: &str) -> Self {
        Self {
            input: input.chars().collect(),
            pos: CharIdx(0),
            line: 1,
            col: 1,
        }
    }

    /// The current position, for marking the start of a lexeme (pair with
    /// [`Self::slice_from`] / [`Self::offset_from`]).
    pub fn pos(&self) -> CharIdx {
        self.pos
    }

    /// Current 1-based line.
    pub fn line(&self) -> usize {
        self.line
    }

    /// Current 1-based column.
    pub fn col(&self) -> usize {
        self.col
    }

    /// Whether every character has been consumed.
    pub fn at_end(&self) -> bool {
        self.pos.0 >= self.input.len()
    }

    /// The char `n` positions ahead of the cursor (`nth(0)` is the current
    /// char), or `None` past the end.
    pub fn nth(&self, n: usize) -> Option<char> {
        self.input.get(self.pos.0.saturating_add(n)).copied()
    }

    /// The current char, or `None` past the end.
    pub fn peek(&self) -> Option<char> {
        self.nth(0)
    }

    /// Consume and return the current char, advancing the cursor and updating
    /// line/column. Past the end this is a bounds-safe no-op returning `None`.
    pub fn advance(&mut self) -> Option<char> {
        let c = self.input.get(self.pos.0).copied()?;
        self.pos = CharIdx(self.pos.0 + 1);
        if c == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(c)
    }

    /// The chars from a previously-marked `start` up to (not including) the
    /// current position, for callers that post-process the lexeme (e.g. strip
    /// digit separators).
    pub fn span(&self, start: CharIdx) -> &[char] {
        &self.input[start.0..self.pos.0]
    }

    /// The text from a previously-marked `start` up to (not including) the
    /// current position, as a `String`.
    pub fn slice_from(&self, start: CharIdx) -> String {
        self.span(start).iter().collect()
    }

    /// The number of characters consumed since `start` (its char length).
    pub fn offset_from(&self, start: CharIdx) -> usize {
        self.pos.0 - start.0
    }

    /// The full source buffer, for the occasional ad-hoc lookahead that scans
    /// with a local index rather than the cursor position.
    pub fn chars(&self) -> &[char] {
        &self.input
    }
}

#[cfg(test)]
mod tests;
