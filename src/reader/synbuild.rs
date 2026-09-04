//! Token-type-agnostic `Syntax` construction shared by the JS/Py/Lua frontends.
//!
//! The three recursive-descent frontends each emit the same `Syntax` trees the
//! s-expression reader produces, so the node-building helpers are identical
//! regardless of source language. The node-building helpers live once as
//! default methods on [`SynBuild`], which each parser opts into with an empty
//! `impl`, rather than being copy-pasted into every parser. Because they stay
//! methods, the call sites (`self.sym(...)`, `self.list(...)`) read naturally.

use super::token::SourceLoc;
use crate::syntax::{Span, Syntax, SyntaxArena, SyntaxKind};

/// Parameter-list marker for a rest/variadic parameter (the `&` before the
/// final name in the emitted `(fn (a b & rest) …)`). One spelling, shared by
/// all three frontends rather than a bare `"&"` literal repeated at each site.
pub(super) const REST_PARAM: &str = "&";

/// `Syntax`-tree construction shared by the three frontend parsers.
///
/// Of these, `make_span` is the one with real logic — and so the one most
/// dangerous to let drift between copies, since a frontend that dropped the
/// `is_unknown` guard would attach a bogus file to every span. Centralising it
/// here removes that hazard.
pub(super) trait SynBuild {
    /// Where this parser's nodes are born. Every node-building default below
    /// allocates through it, so a frontend states its arena once.
    fn arena(&self) -> &SyntaxArena;

    /// Build a span at `loc` covering `len` source columns, tagging it with the
    /// originating file unless the location is the unknown-origin sentinel.
    fn make_span(&self, loc: &SourceLoc, len: usize) -> Span {
        let mut span = Span::new(0, len, loc.line as u32, loc.col as u32);
        if !loc.is_unknown() {
            span = span.with_file(&loc.file);
        }
        span
    }

    /// A one-column span at `loc`, for point-like nodes (symbols, `nil`).
    fn span_from(&self, loc: &SourceLoc) -> Span {
        self.make_span(loc, 1)
    }

    /// A symbol node named `name`, spanned at `loc`.
    fn sym(&self, name: &str, loc: &SourceLoc) -> Syntax {
        Syntax::symbol(self.arena(), name, self.span_from(loc))
    }

    /// A keyword node named `name`, with the given span.
    fn kw(&self, name: &str, span: Span) -> Syntax {
        Syntax::keyword(self.arena(), name, span)
    }

    /// A string-literal node, with the given span.
    fn str_lit(&self, text: &str, span: Span) -> Syntax {
        Syntax::string(self.arena(), text, span)
    }

    /// A list node wrapping `items`, with the given span.
    fn list(&self, items: Vec<Syntax>, span: Span) -> Syntax {
        Syntax::list(self.arena(), &items, span)
    }

    /// An immutable-array node wrapping `items`, with the given span.
    fn arr(&self, items: Vec<Syntax>, span: Span) -> Syntax {
        Syntax::array(self.arena(), &items, span)
    }

    /// A `nil` node, spanned at `loc`.
    fn nil_syntax(&self, loc: &SourceLoc) -> Syntax {
        Syntax::new(SyntaxKind::Nil, self.span_from(loc))
    }

    /// Fold a statement sequence into a single expression: empty → `nil`,
    /// single → itself, many → `(block …)` when any statement binds a local
    /// (`var`/`def`) else `(begin …)`. Shared by the JS and Lua block parsers
    /// (Python's blocks decide `block`/`begin` from a flag instead).
    fn stmts_to_block(&self, mut stmts: Vec<Syntax>, loc: &SourceLoc) -> Syntax {
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
}
