//! `TemplateProto` — a code object's compile-time blueprint.
//!
//! Plain Rust data the emitter builds, owned by whatever holds the compiled
//! program. It never enters a region; what it materializes does
//! (docs/impl/region/template.md § "Three things, not one").

use std::cell::OnceCell;
use std::rc::Rc;

use rustc_hash::FxHashSet;

use crate::error::LocationMap;
use crate::hir::region::{RuntimeRegion, StaticRegion};
use crate::hir::VarargKind;
use crate::signals::Signal;
use crate::value::arena::{alloc_in_region, alloc_region_slice_in_region};
use crate::value::fiberheap::FiberHeap;
use crate::value::heap::HeapObject;
use crate::value::region_slice::RegionSlice;
use crate::value::types::Arity;
use crate::value::CaptureMask;
use crate::value::Value;

use super::header::ClosureTemplate;
use super::payload::{CodePayload, LocEntry, VarargTag};

/// The compile-time blueprint of one lambda: everything the emitter knows about
/// a code object, before any of it reaches a region.
///
/// Held by a `Bytecode`'s `child_protos`, a JIT code object, and every
/// materialized header (so a blueprint outlives every header made from it, and
/// the heap's payload cache cannot sweep a payload a live header still reads).
#[derive(Debug)]
pub struct TemplateProto {
    pub bytecode: Vec<u8>,
    pub arity: Arity,
    pub constants: Vec<Value>,
    /// Total local slots the entry prologue reserves.
    pub num_locals: usize,
    /// Captured variables, for the env layout.
    pub num_captures: usize,
    /// Parameter slots: required + optional + rest, if present.
    pub num_params: usize,
    /// Signal of the closure body.
    pub signal: Signal,
    /// Bit i set means parameter i is mutated and needs a cell.
    pub capture_params_mask: u64,
    /// Which locally-defined variables need a cell. Unbounded in width, so an
    /// uncaptured local at any index gets a bare-NIL env slot, never a leaked
    /// dead cell.
    pub capture_locals_mask: CaptureMask,
    /// Bytecode offset → source location, as the emitter records it. Sorted
    /// into a flat table at materialization.
    pub location_map: LocationMap,
    /// LIR for deferred JIT compilation.
    pub lir_function: Option<Rc<crate::lir::LirFunction>>,
    /// Docstring from the source lambda.
    pub doc: Option<String>,
    /// The defining syntax node, for `eval`'s closure reconstruction.
    pub syntax: Option<Rc<crate::syntax::Syntax>>,
    /// How varargs collect. Only meaningful when `arity` is `AtLeast`.
    pub vararg_kind: VarargKind,
    /// Declared name, for stack traces and diagnostics.
    pub name: Option<String>,
    /// WASM function table index. When set, `rt_call` dispatches to this WASM
    /// function instead of the bytecode.
    pub wasm_func_idx: Option<u32>,
    /// Cached SPIR-V bytes, written once by `(git f)`. SPIR-V is a property of
    /// the code, so the cache belongs to the blueprint every header shares.
    pub spirv: OnceCell<Vec<u8>>,
    /// The compile-time region slots this function mints, each ≥ 2 (slot 1 is
    /// reserved and never minted). Empty when the function has no allocations.
    pub region_table: Vec<StaticRegion>,
    /// The static region slots this function's allocations share after a
    /// builder-idiom merge (docs/impl/region/merging.md § Merging).
    pub merged_slots: FxHashSet<u32>,
    /// The local slots this function's value-routed releases read
    /// (docs/impl/region/mechanism.md § "An abandoned frame runs the releases
    /// it still owes").
    pub frame_release_slots: Vec<u16>,
    /// The `DecrefRegion` half of the same table: the static region slots this
    /// function's slot-routed releases name.
    pub frame_release_regions: Vec<u32>,
    /// The blueprints this code object's `MakeClosure` instructions index.
    pub child_protos: Vec<Rc<TemplateProto>>,
}

impl TemplateProto {
    /// A blueprint with the three required fields; everything else empty.
    pub fn new(bytecode: Vec<u8>, arity: Arity, constants: Vec<Value>) -> Self {
        TemplateProto {
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
            lir_function: None,
            doc: None,
            syntax: None,
            vararg_kind: VarargKind::List,
            name: None,
            wasm_func_idx: None,
            spirv: OnceCell::new(),
            region_table: Vec::new(),
            merged_slots: FxHashSet::default(),
            frame_release_slots: Vec::new(),
            frame_release_regions: Vec::new(),
            child_protos: Vec::new(),
        }
    }

    /// The vararg tag this blueprint's payload carries.
    pub fn vararg_tag(&self) -> VarargTag {
        match self.vararg_kind {
            VarargKind::List => VarargTag::List,
            VarargKind::Struct => VarargTag::Struct,
            VarargKind::StrictStruct(_) => VarargTag::StrictStruct,
        }
    }
}

/// Build this blueprint's payload into `region`. Every slice lands in the one
/// region, so a header's reference to any of them resolves to the same region
/// id and one counted edge covers the whole payload — the invariant
/// [`ClosureTemplate`]'s cross-reference scan relies on.
pub(super) fn materialize_payload(
    heap: &mut FiberHeap,
    proto: &TemplateProto,
    region: RuntimeRegion,
) -> RegionSlice<CodePayload> {
    let bytecode = alloc_region_slice_in_region::<u8>(heap, &proto.bytecode, region);
    let constants = alloc_region_slice_in_region::<Value>(heap, &proto.constants, region);

    // Intern each distinct file name once, then record every location as an
    // index into that table. The emitter's map is a hash map, so the entries
    // are sorted here: a binary search over an unsorted table answers wrongly.
    let mut sorted: Vec<(&usize, &crate::reader::SourceLoc)> = proto.location_map.iter().collect();
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
        .map(|f| alloc_region_slice_in_region::<u8>(heap, f.as_bytes(), region))
        .collect();
    let files = alloc_region_slice_in_region::<RegionSlice<u8>>(heap, &files, region);
    let locations = alloc_region_slice_in_region::<LocEntry>(heap, &entries, region);

    let name = region_str(heap, proto.name.as_deref(), region);
    let doc = region_str(heap, proto.doc.as_deref(), region);

    let region_table =
        alloc_region_slice_in_region::<StaticRegion>(heap, &proto.region_table, region);

    let mut merged: Vec<u32> = proto.merged_slots.iter().copied().collect();
    merged.sort_unstable();
    let merged_slots = alloc_region_slice_in_region::<u32>(heap, &merged, region);

    // Both release tables are stored ascending: a value route's identity is its
    // slot, so an abandoned frame's walk reads them in a fixed order whatever
    // order the lowerer discovered the releases in.
    let mut release_slots = proto.frame_release_slots.clone();
    release_slots.sort_unstable();
    let frame_release_slots = alloc_region_slice_in_region::<u16>(heap, &release_slots, region);
    let mut release_regions = proto.frame_release_regions.clone();
    release_regions.sort_unstable();
    let frame_release_regions = alloc_region_slice_in_region::<u32>(heap, &release_regions, region);
    let capture_locals =
        alloc_region_slice_in_region::<u64>(heap, proto.capture_locals_mask.words(), region);

    let strict_keys = match &proto.vararg_kind {
        VarargKind::StrictStruct(keys) => {
            let keys: Vec<RegionSlice<u8>> = keys
                .iter()
                .map(|k| alloc_region_slice_in_region::<u8>(heap, k.as_bytes(), region))
                .collect();
            alloc_region_slice_in_region::<RegionSlice<u8>>(heap, &keys, region)
        }
        _ => RegionSlice::empty(),
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
        arity: proto.arity,
        signal: proto.signal,
        capture_params_mask: proto.capture_params_mask,
        num_locals: proto.num_locals as u32,
        num_captures: proto.num_captures as u32,
        num_params: proto.num_params as u32,
        wasm_func_idx: proto.wasm_func_idx,
        vararg: proto.vararg_tag(),
        has_name: name.1,
        has_doc: doc.1,
    };
    alloc_region_slice_in_region::<CodePayload>(heap, &[payload], region)
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
            alloc_region_slice_in_region::<u8>(heap, s.as_bytes(), region),
            true,
        ),
        None => (RegionSlice::empty(), false),
    }
}

/// Materialize a header for `proto` into `region` and return it as a `Value`.
///
/// The payload comes from the heap's cache — built on first use for this
/// blueprint and shared by every header afterwards — so this allocates one
/// heap object and copies two words, whatever the size of the function's
/// bytecode.
pub fn materialize(
    heap: &mut FiberHeap,
    proto: &Rc<TemplateProto>,
    region: RuntimeRegion,
) -> Value {
    let payload = heap.template_payload(proto);
    alloc_in_region(
        heap,
        HeapObject::ClosureTemplate(ClosureTemplate::new(payload, Rc::clone(proto))),
        region,
    )
}
