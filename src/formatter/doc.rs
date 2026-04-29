//! Wadler-style document algebra for pretty printing.
//!
//! Based on "A Prettier Printer" (Wadler, 2003). The core idea:
//! build a Doc tree that describes layout *choices* (flat vs broken),
//! then a separate renderer evaluates the tree within a line-width
//! budget and picks the optimal layout.
//!
//! ## Core primitives
//!
//! - `Empty`        — nothing
//! - `Text(s)`      — literal string
//! - `Concat(ds)`   — sequence of docs
//! - `Nest(n, d)`   — increase indentation by n levels when breaking
//! - `Break`        — space if flat, newline + indent if broken
//! - `Group(d)`     — try flat; break inner if doesn't fit
//! - `HardBreak`    — unconditional newline (never flat)
//! - `CommentBreak` — like HardBreak, absorbed by adjacent HardBreak
//! - `Align(d)`     — set indent to current column

/// A document describing layout choices for a pretty printer.
#[derive(Debug, Clone, PartialEq)]
pub enum Doc {
    /// Empty document — produces no output.
    Empty,

    /// Literal text string.
    Text(String),

    /// Concatenation of documents in sequence.
    Concat(Vec<Doc>),

    /// Increase indentation level by `n` when breaking within this doc.
    /// The actual indent width is `n * indent_width` from the config.
    Nest(usize, Box<Doc>),

    /// A break point. Space if the enclosing Group stays flat,
    /// newline + current indentation if broken.
    Break,

    /// Try to lay out the inner doc on one line. If it doesn't fit
    /// within the page width, break it (inner Breaks become newlines).
    Group(Box<Doc>),

    /// Unconditional line break. Always produces a newline regardless
    /// of whether the enclosing Group is flat or broken.
    HardBreak,

    /// Like HardBreak but absorbed by an adjacent HardBreak.
    /// Used after trailing comments to prevent double-newline when
    /// the inter-sibling HardBreak follows.
    CommentBreak,

    /// Align inner content to the current column.
    /// Sets the indent reference to the current column position so that
    /// Breaks inside `inner` align to where Align was entered.
    Align(Box<Doc>),
}

impl Doc {
    /// Create an empty document.
    pub fn empty() -> Self {
        Doc::Empty
    }

    /// Create a text document.
    pub fn text(s: impl Into<String>) -> Self {
        Doc::Text(s.into())
    }

    /// Create a newline (hard break).
    pub fn hardbreak() -> Self {
        Doc::HardBreak
    }

    /// Create a comment break (absorbed by adjacent HardBreak).
    pub fn comment_break() -> Self {
        Doc::CommentBreak
    }

    /// Align inner content to the current column position.
    pub fn align(inner: Doc) -> Self {
        Doc::Align(Box::new(inner))
    }

    /// Concatenate multiple docs.
    pub fn concat(docs: impl IntoIterator<Item = Doc>) -> Self {
        let docs: Vec<Doc> = docs.into_iter().collect();
        if docs.is_empty() {
            Doc::Empty
        } else if docs.len() == 1 {
            docs.into_iter().next().unwrap()
        } else {
            Doc::Concat(docs)
        }
    }

    /// Concatenate docs with a separator between each pair.
    /// The separator is a Break (space if flat, newline if broken).
    pub fn intersperse(docs: impl IntoIterator<Item = Doc>) -> Self {
        let docs: Vec<Doc> = docs.into_iter().collect();
        if docs.is_empty() {
            return Doc::Empty;
        }
        if docs.len() == 1 {
            return docs.into_iter().next().unwrap();
        }
        let mut result = Vec::with_capacity(docs.len() * 2 - 1);
        for (i, doc) in docs.into_iter().enumerate() {
            if i > 0 {
                result.push(Doc::Break);
            }
            result.push(doc);
        }
        Doc::Concat(result)
    }

    /// Concatenate docs with a hard break between each.
    pub fn join_hardbreak(docs: impl IntoIterator<Item = Doc>) -> Self {
        let docs: Vec<Doc> = docs.into_iter().collect();
        if docs.is_empty() {
            return Doc::Empty;
        }
        let mut result = Vec::with_capacity(docs.len() * 2 - 1);
        for (i, doc) in docs.into_iter().enumerate() {
            if i > 0 {
                result.push(Doc::HardBreak);
            }
            result.push(doc);
        }
        Doc::Concat(result)
    }

    /// Increase indentation by `n` levels.
    pub fn nest(self, n: usize) -> Self {
        if matches!(self, Doc::Empty) {
            return self;
        }
        Doc::Nest(n, Box::new(self))
    }

    /// Try to lay out flat; break if doesn't fit.
    pub fn group(self) -> Self {
        if matches!(self, Doc::Empty) {
            return self;
        }
        Doc::Group(Box::new(self))
    }

    /// Fill layout: greedily pack elements, wrapping per-element.
    ///
    /// Produces: `a Group(Break b) Group(Break c) Group(Break d)`
    /// Each Group independently decides: space+elem if it fits on the
    /// current line, newline+elem if it doesn't.
    pub fn fill(docs: impl IntoIterator<Item = Doc>) -> Self {
        let docs: Vec<Doc> = docs.into_iter().collect();
        if docs.is_empty() {
            return Doc::Empty;
        }
        let mut parts = Vec::with_capacity(docs.len());
        for (i, doc) in docs.into_iter().enumerate() {
            if i == 0 {
                parts.push(doc);
            } else {
                parts.push(Doc::Group(Box::new(Doc::concat([Doc::Break, doc]))));
            }
        }
        Doc::Concat(parts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_doc() {
        let doc = Doc::empty();
        assert!(matches!(doc, Doc::Empty));
    }

    #[test]
    fn test_text_doc() {
        let doc = Doc::text("hello");
        assert!(matches!(doc, Doc::Text(s) if s == "hello"));
    }

    #[test]
    fn test_concat_flatten_single() {
        let doc = Doc::concat(vec![Doc::text("x")]);
        // Single-element concat is unwrapped
        assert!(matches!(doc, Doc::Text(s) if s == "x"));
    }

    #[test]
    fn test_concat_empty() {
        let doc = Doc::concat(vec![]);
        assert!(matches!(doc, Doc::Empty));
    }

    #[test]
    fn test_nest_empty() {
        let doc = Doc::empty().nest(2);
        // Nesting empty is still empty
        assert!(matches!(doc, Doc::Empty));
    }

    #[test]
    fn test_group_empty() {
        let doc = Doc::empty().group();
        // Grouping empty is still empty
        assert!(matches!(doc, Doc::Empty));
    }

    #[test]
    fn test_intersperse_empty() {
        let doc = Doc::intersperse(vec![]);
        assert!(matches!(doc, Doc::Empty));
    }

    #[test]
    fn test_intersperse_single() {
        let doc = Doc::intersperse(vec![Doc::text("x")]);
        assert!(matches!(doc, Doc::Text(s) if s == "x"));
    }

    #[test]
    fn test_builder_chaining() {
        let doc = Doc::concat([Doc::text("hello"), Doc::text(" "), Doc::text("world")]).group();
        assert!(matches!(doc, Doc::Group(_)));
    }
}
