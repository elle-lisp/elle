// audited: 2026-11-09
//! `CaptureFact`: one captured slot's binding and whether code assigns it, as a payload record with every byte written.
//!
//! docs/impl/region/template.md
//! docs/modules-proposal/shape.md
//!
//! `fn/captures` reads one record per slot, and the purity walk refuses a slot
//! whose binding code assigns.

use crate::value::SymbolId;

/// One captured slot of a closure's environment: the binding the slot holds,
/// and whether code assigns it.
///
/// The name is a `SymbolId`'s word rather than the `SymbolId` itself, so the
/// record's layout is its fields, every byte written. The flag is a byte, so
/// a record an image maps holds no bit pattern a read could not take.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[repr(C)]
pub struct CaptureFact {
    pub(crate) name: u64,
    /// 1 when code assigns the binding, 0 when no code does.
    pub(crate) assigned: u8,
    /// Written zero.
    pub(crate) pad: [u8; 7],
}

impl CaptureFact {
    /// The record of a slot holding the binding `name`, which code assigns
    /// when `assigned`.
    pub fn new(name: SymbolId, assigned: bool) -> Self {
        CaptureFact {
            name: name.0,
            assigned: u8::from(assigned),
            pad: [0; 7],
        }
    }

    /// The captured binding's name.
    pub fn name(&self) -> SymbolId {
        SymbolId(self.name)
    }

    /// Whether any `assign` in the compile writes the binding.
    pub fn assigned(&self) -> bool {
        self.assigned != 0
    }
}
