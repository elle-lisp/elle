// audited: 2026-11-18
//! The child table a compile reads `MakeClosure` headers from, as a type apart from the values a `ValueConst` loads.
//!
//! docs/impl/jit-closure.md

use crate::lir::ChildIndex;
use crate::value::{ClosureTemplate, Value};

/// The headers a function's `MakeClosure`s build over: its code payload's
/// child table. The compile reads each header as two words and never
/// follows them.
#[derive(Clone, Copy, Debug, Default)]
pub struct ChildTable<'a>(&'a [Value]);

impl<'a> ChildTable<'a> {
    /// The table of a function that builds no closure.
    pub const EMPTY: ChildTable<'static> = ChildTable(&[]);

    /// `headers`, in the order the emitter pushed them.
    pub fn new(headers: &'a [Value]) -> Self {
        ChildTable(headers)
    }

    /// The child table of `template`'s code payload.
    pub fn of(template: &'a ClosureTemplate) -> Self {
        ChildTable(template.payload().children())
    }

    /// The header at `index`, or `None` past the table's end.
    pub fn header(&self, index: ChildIndex) -> Option<Value> {
        self.0.get(index.get()).copied()
    }

    /// A copy owned by nobody else, for the worker's thread.
    pub fn to_owned(&self) -> OwnedChildTable {
        OwnedChildTable(self.0.to_vec())
    }
}

/// A child table copied out of the pages that hold it.
#[derive(Clone, Debug, Default)]
pub struct OwnedChildTable(Vec<Value>);

impl OwnedChildTable {
    pub fn view(&self) -> ChildTable<'_> {
        ChildTable(&self.0)
    }
}
