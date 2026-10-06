// audited: 2026-10-06
//! The borrowed operands an `InstrRef` carries where `LirInstr` holds an owned value.
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

/// A run of immediate constants: a `StructRest`'s excluded keys.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ConstList<'a>(pub(crate) &'a [ConstRec]);

impl<'a> ConstList<'a> {
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

// Each borrowed operand prints as the value `LirInstr` holds in its place, so
// an instruction's `Debug` text reads the same in either form.

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

impl std::fmt::Debug for ConstList<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}
