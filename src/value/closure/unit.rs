// audited: 2026-10-06
//! `CodeUnit`: what a compile hands out — its entry code object, and the code region every payload it built lives in.
//!
//! docs/impl/region/template.md

use std::collections::HashMap;

use crate::signals::Signal;

use super::{ClosureTemplate, CodeArena};

/// One compiled unit: the entry function, the lambdas it nests, and the
/// signal projection a file compile computes for its importers.
pub struct CodeUnit {
    bytecode: crate::compiler::Bytecode,
    entry: ClosureTemplate,
}

impl std::fmt::Debug for CodeUnit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CodeUnit")
            .field("entry", &self.entry)
            .finish()
    }
}

impl CodeUnit {
    /// The unit an emission into `arena` produced, entered through
    /// `bytecode`, the entry function's buffer.
    pub fn new(arena: CodeArena, bytecode: crate::compiler::Bytecode) -> Self {
        let heap = unsafe { &mut *arena.heap_ptr() };
        let entry =
            ClosureTemplate::for_proto(heap, &std::rc::Rc::new(bytecode.clone().into_proto()));
        CodeUnit { bytecode, entry }
    }

    /// The entry function's emitted buffer.
    pub(crate) fn bytecode(&self) -> &crate::compiler::Bytecode {
        &self.bytecode
    }

    /// The entry function's code object: a nullary function with no LIR,
    /// whose child table holds the lambdas the entry builds.
    pub fn entry(&self) -> &ClosureTemplate {
        &self.entry
    }

    /// Keyword field name → signal of each exported closure, when the unit is
    /// a file whose value is a projectable struct.
    pub fn signal_projection(&self) -> Option<&HashMap<String, Signal>> {
        self.bytecode.signal_projection.as_ref()
    }
}
