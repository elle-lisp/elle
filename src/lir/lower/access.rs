// audited: 2026-10-06
//! Loading a value along a pattern's access path, and the constants a struct pattern's keys lower to.
//!
//! docs/impl/lir.md
//!
//! An access path is a chain of field accesses (car, cdr, array index, struct
//! key) from the scrutinee root.

use super::*;
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

impl<'a> Lowerer<'a> {
    /// Load a value by following an access path from the scrutinee.
    ///
    /// Recursively navigates the access path, emitting the appropriate
    /// destructuring instruction at each step.
    pub(super) fn load_access_path(
        &mut self,
        access: &crate::hir::decision::AccessPath,
        scrutinee_slot: u16,
    ) -> Result<Reg, String> {
        use crate::hir::decision::AccessPath;
        match access {
            AccessPath::Root => {
                let dst = self.fresh_reg();
                self.emit(InstrRef::LoadLocal {
                    dst,
                    slot: scrutinee_slot,
                });
                Ok(dst)
            }
            AccessPath::First(inner) => {
                let parent = self.load_access_path(inner, scrutinee_slot)?;
                let dst = self.fresh_reg();
                self.emit(InstrRef::First { dst, pair: parent });
                Ok(dst)
            }
            AccessPath::Rest(inner) => {
                let parent = self.load_access_path(inner, scrutinee_slot)?;
                let dst = self.fresh_reg();
                self.emit(InstrRef::Rest { dst, pair: parent });
                Ok(dst)
            }
            AccessPath::Index(inner, idx) => {
                let parent = self.load_access_path(inner, scrutinee_slot)?;
                let dst = self.fresh_reg();
                self.emit(InstrRef::ArrayMutRefDestructure {
                    dst,
                    src: parent,
                    index: *idx as u16,
                });
                Ok(dst)
            }
            AccessPath::Slice(inner, start) => {
                let parent = self.load_access_path(inner, scrutinee_slot)?;
                let dst = self.fresh_reg();
                self.emit(InstrRef::ArrayMutSliceFrom {
                    dst,
                    src: parent,
                    index: *start as u16,
                });
                Ok(dst)
            }
            AccessPath::Key(inner, key) => {
                let parent = self.load_access_path(inner, scrutinee_slot)?;
                let dst = self.fresh_reg();
                self.emit(InstrRef::StructGetOrNil {
                    dst,
                    src: parent,
                    key: pattern_key_const(key),
                });
                Ok(dst)
            }
            AccessPath::StructRest(inner, exclude_keys) => {
                let src_reg = self.load_access_path(inner, scrutinee_slot)?;
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
}
