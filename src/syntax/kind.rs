// audited: 2026-11-24
//! The kind of a syntax node: its variants, their shapes, and the bytes a short variant leaves unwritten.
//!
//! docs/impl/syntax.md

use std::mem::size_of;

use crate::value::region_slice::{RegionSlice, RegionStr};

use super::{SynRef, Syntax, SyntaxArena};

/// `repr(C, u8)`: the tag is byte 0 and a payload sits at its own alignment
/// after it, which is what lets `SyntaxKind::zero_unwritten` find the bytes
/// a short variant leaves alone.
#[derive(Debug, Clone, Copy)]
#[repr(C, u8)]
pub enum SyntaxKind {
    // Atoms
    Nil,
    Bool(bool),
    Int(i64),
    Float(f64),
    Symbol(RegionStr),
    Keyword(RegionStr),
    String(RegionStr),
    /// Mutable string literal: `@"..."`
    StringMut(RegionStr),

    // Compounds
    List(RegionSlice<Syntax>),
    /// Bracket-delimited immutable array: `[...]`
    Array(RegionSlice<Syntax>),
    /// Bracket-delimited mutable array: `@[...]`
    ArrayMut(RegionSlice<Syntax>),
    /// Brace-delimited immutable struct: `{...}`
    Struct(RegionSlice<Syntax>),
    /// Brace-delimited mutable struct: `@{...}`
    StructMut(RegionSlice<Syntax>),
    /// Pipe-delimited immutable set literal: `|...|`
    Set(RegionSlice<Syntax>),
    /// Pipe-delimited mutable set literal: `@|...|`
    SetMut(RegionSlice<Syntax>),
    /// Bytes literal: `b[...]`
    Bytes(RegionSlice<Syntax>),
    /// Mutable bytes literal: `@b[...]`
    BytesMut(RegionSlice<Syntax>),

    // Quote forms - preserved as structure for macro handling
    Quote(SynRef),
    Quasiquote(SynRef),
    Unquote(SynRef),
    UnquoteSplicing(SynRef),
    /// Splice form: `;expr` or `(splice expr)`. Marks a value for
    /// array-spreading at call sites and data constructors.
    Splice(SynRef),

    /// Internal: a hygiene-bearing template symbol carried as plain compile-time
    /// data (NOT a pre-baked heap `Value`). Never produced by the reader; created
    /// by quasiquote (`quasiquote_to_code`) to preserve a template symbol's scope
    /// set through the expansion round-trip. The analyzer materializes it as an
    /// ORDINARY allocation per execution via `HirKind::QuoteConst`
    /// (`ConstTemplate::SyntaxSymbol`) — a heap literal is an ordinary,
    /// reclaimable allocation.
    SyntaxLiteral(SynRef),
}

/// The constructor of a sequence kind: `SyntaxKind::List` and its eight
/// siblings all have this signature, so a caller can name one by value.
pub type SeqCtor = fn(RegionSlice<Syntax>) -> SyntaxKind;

/// The constructor of a single-child kind: `SyntaxKind::Quote` and its
/// siblings.
pub type WrapCtor = fn(SynRef) -> SyntaxKind;

/// How a compound kind holds its children — the shape a generic walk needs in
/// order to take a tree apart and put an equivalent one back together.
///
/// The nine sequence kinds and the five single-child kinds each differ only in
/// their tag, so every recursive pass over the tree (deep copy, scope
/// stamping, keyword learning, the `SyntaxLiteral` scan) would otherwise
/// repeat a fourteen-arm match. [`SyntaxKind::children`] and
/// [`SyntaxKind::rebuild`] are that match, written once. A `SyntaxLiteral`
/// holds a node but reports no children, so its shape is an atom's.
enum Shape {
    Atom,
    /// A sequence kind and its constructor.
    Seq(SeqCtor),
    /// A single-child kind and its constructor.
    Wrap(WrapCtor),
}

impl SyntaxKind {
    /// A list over `items`, or the quoting form the list spells out: two
    /// items whose head is the symbol `quote`, `quasiquote`, `unquote` or
    /// `unquote-splicing` make that form's node over the second item. The
    /// reader, `Syntax::from_value`, `rebuild` and `Syntax::list` build every
    /// list here, so no tree they build holds a quoting form as a list
    /// (docs/impl/syntax.md). It is a [`SeqCtor`], so a caller names it where
    /// it would name `SyntaxKind::List`.
    pub fn list(items: RegionSlice<Syntax>) -> SyntaxKind {
        if let [head, _] = items.as_slice() {
            let quoting: Option<WrapCtor> = match head.as_symbol() {
                Some("quote") => Some(SyntaxKind::Quote),
                Some("quasiquote") => Some(SyntaxKind::Quasiquote),
                Some("unquote") => Some(SyntaxKind::Unquote),
                Some("unquote-splicing") => Some(SyntaxKind::UnquoteSplicing),
                _ => None,
            };
            if let Some(make) = quoting {
                // The form is a node of the slice, in the slice's region,
                // which outlives every node that holds the slice.
                return make(unsafe { SynRef::from_raw(&items.as_slice()[1]) });
            }
        }
        SyntaxKind::List(items)
    }

    fn shape(&self) -> Shape {
        match self {
            SyntaxKind::List(_) => Shape::Seq(SyntaxKind::list),
            SyntaxKind::Array(_) => Shape::Seq(SyntaxKind::Array),
            SyntaxKind::ArrayMut(_) => Shape::Seq(SyntaxKind::ArrayMut),
            SyntaxKind::Struct(_) => Shape::Seq(SyntaxKind::Struct),
            SyntaxKind::StructMut(_) => Shape::Seq(SyntaxKind::StructMut),
            SyntaxKind::Set(_) => Shape::Seq(SyntaxKind::Set),
            SyntaxKind::SetMut(_) => Shape::Seq(SyntaxKind::SetMut),
            SyntaxKind::Bytes(_) => Shape::Seq(SyntaxKind::Bytes),
            SyntaxKind::BytesMut(_) => Shape::Seq(SyntaxKind::BytesMut),
            SyntaxKind::Quote(_) => Shape::Wrap(SyntaxKind::Quote),
            SyntaxKind::Quasiquote(_) => Shape::Wrap(SyntaxKind::Quasiquote),
            SyntaxKind::Unquote(_) => Shape::Wrap(SyntaxKind::Unquote),
            SyntaxKind::UnquoteSplicing(_) => Shape::Wrap(SyntaxKind::UnquoteSplicing),
            SyntaxKind::Splice(_) => Shape::Wrap(SyntaxKind::Splice),
            _ => Shape::Atom,
        }
    }

    /// This kind's children: the elements of a sequence, the one child of a
    /// wrapping kind, or nothing for an atom.
    pub fn children(&self) -> &[Syntax] {
        match self {
            SyntaxKind::List(items)
            | SyntaxKind::Array(items)
            | SyntaxKind::ArrayMut(items)
            | SyntaxKind::Struct(items)
            | SyntaxKind::StructMut(items)
            | SyntaxKind::Set(items)
            | SyntaxKind::SetMut(items)
            | SyntaxKind::Bytes(items)
            | SyntaxKind::BytesMut(items) => items.as_slice(),
            SyntaxKind::Quote(inner)
            | SyntaxKind::Quasiquote(inner)
            | SyntaxKind::Unquote(inner)
            | SyntaxKind::UnquoteSplicing(inner)
            | SyntaxKind::Splice(inner) => unsafe { std::slice::from_raw_parts(inner.as_ptr(), 1) },
            // A syntax literal's child is data the walkers must not descend
            // into: it carries the scopes of the context it was captured in.
            // Reporting no children is how `children`/`rebuild` state that.
            SyntaxKind::SyntaxLiteral(_) => &[],
            _ => &[],
        }
    }

    /// The count of children [`rebuild`](Self::rebuild) takes for this kind:
    /// none for an atom, one for a single-child kind, and `None` for a
    /// sequence, which takes any count.
    pub(crate) fn fixed_children(&self) -> Option<usize> {
        match self.shape() {
            Shape::Atom => Some(0),
            Shape::Wrap(_) => Some(1),
            Shape::Seq(_) => None,
        }
    }

    /// This kind again, over `items` — the inverse of [`children`](Self::children).
    ///
    /// An atom, and a `SyntaxLiteral` (which reports no children), rebuild as
    /// themselves. A wrapping kind takes `items[0]`.
    pub(crate) fn rebuild(&self, arena: &SyntaxArena, items: &[Syntax]) -> SyntaxKind {
        match self.shape() {
            Shape::Atom => *self,
            Shape::Seq(make) => make(arena.nodes(items)),
            Shape::Wrap(make) => make(arena.node(items[0])),
        }
    }

    /// Write zero to every byte of this kind outside its tag and its payload:
    /// the bytes between the tag and the payload, and the bytes a payload
    /// shorter than the widest leaves after itself. A kind built on the stack
    /// carries whatever the stack held there, and a node copied into a region
    /// page has to carry nothing (docs/impl/syntax.md § "The node").
    pub(crate) fn zero_unwritten(&mut self) {
        use SyntaxKind::*;
        let base = self as *mut SyntaxKind as *mut u8;
        let offset = |payload: *const u8| payload as usize - base as usize;
        // Where the payload starts and how long it is. `Nil` has none, so
        // everything past the tag is zeroed.
        let (at, len) = match self {
            Nil => (1, 0),
            Bool(b) => (offset(b as *const bool as *const u8), size_of::<bool>()),
            Int(n) => (offset(n as *const i64 as *const u8), size_of::<i64>()),
            Float(f) => (offset(f as *const f64 as *const u8), size_of::<f64>()),
            Symbol(s) | Keyword(s) | String(s) | StringMut(s) => (
                offset(s as *const RegionStr as *const u8),
                size_of::<RegionStr>(),
            ),
            List(n) | Array(n) | ArrayMut(n) | Struct(n) | StructMut(n) | Set(n) | SetMut(n)
            | Bytes(n) | BytesMut(n) => (
                offset(n as *const RegionSlice<Syntax> as *const u8),
                size_of::<RegionSlice<Syntax>>(),
            ),
            Quote(r) | Quasiquote(r) | Unquote(r) | UnquoteSplicing(r) | Splice(r)
            | SyntaxLiteral(r) => (offset(r as *const SynRef as *const u8), size_of::<SynRef>()),
        };
        let end = size_of::<SyntaxKind>();
        // The borrows of the payload ended with the match, and the tag at
        // byte 0 is the one byte before `at` that stays.
        unsafe {
            std::ptr::write_bytes(base.add(1), 0, at - 1);
            std::ptr::write_bytes(base.add(at + len), 0, end - at - len);
        }
    }
}
