//! Access path loading for pattern matching.
//!
//! Computes how to destructure a value by following a chain of field
//! accesses (car, cdr, array index, struct key) from the scrutinee root.

use super::*;
use crate::hir::PatternKey;

impl Lowerer {
    /// Load a value by following an access path from the scrutinee.
    ///
    /// Recursively navigates the access path, emitting the appropriate
    /// destructuring instruction at each step.
    pub(super) fn load_access_path(
        &mut self,
        access: &super::decision::AccessPath,
        scrutinee_slot: u16,
    ) -> Result<Reg, String> {
        use super::decision::AccessPath;
        match access {
            AccessPath::Root => {
                let dst = self.fresh_reg();
                self.emit(LirInstr::LoadLocal {
                    dst,
                    slot: scrutinee_slot,
                });
                Ok(dst)
            }
            AccessPath::Car(inner) => {
                let parent = self.load_access_path(inner, scrutinee_slot)?;
                let dst = self.fresh_reg();
                self.emit(LirInstr::Car { dst, pair: parent });
                Ok(dst)
            }
            AccessPath::Cdr(inner) => {
                let parent = self.load_access_path(inner, scrutinee_slot)?;
                let dst = self.fresh_reg();
                self.emit(LirInstr::Cdr { dst, pair: parent });
                Ok(dst)
            }
            AccessPath::Index(inner, idx) => {
                let parent = self.load_access_path(inner, scrutinee_slot)?;
                let dst = self.fresh_reg();
                self.emit(LirInstr::ArrayMutRefDestructure {
                    dst,
                    src: parent,
                    index: *idx as u16,
                });
                Ok(dst)
            }
            AccessPath::Slice(inner, start) => {
                let parent = self.load_access_path(inner, scrutinee_slot)?;
                let dst = self.fresh_reg();
                self.emit(LirInstr::ArrayMutSliceFrom {
                    dst,
                    src: parent,
                    index: *start as u16,
                });
                Ok(dst)
            }
            AccessPath::Key(inner, key) => {
                let parent = self.load_access_path(inner, scrutinee_slot)?;
                let dst = self.fresh_reg();
                let lir_key = match key {
                    PatternKey::Keyword(k) => LirConst::Keyword(k.clone()),
                    PatternKey::Symbol(sid) => LirConst::Symbol(*sid),
                };
                self.emit(LirInstr::StructGetOrNil {
                    dst,
                    src: parent,
                    key: lir_key,
                });
                Ok(dst)
            }
            AccessPath::StructRest(inner, exclude_keys) => {
                let src_reg = self.load_access_path(inner, scrutinee_slot)?;
                let rest_reg = self.fresh_reg();
                let lir_exclude: Vec<LirConst> = exclude_keys
                    .iter()
                    .map(|k| match k {
                        PatternKey::Keyword(s) => LirConst::Keyword(s.clone()),
                        PatternKey::Symbol(sid) => LirConst::Symbol(*sid),
                    })
                    .collect();
                self.emit(LirInstr::StructRest {
                    dst: rest_reg,
                    src: src_reg,
                    exclude_keys: lir_exclude,
                });
                Ok(rest_reg)
            }
        }
    }
}
