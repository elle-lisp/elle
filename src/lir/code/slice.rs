// audited: 2026-11-25
//! The instructions that test for and slice an array of one mutability.
//!
//! docs/impl/region/anchors.md

use super::instr::InstrRef;
use crate::lir::Reg;
use crate::value::Mutability;

impl InstrRef<'_> {
    /// The test that `src` is an array of `kind`.
    pub fn is_array(kind: Mutability, dst: Reg, src: Reg) -> Self {
        match kind {
            Mutability::Immutable => InstrRef::IsArray { dst, src },
            Mutability::Mutable => InstrRef::IsArrayMut { dst, src },
        }
    }

    /// The slice an array rest takes of an array of `kind`: a fresh array of
    /// that kind holding the elements from `index` on.
    pub fn array_slice(kind: Mutability, dst: Reg, src: Reg, index: u16) -> Self {
        match kind {
            Mutability::Immutable => InstrRef::ArraySliceFrom { dst, src, index },
            Mutability::Mutable => InstrRef::ArrayMutSliceFrom { dst, src, index },
        }
    }
}
