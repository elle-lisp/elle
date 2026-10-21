// audited: 2026-10-21
//! The names a code payload holds outside its value slots, which a carrier spells for another instance.
//!
//! docs/impl/region/template.md
//!
//! A value walk spells every symbol and keyword it meets. A payload's fragment
//! and its LIR name symbols and keywords no value holds, so the image dumper
//! and `send` ask the payload for them here.

use crate::hir::fragment::{FragmentName, LambdaFragment};
use crate::lir::{ConstRef, InstrRef};
use crate::symbol::SymbolTable;
use crate::value::SymbolId;

use super::ClosureTemplate;

/// A name a payload holds where no value holds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PayloadName<'a> {
    /// A symbol, by its id: a fragment's binding or global, a LIR symbol
    /// constant, or a capture cell's name.
    Symbol(SymbolId),
    /// A keyword, by its payload hash: a LIR keyword constant or struct key.
    Keyword(u64),
    /// A keyword, by the spelling a fragment's body wrote.
    Spelled(&'a str),
}

impl<'a> PayloadName<'a> {
    /// The name's hash and its spelling. A spelled keyword carries its own; a
    /// hash takes the one `memo` or the keyword vocabulary holds, and a name
    /// neither knows answers `None`.
    pub(crate) fn spelling(self, memo: Option<&'a SymbolTable>) -> Option<(u64, &'a str)> {
        match self {
            PayloadName::Symbol(id) => memo.and_then(|m| m.name(id)).map(|n| (id.0, n)),
            PayloadName::Keyword(hash) => {
                crate::value::keyword::resolve_keyword_name(memo, hash).map(|n| (hash, n))
            }
            PayloadName::Spelled(k) => Some((crate::value::keyword::keyword_hash(k), k)),
        }
    }
}

impl ClosureTemplate {
    /// Call `f` on every name this code object's payload holds outside its
    /// value slots: each name its fragment holds, and each its LIR's records
    /// hold. The bytecode's capture-cell operands repeat the LIR's cell names,
    /// so they are not read. Children are code objects of their own, and are
    /// not walked.
    pub(crate) fn for_each_name(&self, mut f: impl FnMut(PayloadName<'_>)) {
        if let Some(fragment) = LambdaFragment::decode(self.fragment()) {
            fragment.for_each_name(&mut |n| {
                f(match n {
                    FragmentName::Symbol(id) => PayloadName::Symbol(id),
                    FragmentName::Keyword(k) => PayloadName::Spelled(k),
                })
            });
        }
        let Some(lir) = self.lir() else {
            return;
        };
        for node in lir.nodes() {
            match node.instr() {
                InstrRef::Const { value, .. }
                | InstrRef::StructGetOrNil { key: value, .. }
                | InstrRef::StructGetDestructure { key: value, .. } => const_name(value, &mut f),
                InstrRef::StructRest { exclude_keys, .. } => {
                    for key in exclude_keys.iter() {
                        const_name(key, &mut f);
                    }
                }
                InstrRef::MakeCaptureCell { name, .. } => f(PayloadName::Symbol(name)),
                _ => {}
            }
        }
    }
}

/// The name a LIR constant holds, if it is a symbol or a keyword.
fn const_name(c: ConstRef, f: &mut impl FnMut(PayloadName<'_>)) {
    match c {
        ConstRef::Symbol(id) => f(PayloadName::Symbol(id)),
        ConstRef::Keyword(hash) => f(PayloadName::Keyword(hash)),
        _ => {}
    }
}
