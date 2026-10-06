// audited: 2026-10-06
//! `CodeBuilder`: a code object written by hand, for the tests and tools that need one no compile produced.
//!
//! docs/impl/region/template.md
//!
//! Every field starts empty and a setter fills it, so a test names exactly the
//! fields its claim is about. A compile never comes through here: the emitter
//! writes its payloads from the LIR it froze.

use std::rc::Rc;

use crate::error::LocationMap;
use crate::hir::region::{RuntimeRegion, StaticRegion};
use crate::hir::VarargKind;
use crate::signals::Signal;
use crate::value::fiberheap::FiberHeap;
use crate::value::types::Arity;
use crate::value::{CaptureMask, Value};

use super::{CodeUnit, TemplateRef};

/// One code object's fields, before they are written into a code region.
pub struct CodeBuilder {
    proto: super::TemplateProto,
}

impl CodeBuilder {
    /// A code object running `bytecode` over `constants`, every other field
    /// empty.
    pub fn new(bytecode: Vec<u8>, arity: Arity, constants: Vec<Value>) -> Self {
        CodeBuilder {
            proto: super::TemplateProto::new(bytecode, arity, constants),
        }
    }

    /// The nullary code object a hand-emitted buffer runs as: its bytecode,
    /// constants, locations, signal and region tables.
    pub fn from_bytecode(bc: crate::compiler::Bytecode) -> Self {
        CodeBuilder {
            proto: bc.into_proto(),
        }
    }

    pub fn name(mut self, name: &str) -> Self {
        self.proto.name = Some(name.to_string());
        self
    }

    pub fn doc(mut self, doc: &str) -> Self {
        self.proto.doc = Some(doc.to_string());
        self
    }

    pub fn origin(mut self, origin: crate::syntax::Span) -> Self {
        self.proto.origin = Some(origin);
        self
    }

    pub fn location_map(mut self, map: LocationMap) -> Self {
        self.proto.location_map = map;
        self
    }

    pub fn num_locals(mut self, n: usize) -> Self {
        self.proto.num_locals = n;
        self
    }

    pub fn num_captures(mut self, n: usize) -> Self {
        self.proto.num_captures = n;
        self
    }

    pub fn num_params(mut self, n: usize) -> Self {
        self.proto.num_params = n;
        self
    }

    pub fn signal(mut self, signal: Signal) -> Self {
        self.proto.signal = signal;
        self
    }

    pub fn capture_params_mask(mut self, mask: u64) -> Self {
        self.proto.capture_params_mask = mask;
        self
    }

    pub fn capture_locals_mask(mut self, mask: CaptureMask) -> Self {
        self.proto.capture_locals_mask = mask;
        self
    }

    pub fn vararg_kind(mut self, kind: VarargKind) -> Self {
        self.proto.vararg_kind = kind;
        self
    }

    pub fn rest_list_layout(mut self, layout: super::RestListLayout) -> Self {
        self.proto.rest_list_layout = layout;
        self
    }

    pub fn region_table(mut self, table: Vec<StaticRegion>) -> Self {
        self.proto.region_table = table;
        self
    }

    pub fn merged_slots(mut self, slots: Vec<u32>) -> Self {
        self.proto.merged_slots = slots.into_iter().collect();
        self
    }

    pub fn frame_release_slots(mut self, slots: Vec<u16>) -> Self {
        self.proto.frame_release_slots = slots;
        self
    }

    pub fn frame_release_regions(mut self, regions: Vec<u32>) -> Self {
        self.proto.frame_release_regions = regions;
        self
    }

    /// The module function-table index a WASM-built closure dispatches to.
    pub fn wasm_func_idx(mut self, idx: u32) -> Self {
        self.proto.wasm_func_idx = Some(idx);
        self
    }

    /// The frozen function the JIT promotes this code object from.
    pub fn lir(mut self, lir: crate::lir::LirOwned) -> Self {
        self.proto.lir_function = Some(Rc::new(lir));
        self
    }

    /// The code objects this one's `MakeClosure` instructions index, in
    /// instruction order.
    pub fn children(mut self, children: Vec<CodeBuilder>) -> Self {
        self.proto.child_protos = children.into_iter().map(|c| Rc::new(c.proto)).collect();
        self
    }

    /// Write the code object into a fresh code region of `heap`, and name its
    /// header there. The region keeps the reference it was minted with, so it
    /// lives as long as the heap.
    pub fn build(self, heap: &mut FiberHeap) -> TemplateRef {
        let region = heap.new_runtime_region();
        TemplateRef::region(self.build_in(heap, region))
    }

    /// Write the code object into a fresh code region of `heap`, and allocate
    /// its header in `region`, as `MakeClosure` does. The header is the code
    /// region's only holder, so freeing `region` frees the code too.
    pub fn build_in(self, heap: &mut FiberHeap, region: RuntimeRegion) -> Value {
        super::materialize(heap, &Rc::new(self.proto), region)
    }

    /// Write the code object into a fresh code region of `heap` as the entry of
    /// a unit, which a VM on that heap runs.
    pub fn unit(self, heap: &mut FiberHeap) -> CodeUnit {
        let p = self.proto;
        let bytecode = crate::compiler::Bytecode {
            instructions: p.bytecode,
            constants: p.constants,
            location_map: p.location_map,
            signal: p.signal,
            signal_projection: None,
            child_protos: p.child_protos,
            merged_slots: p.merged_slots,
            frame_release_slots: p.frame_release_slots,
            frame_release_regions: p.frame_release_regions,
        };
        CodeUnit::new(super::CodeArena::mint(heap), bytecode)
    }
}
