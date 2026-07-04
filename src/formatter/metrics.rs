//! Layout metrics: distinct newtypes for the renderer's spatial quantities.
//!
//! The Wadler renderer juggles five conceptually-different numbers that are
//! all "just a `usize`" in the naive encoding:
//!
//! - the current **column** (and, separately, a measured **flat width**),
//! - the current **indent** (absolute spaces at the start of a broken line),
//! - a **nest level** (the `n` of `Nest(n, _)`, a count of indent *steps*),
//! - the configured **indent width** (spaces per step),
//! - the configured **line width** (the page budget).
//!
//! Encoded as bare `usize`, any two of these can be swapped at a call site
//! with no compile error — and the renderer threads `col` and `indent`
//! side-by-side through every recursive call, which is exactly where such a
//! swap would silently corrupt every line of output. Giving each quantity its
//! own type makes those swaps type errors, and moves the one piece of real
//! arithmetic — converting a nest *level* into an indent *width* (`n * step`) —
//! behind a single named, tested method instead of an inline `*` that could
//! drift to a `+`.
//!
//! All of these are cheap `Copy` wrappers; they compile to the same code as
//! the bare `usize`s they replace.

/// A horizontal position, measured in characters from the left margin.
///
/// Also used for a *measured flat width* — the two are the same kind of
/// quantity (a count of columns) and are freely added: a starting column
/// plus a flat width yields the column the content would end at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Column(usize);

/// An absolute indentation, measured in spaces at the start of a line.
///
/// Distinct from [`Column`] so the renderer cannot pass a running column
/// where a line's indent is expected (or vice-versa).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Indent(usize);

/// A nesting depth, counted in indent *steps* (not spaces).
///
/// This is the `n` carried by `Doc::Nest(n, _)`. It only becomes a concrete
/// number of spaces once multiplied by an [`IndentWidth`] via [`widen`].
///
/// [`widen`]: IndentLevel::widen
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct IndentLevel(usize);

/// The configured number of spaces per indent step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct IndentWidth(usize);

/// The configured page budget: the column past which content should break.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct LineWidth(usize);

impl Column {
    /// Column zero — the left margin.
    pub const ZERO: Column = Column(0);

    /// A column at `n` characters from the left margin.
    pub fn new(n: usize) -> Self {
        Column(n)
    }

    /// The raw character count.
    pub fn get(self) -> usize {
        self.0
    }

    /// Advance by `n` literal characters (e.g. the length of emitted text).
    pub fn advance(self, n: usize) -> Self {
        Column(self.0 + n)
    }

    /// Add a flat width to this column, yielding the column content laid out
    /// flat from here would end at. Saturates rather than overflowing.
    pub fn plus(self, width: Column) -> Self {
        Column(self.0.saturating_add(width.0))
    }

    /// Add a flat width, returning `None` on overflow. Used while measuring a
    /// doc whose total flat width must not wrap around.
    pub fn checked_plus(self, width: Column) -> Option<Self> {
        self.0.checked_add(width.0).map(Column)
    }

    /// True if this column lies within the page budget (`<= width`).
    pub fn fits(self, width: LineWidth) -> bool {
        self.0 <= width.0
    }

    /// Reinterpret this column as an indentation. Used by `Align`, which sets
    /// the indent reference to wherever the cursor currently sits.
    pub fn as_indent(self) -> Indent {
        Indent(self.0)
    }
}

impl Indent {
    /// No indentation — the left margin.
    pub const ZERO: Indent = Indent(0);

    /// An indent of `n` spaces.
    pub fn new(n: usize) -> Self {
        Indent(n)
    }

    /// The raw space count.
    pub fn get(self) -> usize {
        self.0
    }

    /// Add a further indent (e.g. the widened result of a `Nest`).
    pub fn plus(self, more: Indent) -> Self {
        Indent(self.0 + more.0)
    }

    /// The column reached after emitting this indent at the start of a line.
    pub fn as_column(self) -> Column {
        Column(self.0)
    }

    /// The literal run of spaces this indent renders as.
    pub fn spaces(self) -> String {
        " ".repeat(self.0)
    }
}

impl IndentLevel {
    /// A nesting depth of `n` indent steps.
    pub fn new(n: usize) -> Self {
        IndentLevel(n)
    }

    /// Convert this step count into a concrete [`Indent`] given the width of a
    /// single step: `n` steps of `width` spaces each is `n * width` spaces.
    pub fn widen(self, width: IndentWidth) -> Indent {
        Indent(self.0 * width.0)
    }
}

impl IndentWidth {
    /// `n` spaces per indent step.
    pub fn new(n: usize) -> Self {
        IndentWidth(n)
    }

    /// The raw space count of one step.
    pub fn get(self) -> usize {
        self.0
    }
}

impl LineWidth {
    /// A page budget of `n` columns.
    pub fn new(n: usize) -> Self {
        LineWidth(n)
    }

    /// The raw column budget.
    pub fn get(self) -> usize {
        self.0
    }

    /// The midpoint column. Used to cap `Align`: past half the page width we
    /// stop introducing fresh alignment points so deep nesting can't march
    /// off the right edge.
    pub fn half(self) -> Column {
        Column(self.0 / 2)
    }
}

#[cfg(test)]
mod tests;
