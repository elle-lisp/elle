// audited: 2026-10-06
//! `CodeArena` and `PayloadParts`: one compile unit's code region, and the fields a payload is written into it from.
//!
//! docs/impl/region/template.md

use crate::error::LocationMap;
use crate::hir::region::{RuntimeRegion, StaticRegion};
use crate::hir::VarargKind;
use crate::signals::Signal;
use crate::value::fiberheap::FiberHeap;
use crate::value::heap::HeapObject;
use crate::value::region_slice::RegionSlice;
use crate::value::types::Arity;
use crate::value::{CaptureMask, Value};

use super::header::ClosureTemplate;
use super::payload::{CodePayload, LocEntry, RestListLayout, VarargTag};

/// The allocation target a compile writes its code objects into: a heap and
/// one code region on it.
///
/// `Copy`, like a `SyntaxArena`, so it threads through the emitter without
/// borrowing anything. It names a region; it does not own one. The region's
/// birth reference belongs to the `CodeUnit` built over the arena.
#[derive(Clone, Copy)]
pub struct CodeArena {
    heap: *mut FiberHeap,
    region: RuntimeRegion,
}

impl CodeArena {
    /// A fresh code region on `heap`, for one compile unit.
    pub fn mint(heap: &mut FiberHeap) -> Self {
        let region = heap.new_runtime_region();
        CodeArena {
            heap: heap as *mut FiberHeap,
            region,
        }
    }

    /// The code region on `heap` named `region`, for a writer that holds the
    /// region some other way: the root region, an image's.
    pub(crate) fn over(heap: &mut FiberHeap, region: RuntimeRegion) -> Self {
        CodeArena {
            heap: heap as *mut FiberHeap,
            region,
        }
    }

    /// The heap this arena's region lives on.
    pub fn heap_ptr(&self) -> *mut FiberHeap {
        self.heap
    }

    /// The code region.
    pub fn region(&self) -> RuntimeRegion {
        self.region
    }

    /// Write `parts` as a payload in the code region.
    pub fn write(&self, parts: PayloadParts) -> RegionSlice<CodePayload> {
        parts.write(unsafe { &mut *self.heap }, self.region)
    }

    /// Allocate a header over `payload` in the code region: the shape a child
    /// table holds. A self-edge, so it takes no reference of its own.
    pub fn header(&self, payload: RegionSlice<CodePayload>) -> Value {
        unsafe { &mut *self.heap }.alloc_in_region(
            HeapObject::ClosureTemplate(ClosureTemplate::new(payload)),
            self.region,
        )
    }
}

impl std::fmt::Debug for CodeArena {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "CodeArena({})", self.region)
    }
}

/// The shape of the frame a WASM closure runs in, as the `MakeClosure` site
/// supplies it: what a lambda's own frozen LIR would answer, read out of
/// linear memory instead because the host has no LIR to ask.
///
/// A struct rather than eight arguments: the three counts and the two masks are
/// each a bare integer, and at a call site of eight positions a pair of them
/// swapped type-checks and runs.
pub struct WasmClosureMeta {
    pub arity: Arity,
    pub num_locals: usize,
    pub num_captures: usize,
    pub num_params: usize,
    pub signal: Signal,
    pub capture_params_mask: u64,
    pub capture_locals_mask: CaptureMask,
    /// The module function-table index of this closure's compiled body, which
    /// is how `rt_call` dispatches to it.
    pub wasm_func_idx: u32,
}

/// Every field of one code object, owned, before it is written into a code
/// region. Each writer of a payload builds one through a constructor named
/// for it, so no field is a writer's to remember.
pub struct PayloadParts {
    pub(crate) bytecode: Vec<u8>,
    pub(crate) arity: Arity,
    pub(crate) constants: Vec<Value>,
    pub(crate) num_locals: usize,
    pub(crate) num_captures: usize,
    pub(crate) num_params: usize,
    pub(crate) signal: Signal,
    pub(crate) capture_params_mask: u64,
    pub(crate) capture_locals_mask: CaptureMask,
    pub(crate) location_map: LocationMap,
    pub(crate) lir: Option<crate::lir::LirOwned>,
    pub(crate) doc: Option<String>,
    pub(crate) origin: Option<crate::syntax::Span>,
    pub(crate) vararg_kind: VarargKind,
    /// How a `&` rest list is built, as the region analysis proved it may be
    /// (docs/impl/region/restlist.md). Only meaningful when `vararg_kind` is
    /// `List`.
    pub(crate) rest_list_layout: RestListLayout,
    pub(crate) name: Option<String>,
    pub(crate) wasm_func_idx: Option<u32>,
    pub(crate) region_table: Vec<StaticRegion>,
    pub(crate) merged_slots: Vec<u32>,
    pub(crate) frame_release_slots: Vec<u16>,
    pub(crate) frame_release_regions: Vec<u32>,
    /// The headers this code object's `MakeClosure` instructions index, in
    /// instruction order.
    pub(crate) children: Vec<Value>,
}

impl PayloadParts {
    /// A code object running `bytecode` over `constants`, every other field
    /// empty.
    pub fn new(bytecode: Vec<u8>, arity: Arity, constants: Vec<Value>) -> Self {
        PayloadParts {
            bytecode,
            arity,
            constants,
            num_locals: 0,
            num_captures: 0,
            num_params: 0,
            signal: Signal::silent(),
            capture_params_mask: 0,
            capture_locals_mask: CaptureMask::empty(),
            location_map: LocationMap::new(),
            lir: None,
            doc: None,
            origin: None,
            vararg_kind: VarargKind::List,
            rest_list_layout: RestListLayout::PerCell,
            name: None,
            wasm_func_idx: None,
            region_table: Vec::new(),
            merged_slots: Vec::new(),
            frame_release_slots: Vec::new(),
            frame_release_regions: Vec::new(),
            children: Vec::new(),
        }
    }

    /// The entry function of a unit: the emitter's buffer for it, nullary,
    /// with no LIR.
    pub fn entry(bc: crate::compiler::Bytecode) -> Self {
        PayloadParts {
            signal: bc.signal,
            location_map: bc.location_map,
            merged_slots: bc.merged_slots,
            frame_release_slots: bc.frame_release_slots,
            frame_release_regions: bc.frame_release_regions,
            children: bc.children,
            ..PayloadParts::new(bc.instructions, Arity::Exact(0), bc.constants)
        }
    }

    /// A nested lambda: everything `lir` knows about itself, plus the bytecode
    /// its own emission produced (docs/impl/region/template.md § "One
    /// constructor builds a nested lambda's payload").
    ///
    /// `num_captures` comes from the instruction rather than from `lir`: what
    /// a lambda closes over is decided at the site that builds it.
    pub fn lambda(
        lir: &crate::lir::LirOwned,
        num_captures: usize,
        compiled: crate::lir::ClosureCompiled,
    ) -> Self {
        let (bytecode, yield_points, call_sites) = compiled;
        // The LIR the JIT promotes this lambda from is the frozen function plus
        // the sites only emission can supply.
        let mut lir = lir.clone();
        lir.set_sites(&yield_points, &call_sites);
        let func = lir.view();
        let parts = PayloadParts {
            num_locals: func.num_locals() as usize,
            num_captures,
            num_params: func.num_params(),
            signal: func.signal(),
            capture_params_mask: func.capture_params_mask(),
            capture_locals_mask: CaptureMask::from_words(
                func.capture_locals_mask().words().to_vec(),
            ),
            location_map: bytecode.location_map,
            doc: func.doc().map(str::to_string),
            origin: func.origin(),
            vararg_kind: func.vararg_kind(),
            rest_list_layout: func.rest_list_layout(),
            name: func.name().map(str::to_string),
            region_table: func.region_table().to_vec(),
            merged_slots: func.merged_slots().iter().map(|s| s.get()).collect(),
            frame_release_slots: func.frame_release_slots().to_vec(),
            frame_release_regions: func
                .frame_release_regions()
                .iter()
                .map(|r| r.get())
                .collect(),
            children: bytecode.children,
            ..PayloadParts::new(bytecode.instructions, func.arity(), bytecode.constants)
        };
        PayloadParts {
            lir: Some(lir),
            ..parts
        }
    }

    /// A closure a compiled WASM module builds: the code half off `code`, the
    /// module's own payload for the closure, and the shape half off the call
    /// (docs/impl/region/template.md § "The WASM backend copies the module's
    /// own payload"). `None` is a module carrying no payload, whose code object
    /// has no bytecode to run and takes its shape from `meta` alone.
    pub fn wasm_closure(code: Option<&ClosureTemplate>, meta: WasmClosureMeta) -> Self {
        let shape = |parts: PayloadParts| PayloadParts {
            num_locals: meta.num_locals,
            num_captures: meta.num_captures,
            num_params: meta.num_params,
            signal: meta.signal,
            capture_params_mask: meta.capture_params_mask,
            capture_locals_mask: meta.capture_locals_mask,
            wasm_func_idx: Some(meta.wasm_func_idx),
            ..parts
        };
        match code {
            None => shape(PayloadParts::new(Vec::new(), meta.arity, Vec::new())),
            Some(code) => shape(PayloadParts {
                location_map: code.location_map(),
                merged_slots: code.merged_slots().as_slice().to_vec(),
                frame_release_slots: code.frame_release_slots().to_vec(),
                frame_release_regions: code.frame_release_regions().to_vec(),
                children: code.payload().children().to_vec(),
                ..PayloadParts::new(
                    code.bytecode().to_vec(),
                    meta.arity,
                    code.constants().to_vec(),
                )
            }),
        }
    }

    /// Every field of an existing payload, for a copy onto another heap. The
    /// child table is the caller's to rewrite, because the copy's children
    /// are copies too.
    pub(crate) fn of_payload(t: &ClosureTemplate) -> Self {
        PayloadParts {
            num_locals: t.num_locals(),
            num_captures: t.num_captures(),
            num_params: t.num_params(),
            signal: t.signal(),
            capture_params_mask: t.capture_params_mask(),
            capture_locals_mask: t.owned_capture_locals_mask(),
            location_map: t.location_map(),
            lir: t.lir().map(|l| l.to_owned()),
            doc: t.doc().map(str::to_string),
            origin: t.origin(),
            vararg_kind: t.vararg_kind(),
            rest_list_layout: t.rest_list_layout(),
            name: t.name().map(str::to_string),
            wasm_func_idx: t.wasm_func_idx(),
            region_table: t.region_table().to_vec(),
            merged_slots: t.merged_slots().as_slice().to_vec(),
            frame_release_slots: t.frame_release_slots().to_vec(),
            frame_release_regions: t.frame_release_regions().to_vec(),
            children: t.payload().children().to_vec(),
            ..PayloadParts::new(t.bytecode().to_vec(), t.arity(), t.constants().to_vec())
        }
    }

    /// The vararg tag the payload carries.
    fn vararg_tag(&self) -> VarargTag {
        match self.vararg_kind {
            VarargKind::List => VarargTag::List,
            VarargKind::Struct => VarargTag::Struct,
            VarargKind::StrictStruct(_) => VarargTag::StrictStruct,
        }
    }

    /// Write the payload into `region`. Every slice lands in the one region, so
    /// a header's reference to any of them resolves to the same region id and
    /// one counted edge covers the whole payload.
    pub(crate) fn write(
        self,
        heap: &mut FiberHeap,
        region: RuntimeRegion,
    ) -> RegionSlice<CodePayload> {
        let bytecode = heap.alloc_region_slice_in_region::<u8>(&self.bytecode, region);
        let constants = heap.alloc_region_slice_in_region::<Value>(&self.constants, region);

        // Intern each distinct file name once, then record every location as an
        // index into that table. The emitter's map is a hash map, so the entries
        // are sorted here: a binary search over an unsorted table answers wrongly.
        let mut sorted: Vec<(&usize, &crate::reader::SourceLoc)> =
            self.location_map.iter().collect();
        sorted.sort_unstable_by_key(|(off, _)| **off);
        let mut file_names: Vec<&str> = Vec::new();
        let mut entries: Vec<LocEntry> = Vec::with_capacity(sorted.len());
        for (off, loc) in sorted {
            let file = match file_names.iter().position(|f| *f == loc.file.as_str()) {
                Some(ix) => ix,
                None => {
                    file_names.push(loc.file.as_str());
                    file_names.len() - 1
                }
            };
            entries.push(LocEntry {
                offset: *off as u32,
                file: file as u32,
                line: loc.line as u32,
                col: loc.col as u32,
            });
        }
        let files: Vec<RegionSlice<u8>> = file_names
            .iter()
            .map(|f| heap.alloc_region_slice_in_region::<u8>(f.as_bytes(), region))
            .collect();
        let files = heap.alloc_region_slice_in_region::<RegionSlice<u8>>(&files, region);
        let locations = heap.alloc_region_slice_in_region::<LocEntry>(&entries, region);

        let name = region_str(heap, self.name.as_deref(), region);
        let doc = region_str(heap, self.doc.as_deref(), region);
        let region_table =
            heap.alloc_region_slice_in_region::<StaticRegion>(&self.region_table, region);

        // The merge set is stored ascending: the alloc dispatch resolves
        // membership by binary search, so an unsorted set would answer wrongly
        // rather than slowly. This is where that invariant is established, so it
        // is where it is checked — the reader on the hot path takes it on trust.
        let mut merged = self.merged_slots.clone();
        merged.sort_unstable();
        merged.dedup();
        let merged_slots = heap.alloc_region_slice_in_region::<u32>(&merged, region);

        // Both release tables are stored ascending: a value route's identity is
        // its slot, so an abandoned frame's walk reads them in a fixed order
        // whatever order the lowerer discovered the releases in.
        let mut release_slots = self.frame_release_slots.clone();
        release_slots.sort_unstable();
        let frame_release_slots = heap.alloc_region_slice_in_region::<u16>(&release_slots, region);
        let mut release_regions = self.frame_release_regions.clone();
        release_regions.sort_unstable();
        let frame_release_regions =
            heap.alloc_region_slice_in_region::<u32>(&release_regions, region);
        let capture_locals =
            heap.alloc_region_slice_in_region::<u64>(self.capture_locals_mask.words(), region);

        let strict_keys = match &self.vararg_kind {
            VarargKind::StrictStruct(keys) => {
                let keys: Vec<RegionSlice<u8>> = keys
                    .iter()
                    .map(|k| heap.alloc_region_slice_in_region::<u8>(k.as_bytes(), region))
                    .collect();
                heap.alloc_region_slice_in_region::<RegionSlice<u8>>(&keys, region)
            }
            _ => RegionSlice::empty(),
        };
        let children = heap.alloc_region_slice_in_region::<Value>(&self.children, region);

        // The frozen function lands beside the rest of the payload, so every
        // reader of the code object — the JIT, an image, `send` — reads it here
        // (docs/impl/lir.md).
        let lir = match &self.lir {
            Some(lir) => crate::lir::LirBody::build(heap, &lir.view(), region),
            None => crate::lir::LirBody::empty(),
        };

        let payload = CodePayload {
            bytecode,
            constants,
            locations,
            files,
            name: name.0,
            doc: doc.0,
            region_table,
            merged_slots,
            frame_release_slots,
            frame_release_regions,
            capture_locals,
            strict_keys,
            children,
            origin: self.origin.unwrap_or_else(crate::syntax::Span::synthetic),
            has_origin: self.origin.is_some(),
            lir,
            has_lir: self.lir.is_some(),
            arity: self.arity,
            signal: self.signal,
            capture_params_mask: self.capture_params_mask,
            num_locals: self.num_locals as u32,
            num_captures: self.num_captures as u32,
            num_params: self.num_params as u32,
            wasm_func_idx: self.wasm_func_idx.unwrap_or(0),
            has_wasm_idx: self.wasm_func_idx.is_some(),
            vararg: self.vararg_tag(),
            rest_list: self.rest_list_layout,
            has_name: name.1,
            has_doc: doc.1,
        };
        heap.alloc_region_slice_in_region::<CodePayload>(&[payload], region)
    }
}

impl FiberHeap {
    /// This instance's placeholder code object: a nullary body of a single
    /// `Return` that is never executed.
    ///
    /// A fiber that runs no bytecode still names a code object — the root
    /// fiber, whose execution context is top-level bytecode rather than a
    /// closure, and a native-iterator fiber, which the resume path
    /// short-circuits. The payload and its header live in the pinned root
    /// region, a process root, so the placeholder lives as long as the
    /// instance (docs/impl/region/template.md).
    pub(crate) fn placeholder_template(&mut self) -> super::TemplateRef {
        if let Some(header) = self.placeholder_slot() {
            return super::TemplateRef::region(header);
        }
        let root = crate::value::arena::root_region(self);
        let arena = CodeArena::over(self, root);
        let ret = crate::compiler::bytecode::Instruction::Return as u8;
        let payload = arena.write(PayloadParts::new(
            vec![ret, 0, 0, 0],
            Arity::Exact(0),
            Vec::new(),
        ));
        let header = arena.header(payload);
        self.set_placeholder(Some(header));
        super::TemplateRef::region(header)
    }
}

/// Copy an optional string into `region`, reporting whether it was present at
/// all: an absent docstring and an empty one are different answers, and both
/// are empty slices.
fn region_str(
    heap: &mut FiberHeap,
    s: Option<&str>,
    region: RuntimeRegion,
) -> (RegionSlice<u8>, bool) {
    match s {
        Some(s) => (
            heap.alloc_region_slice_in_region::<u8>(s.as_bytes(), region),
            true,
        ),
        None => (RegionSlice::empty(), false),
    }
}
