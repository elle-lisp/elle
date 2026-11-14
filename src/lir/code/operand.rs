// audited: 2026-11-13
//! The borrowed operands an `InstrRef` carries: a template's bytes, stash slots, a call's offers, a run of constants.
//!
//! src/lir/AGENTS.md
//! docs/impl/lir.md

use super::record::{kind, ConstRec};
use crate::value::{ConstTemplate, SymbolId};

/// An immediate constant, read out of a `ConstRec`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ConstRef {
    Nil,
    EmptyList,
    Bool(bool),
    Int(i64),
    Float(f64),
    Symbol(SymbolId),
    Keyword(u64),
}

impl ConstRef {
    /// The constant a record holds. Panics on a record that holds raw bits,
    /// which no instruction reads as a `ConstRef`.
    pub(crate) fn of(rec: &ConstRec) -> ConstRef {
        match rec.kind {
            kind::NIL => ConstRef::Nil,
            kind::EMPTY_LIST => ConstRef::EmptyList,
            kind::BOOL => ConstRef::Bool(rec.bits != 0),
            kind::INT => ConstRef::Int(rec.bits as i64),
            kind::FLOAT => ConstRef::Float(f64::from_bits(rec.bits)),
            kind::SYMBOL => ConstRef::Symbol(SymbolId(rec.bits)),
            kind::KEYWORD => ConstRef::Keyword(rec.bits),
            k => panic!("a constant record of kind {k} is not an immediate"),
        }
    }
}

/// A `MaterializeConst`'s template, as `ConstTemplate::encode` wrote it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct TemplateBytes<'a>(pub(crate) &'a [u8]);

impl<'a> TemplateBytes<'a> {
    /// A template as `ConstTemplate::encode` wrote it, for an instruction
    /// being built.
    pub fn new(bytes: &'a [u8]) -> Self {
        TemplateBytes(bytes)
    }

    /// The encoded bytes, which the bytecode carries inline unchanged.
    pub fn bytes(&self) -> &'a [u8] {
        self.0
    }

    /// The template the bytes encode.
    pub fn decode(&self) -> ConstTemplate {
        let mut ip = 0;
        ConstTemplate::decode(self.0, &mut ip)
    }
}

/// A tail call's borrowed-argument stash slots, each a local slot number.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Slots<'a>(pub(crate) &'a [u32]);

impl<'a> Slots<'a> {
    /// Stash slots, each a local slot number, for an instruction being built.
    pub fn new(slots: &'a [u32]) -> Self {
        Slots(slots)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = u16> + 'a {
        self.0.iter().map(|&s| s as u16)
    }
}

/// An argument a call hands its callee with the frame's own reference: its
/// position in the argument list, and the local slot the frame's release of it
/// loads (docs/impl/region/handoff.md).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Offer {
    position: u8,
    slot: u16,
}

impl Offer {
    /// The positions an offer may name. Compiled code hears back which offers
    /// a callee took as one 64-bit word, a bit per position.
    pub const LIMIT: usize = 64;

    /// The offer of the argument at `position`, whose release loads `slot`,
    /// or `None` for a position at or past [`Offer::LIMIT`].
    pub const fn new(position: usize, slot: u16) -> Option<Offer> {
        if position < Self::LIMIT {
            Some(Offer {
                position: position as u8,
                slot,
            })
        } else {
            None
        }
    }

    pub const fn position(self) -> usize {
        self.position as usize
    }

    pub const fn slot(self) -> u16 {
        self.slot
    }

    /// This offer's bit in a word of positions.
    pub const fn bit(self) -> u64 {
        1 << self.position
    }

    /// The one pool word an offer is stored as: the position above the slot.
    pub const fn word(self) -> u32 {
        ((self.position as u32) << 16) | self.slot as u32
    }

    fn of_word(word: u32) -> Offer {
        Offer {
            position: (word >> 16) as u8,
            slot: word as u16,
        }
    }

    /// The three bytes an offer takes in the bytecode: the position, then the
    /// slot big-endian (docs/impl/bytecode.md).
    pub const fn code(self) -> [u8; 3] {
        let [hi, lo] = self.slot.to_be_bytes();
        [self.position, hi, lo]
    }

    /// The offer [`Offer::code`] wrote.
    pub const fn of_code([position, hi, lo]: [u8; 3]) -> Offer {
        Offer {
            position,
            slot: u16::from_be_bytes([hi, lo]),
        }
    }
}

/// A call's offers, each one pool word: a non-tail call's moves, or a tail
/// call's late mints.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Offers<'a>(pub(crate) &'a [u32]);

impl<'a> Offers<'a> {
    /// A call that offers nothing.
    pub const NONE: Offers<'static> = Offers(&[]);

    /// Offers as `Offer::word` wrote them, for an instruction being built.
    pub fn new(words: &'a [u32]) -> Self {
        Offers(words)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = Offer> + 'a {
        self.0.iter().map(|&w| Offer::of_word(w))
    }

    /// The offered positions as one word, a bit each: what compiled code
    /// hands its call helper.
    pub fn positions(&self) -> u64 {
        self.iter().fold(0, |word, o| word | o.bit())
    }
}

/// A run of immediate constants: a `StructRest`'s excluded keys.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ConstList<'a>(pub(crate) &'a [ConstRec]);

impl<'a> ConstList<'a> {
    /// A run of constant records, for an instruction being built.
    pub fn new(keys: &'a [ConstRec]) -> Self {
        ConstList(keys)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = ConstRef> + 'a {
        self.0.iter().map(ConstRef::of)
    }
}

// Each borrowed operand prints as the value it stands for — a template, a
// slot list, a key list — so an instruction's `Debug` text reads its meaning
// rather than its encoding.

impl std::fmt::Debug for TemplateBytes<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.decode().fmt(f)
    }
}

impl std::fmt::Debug for Slots<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl std::fmt::Debug for Offers<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entries(self.iter().map(|o| (o.position(), o.slot())))
            .finish()
    }
}

impl std::fmt::Debug for ConstList<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}
