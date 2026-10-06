// audited: 2026-10-06
//! `CodeArena`: a heap and the code region on it that one compile unit writes its payloads into.
//!
//! docs/impl/region/template.md

use crate::hir::region::RuntimeRegion;
use crate::value::fiberheap::FiberHeap;

/// The allocation target a compile writes its code objects into: a heap and
/// one code region on it.
///
/// `Copy`, like a `SyntaxArena`, so it threads through the emitter without
/// borrowing anything. It names a region; it does not own one — the
/// `CodeUnit` built over it does.
#[derive(Clone, Copy)]
pub struct CodeArena {
    heap: *mut FiberHeap,
    region: Option<RuntimeRegion>,
}

impl CodeArena {
    /// A code region on `heap`, named for one compile unit.
    pub fn mint(heap: &mut FiberHeap) -> Self {
        CodeArena {
            heap: heap as *mut FiberHeap,
            region: None,
        }
    }

    /// The heap this arena's region lives on.
    pub fn heap_ptr(&self) -> *mut FiberHeap {
        self.heap
    }
}

impl std::fmt::Debug for CodeArena {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "CodeArena({:?})", self.region)
    }
}
