// audited: 2026-11-25
//! Loading a value along a pattern's access path, the view each step reads into, and the constants a struct pattern's keys lower to.
//!
//! docs/impl/lir.md
//! docs/macros/syntax.md
//!
//! An access path is a chain of field accesses (car, cdr, array index, struct
//! key) from the scrutinee root.

use super::*;
use crate::hir::decision::AccessPath;
use crate::hir::PatternKey;

/// The immediate a struct pattern's key lowers to: a keyword's hash, or a
/// symbol's id.
pub(super) fn pattern_key_const(key: &PatternKey) -> ConstRef {
    match key {
        PatternKey::Keyword(k) => ConstRef::Keyword(crate::value::keyword::keyword_hash(k)),
        PatternKey::Symbol(sid) => ConstRef::Symbol(*sid),
    }
}

/// The records a `StructRest` carries as its excluded keys, one per key the
/// pattern names.
pub(super) fn excluded_keys<'k>(keys: impl IntoIterator<Item = &'k PatternKey>) -> Vec<ConstRec> {
    keys.into_iter()
        .map(|k| ConstRec::immediate(pattern_key_const(k)))
        .collect()
}

/// What a decision tree has read on the way to the node being lowered: the
/// scrutinee's slot, and a slot holding the view of each value a switch above
/// tested (docs/macros/syntax.md).
///
/// A switch views its value once and pushes the slot, and every access below
/// it steps into that view. A switch's slot is stored in a block that
/// dominates its subtree, so a load in the subtree may read it.
pub(super) struct Viewed {
    pub(super) scrutinee_slot: u16,
    views: Vec<(AccessPath, u16)>,
}

impl Viewed {
    pub(super) fn new(scrutinee_slot: u16) -> Self {
        Viewed {
            scrutinee_slot,
            views: Vec::new(),
        }
    }

    /// Record that `slot` holds the view of the value at `access`, for the
    /// subtree lowered until the matching [`pop`](Self::pop).
    pub(super) fn push(&mut self, access: &AccessPath, slot: u16) {
        self.views.push((access.clone(), slot));
    }

    pub(super) fn pop(&mut self) {
        self.views.pop();
    }

    fn slot_of(&self, access: &AccessPath) -> Option<u16> {
        self.views
            .iter()
            .rev()
            .find(|(viewed, _)| viewed == access)
            .map(|(_, slot)| *slot)
    }
}

impl<'a> Lowerer<'a> {
    /// What `match` reads the value in `src` as: a syntax object's view, and
    /// any other value unchanged (docs/macros/syntax.md).
    pub(super) fn emit_syntax_view(&mut self, src: Reg) -> Reg {
        let dst = self.fresh_reg();
        self.emit(InstrRef::SyntaxView { dst, src });
        dst
    }

    /// Load the value at `access`, as a name bound there holds it.
    ///
    /// Each step reads into its parent's view, which the switch that tested
    /// the parent left in a slot.
    pub(super) fn load_access_path(
        &mut self,
        access: &AccessPath,
        viewed: &Viewed,
    ) -> Result<Reg, String> {
        match access {
            AccessPath::Root => {
                let dst = self.fresh_reg();
                self.emit(InstrRef::LoadLocal {
                    dst,
                    slot: viewed.scrutinee_slot,
                });
                Ok(dst)
            }
            AccessPath::First(inner) => {
                let parent = self.load_view(inner, viewed)?;
                let dst = self.fresh_reg();
                self.emit(InstrRef::First { dst, pair: parent });
                Ok(dst)
            }
            AccessPath::Rest(inner) => {
                let parent = self.load_view(inner, viewed)?;
                let dst = self.fresh_reg();
                self.emit(InstrRef::Rest { dst, pair: parent });
                Ok(dst)
            }
            AccessPath::Index(inner, idx) => {
                let parent = self.load_view(inner, viewed)?;
                let dst = self.fresh_reg();
                self.emit(InstrRef::ArrayMutRefDestructure {
                    dst,
                    src: parent,
                    index: *idx as u16,
                });
                Ok(dst)
            }
            AccessPath::Slice(inner, start, kind) => {
                let parent = self.load_view(inner, viewed)?;
                let dst = self.fresh_reg();
                self.emit(InstrRef::array_slice(*kind, dst, parent, *start as u16));
                Ok(dst)
            }
            AccessPath::Key(inner, key) => {
                let parent = self.load_view(inner, viewed)?;
                let dst = self.fresh_reg();
                self.emit(InstrRef::StructGetOrNil {
                    dst,
                    src: parent,
                    key: pattern_key_const(key),
                });
                Ok(dst)
            }
            AccessPath::Has(inner, key) => {
                let parent = self.load_view(inner, viewed)?;
                let dst = self.fresh_reg();
                self.emit(InstrRef::StructHasKey {
                    dst,
                    src: parent,
                    key: pattern_key_const(key),
                });
                Ok(dst)
            }
            AccessPath::StructRest(inner, exclude_keys) => {
                let src_reg = self.load_view(inner, viewed)?;
                let rest_reg = self.fresh_reg();
                let keys = excluded_keys(exclude_keys);
                self.emit(InstrRef::StructRest {
                    dst: rest_reg,
                    src: src_reg,
                    exclude_keys: ConstList::new(&keys),
                });
                Ok(rest_reg)
            }
        }
    }

    /// Load the view of the value at `access`: the slot a switch above left,
    /// or a view taken here.
    fn load_view(&mut self, access: &AccessPath, viewed: &Viewed) -> Result<Reg, String> {
        if let Some(slot) = viewed.slot_of(access) {
            let dst = self.fresh_reg();
            self.emit(InstrRef::LoadLocal { dst, slot });
            return Ok(dst);
        }
        let value = self.load_access_path(access, viewed)?;
        Ok(self.emit_syntax_view(value))
    }
}
