//! Source-position newtypes for the trivia/comment layer.
//!
//! Attaching comments and blank lines to syntax nodes is all byte-offset and
//! line-number arithmetic. In the naive encoding a comment carries a
//! `byte_offset: usize` and two `u32`s — `line` and `col` — and blank lines
//! carry a `count: u32`. Three of those four are the same primitive sitting
//! next to each other in a struct literal, so `line`/`col`/`count` can be
//! transposed at a construction site with no compile error, and a byte offset
//! can be compared against the wrong quantity just as silently.
//!
//! These wrappers give each role its own type. The values that get *compared*
//! during attachment ([`ByteOffset`], [`LineNum`]) derive `Ord` so the
//! existing comparisons keep working; the conversions to/from the raw lexer
//! and `Span` values happen explicitly at the module boundary via `new`/`get`.

/// A byte offset into the source string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ByteOffset(usize);

/// A 1-indexed source line number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct LineNum(u32);

/// A 1-indexed source column number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ColNum(u32);

/// A count of consecutive blank lines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct BlankCount(u32);

impl ByteOffset {
    /// The largest representable offset — used as a "past the end" sentinel
    /// when there is no following form to bound a trivia search.
    pub const MAX: ByteOffset = ByteOffset(usize::MAX);

    /// A byte offset at position `n`.
    pub fn new(n: usize) -> Self {
        ByteOffset(n)
    }

    /// The raw byte position.
    pub fn get(self) -> usize {
        self.0
    }
}

impl LineNum {
    /// Line number `n` (1-indexed).
    pub fn new(n: u32) -> Self {
        LineNum(n)
    }

    /// The raw line number.
    pub fn get(self) -> u32 {
        self.0
    }
}

impl ColNum {
    /// Column number `n` (1-indexed).
    pub fn new(n: u32) -> Self {
        ColNum(n)
    }

    /// The raw column number.
    pub fn get(self) -> u32 {
        self.0
    }
}

impl BlankCount {
    /// A run of `n` blank lines.
    pub fn new(n: u32) -> Self {
        BlankCount(n)
    }

    /// The raw count.
    pub fn get(self) -> u32 {
        self.0
    }

    /// The count clamped to at most `max` — the formatter collapses any run
    /// of blank lines to a small ceiling rather than echoing them verbatim.
    pub fn capped_at(self, max: u32) -> u32 {
        self.0.min(max)
    }
}

#[cfg(test)]
mod tests;
