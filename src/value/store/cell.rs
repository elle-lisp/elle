// audited: 2026-10-22
//! The box and capture-cell store: one value, copied out on a read.
//!
//! docs/impl/mutable.md

use std::cell::RefCell;
use std::rc::Rc;

use crate::value::Value;

/// The value a box or a capture cell holds. A read copies it out, so a cell
/// has no read guard.
pub struct CellStore(Rc<RefCell<Value>>);

impl CellStore {
    /// A store holding `v`.
    pub fn new(v: Value) -> Self {
        CellStore(Rc::new(RefCell::new(v)))
    }

    /// The value the cell holds now.
    #[inline]
    pub fn get(&self) -> Value {
        *self.0.borrow()
    }

    /// Store `v` and answer the value it displaced.
    pub(in crate::value) fn replace(&self, v: Value) -> Value {
        self.0.replace(v)
    }
}
