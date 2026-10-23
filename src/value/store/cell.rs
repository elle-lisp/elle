// audited: 2026-10-22
//! The box and capture-cell store: one value inside the shell, copied out on a read.
//!
//! docs/impl/mutable.md

use std::cell::Cell;

use crate::value::Value;

/// The value a box or a capture cell holds, inline in its shell. A read copies
/// it out, so a cell has no read guard and no borrow count.
pub struct CellStore(Cell<Value>);

impl CellStore {
    /// A store holding `v`.
    pub fn new(v: Value) -> Self {
        CellStore(Cell::new(v))
    }

    /// The value the cell holds now.
    #[inline]
    pub fn get(&self) -> Value {
        self.0.get()
    }

    /// Store `v` and answer the value it displaced.
    pub(in crate::value) fn replace(&self, v: Value) -> Value {
        self.0.replace(v)
    }
}
