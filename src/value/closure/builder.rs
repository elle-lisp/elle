// audited: 2026-10-06
//! `CodeBuilder`: a code object written by hand, for the tests and tools that need one no compile produced.
//!
//! docs/impl/region/template.md
//!
//! Every field starts empty and a setter fills it, so a test names exactly the
//! fields its claim is about. A compile never comes through here: the emitter
//! writes its payloads from the LIR it froze.

use crate::error::LocationMap;
use crate::hir::region::{RuntimeRegion, StaticRegion};
use crate::hir::VarargKind;
use crate::signals::Signal;
use crate::value::fiberheap::FiberHeap;
use crate::value::types::Arity;
use crate::value::{CaptureMask, Value};

use super::{ClosureTemplate, CodeArena, CodeUnit, PayloadParts, TemplateRef};

/// One code object's fields, and its children's, before they are written into
/// a code region.
pub struct CodeBuilder {
    parts: PayloadParts,
    children: Vec<CodeBuilder>,
}

impl CodeBuilder {
    /// A code object running `bytecode` over `constants`, every other field
    /// empty.
    pub fn new(bytecode: Vec<u8>, arity: Arity, constants: Vec<Value>) -> Self {
        CodeBuilder {
            parts: PayloadParts::new(bytecode, arity, constants),
            children: Vec::new(),
        }
    }

    /// The nullary code object a hand-emitted buffer runs as: its bytecode,
    /// constants, locations, signal and region tables.
    pub fn from_bytecode(bc: crate::compiler::Bytecode) -> Self {
        CodeBuilder {
            parts: PayloadParts::entry(bc),
            children: Vec::new(),
        }
    }

    pub fn name(mut self, name: &str) -> Self {
        self.parts.name = Some(name.to_string());
        self
    }

    pub fn doc(mut self, doc: &str) -> Self {
        self.parts.doc = Some(doc.to_string());
        self
    }

    pub fn origin(mut self, origin: crate::syntax::Span) -> Self {
        self.parts.origin = Some(origin);
        self
    }

    pub fn location_map(mut self, map: LocationMap) -> Self {
        self.parts.location_map = map;
        self
    }

    pub fn num_locals(mut self, n: usize) -> Self {
        self.parts.num_locals = n;
        self
    }

    pub fn num_captures(mut self, n: usize) -> Self {
        self.parts.num_captures = n;
        self
    }

    pub fn num_params(mut self, n: usize) -> Self {
        self.parts.num_params = n;
        self
    }

    pub fn signal(mut self, signal: Signal) -> Self {
        self.parts.signal = signal;
        self
    }

    pub fn capture_params_mask(mut self, mask: u64) -> Self {
        self.parts.capture_params_mask = mask;
        self
    }

    pub fn capture_locals_mask(mut self, mask: CaptureMask) -> Self {
        self.parts.capture_locals_mask = mask;
        self
    }

    pub fn vararg_kind(mut self, kind: VarargKind) -> Self {
        self.parts.vararg_kind = kind;
        self
    }

    pub fn rest_list_layout(mut self, layout: super::RestListLayout) -> Self {
        self.parts.rest_list_layout = layout;
        self
    }

    pub fn region_table(mut self, table: Vec<StaticRegion>) -> Self {
        self.parts.region_table = table;
        self
    }

    pub fn merged_slots(mut self, slots: Vec<u32>) -> Self {
        self.parts.merged_slots = slots;
        self
    }

    pub fn frame_release_slots(mut self, slots: Vec<u16>) -> Self {
        self.parts.frame_release_slots = slots;
        self
    }

    pub fn frame_release_regions(mut self, regions: Vec<u32>) -> Self {
        self.parts.frame_release_regions = regions;
        self
    }

    /// The module function-table index a WASM-built closure dispatches to.
    pub fn wasm_func_idx(mut self, idx: u32) -> Self {
        self.parts.wasm_func_idx = Some(idx);
        self
    }

    /// The frozen function the JIT promotes this code object from.
    pub fn lir(mut self, lir: crate::lir::LirOwned) -> Self {
        self.parts.lir = Some(lir);
        self
    }

    /// The code objects this one's `MakeClosure` instructions index, in
    /// instruction order.
    pub fn children(mut self, children: Vec<CodeBuilder>) -> Self {
        self.children = children;
        self
    }

    /// Write this code object, its children first, into `arena`, and answer
    /// its payload.
    fn write(
        self,
        arena: CodeArena,
    ) -> crate::value::region_slice::RegionSlice<super::CodePayload> {
        arena.write(self.children_into(arena))
    }

    /// Write the code object into a fresh code region of `heap`, and name its
    /// header there. The region keeps the reference it was minted with, so it
    /// lives as long as the heap.
    pub fn build(self, heap: &mut FiberHeap) -> TemplateRef {
        let arena = CodeArena::mint(heap);
        TemplateRef::region(arena.header(self.write(arena)))
    }

    /// Write the code object into a fresh code region of `heap`, and allocate
    /// its header in `region`, as `MakeClosure` does. The header is the code
    /// region's only holder, so freeing `region` frees the code too.
    pub fn build_in(self, heap: &mut FiberHeap, region: RuntimeRegion) -> Value {
        let arena = CodeArena::mint(heap);
        let code = ClosureTemplate::new(self.write(arena));
        let header = crate::value::build::template(heap, &code, region);
        heap.decref_region(arena.region());
        header
    }

    /// Write the code object into a fresh code region of `heap` as the entry of
    /// a unit, which a VM on that heap runs.
    pub fn unit(self, heap: &mut FiberHeap) -> CodeUnit {
        let arena = CodeArena::mint(heap);
        let parts = self.children_into(arena);
        CodeUnit::of_parts(arena, parts, None)
    }

    /// This code object's fields, its children written into `arena` and
    /// named by its child table.
    fn children_into(self, arena: CodeArena) -> PayloadParts {
        let mut parts = self.parts;
        parts.children = self
            .children
            .into_iter()
            .map(|child| arena.header(child.write(arena)))
            .collect();
        parts
    }
}
