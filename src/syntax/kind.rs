// audited: 2026-10-12
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
/// The nine sequence kinds and the six single-child kinds each differ only in
/// their tag, so every recursive pass over the tree (deep copy, scope
/// stamping, keyword learning, the `SyntaxLiteral` scan) would otherwise
/// repeat a fifteen-arm match. [`SyntaxKind::children`] and
/// [`SyntaxKind::rebuild`] are that match, written once.
enum Shape {
    Atom,
    /// A sequence kind and its constructor.
    Seq(SeqCtor),
    /// A single-child kind and its constructor.
    Wrap(WrapCtor),
}

impl SyntaxKind {
    fn shape(&self) -> Shape {
        match self {
            SyntaxKind::List(_) => Shape::Seq(SyntaxKind::List),
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
            SyntaxKind::SyntaxLiteral(_) => Shape::Wrap(SyntaxKind::SyntaxLiteral),
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
