// audited: 2026-09-21
//! The code-object half of the compacting copy: a payload, and the child code
//! objects its `MakeClosure` instructions index.
//!
//! docs/impl/image/sealing.md
//!
//! copy.rs owns the value walk and calls in here whenever it meets a code
//! object. A payload copies once per blueprint, its constants go back through
//! the value walk, and its child table is built here — where the blueprint
//! that would otherwise have answered for it is still in reach.

use crate::hir::region::RuntimeRegion;
use crate::value::closure::{ChildCode, ClosureTemplate, CodePayload};
use crate::value::fiberheap::FiberHeap;
use crate::value::heap::HeapObject;
use crate::value::region_slice::RegionSlice;
use crate::value::Value;

use super::copy::{copy_value, Walk};
use super::ImageError;

/// Copy one code payload into the scratch region, deduplicated on the source
/// backing so every header from one blueprint keeps one copy. Constants and
/// children go through walks of their own; every other field is plain data.
///
/// The refusal lives here because only the blueprint can answer it: a
/// WASM-built closure dispatches through a function table this process holds
/// (docs/impl/image/sealing.md).
pub(super) fn copy_payload(
    heap: &mut FiberHeap,
    region: RuntimeRegion,
    t: &ClosureTemplate,
    walk: &mut Walk,
) -> Result<RegionSlice<CodePayload>, ImageError> {
    if t.wasm_func_idx().is_some() {
        return Err(ImageError::Unsupported(format!(
            "closure {} dispatches into a WASM module this process holds, so no \
             image carries it",
            t.display_label()
        )));
    }
    super::portable_signals(t.signal().bits, &format!("closure {}", t.display_label()))?;
    let key = t.payload_backing() as usize;
    if let Some(&copy) = walk.payloads.get(&key) {
        return Ok(copy);
    }
    let src = *t.payload();
    let mut constants = Vec::with_capacity(src.constants.len());
    for &c in src.constants.iter() {
        constants.push(copy_value(heap, region, c, walk)?);
    }
    // The child table is built here, where the blueprint that answers for it
    // is still in reach; the copy carries it because the blueprint does not
    // (docs/impl/image/sealing.md).
    let mut children = Vec::with_capacity(t.num_children());
    for i in 0..t.num_children() {
        children.push(copy_child(heap, region, t.child(i), walk)?);
    }
    let children = heap.alloc_region_slice_in_region(&children, region);
    let payload = CodePayload {
        bytecode: heap.alloc_region_slice_in_region(src.bytecode.as_slice(), region),
        constants: heap.alloc_region_slice_in_region(&constants, region),
        locations: heap.alloc_region_slice_in_region(src.locations.as_slice(), region),
        files: copy_bytes_slices(heap, region, &src.files),
        name: heap.alloc_region_slice_in_region(src.name.as_slice(), region),
        doc: heap.alloc_region_slice_in_region(src.doc.as_slice(), region),
        region_table: heap.alloc_region_slice_in_region(src.region_table.as_slice(), region),
        merged_slots: heap.alloc_region_slice_in_region(src.merged_slots.as_slice(), region),
        frame_release_slots: heap
            .alloc_region_slice_in_region(src.frame_release_slots.as_slice(), region),
        frame_release_regions: heap
            .alloc_region_slice_in_region(src.frame_release_regions.as_slice(), region),
        capture_locals: heap.alloc_region_slice_in_region(src.capture_locals.as_slice(), region),
        strict_keys: copy_bytes_slices(heap, region, &src.strict_keys),
        children,
        ..src
    };
    let copy = heap.alloc_region_slice_in_region(&[payload], region);
    walk.payloads.insert(key, copy);
    Ok(copy)
}

/// Copy one child code object into the body as a header of its own.
///
/// A hydrated `MakeClosure` indexes objects, not blueprints, so a child that
/// is still a blueprint is materialized through the heap's ordinary payload
/// cache first — the same payload the instruction would have built from, one
/// dump earlier (docs/impl/image/sealing.md). Two parents naming one child
/// therefore keep one header as well as one payload.
fn copy_child(
    heap: &mut FiberHeap,
    region: RuntimeRegion,
    child: ChildCode<'_>,
    walk: &mut Walk,
) -> Result<Value, ImageError> {
    let header = match child {
        ChildCode::Blueprint(proto) => ClosureTemplate::for_proto(heap, proto),
        ChildCode::Header(header) => header,
    };
    let key = header.payload_backing() as usize;
    if let Some(&copy) = walk.children.get(&key) {
        return Ok(copy);
    }
    let payload = copy_payload(heap, region, &header, walk)?;
    let copy = heap.alloc_in_region(
        HeapObject::ClosureTemplate(ClosureTemplate::new(payload, None)),
        region,
    );
    walk.children.insert(key, copy);
    Ok(copy)
}

/// Copy a slice of byte slices — a payload's interned file names, or its
/// `&named` key set — every element's bytes landing in the scratch region.
fn copy_bytes_slices(
    heap: &mut FiberHeap,
    region: RuntimeRegion,
    src: &RegionSlice<RegionSlice<u8>>,
) -> RegionSlice<RegionSlice<u8>> {
    let mut copies = Vec::with_capacity(src.len());
    for inner in src.iter() {
        copies.push(heap.alloc_region_slice_in_region(inner.as_slice(), region));
    }
    heap.alloc_region_slice_in_region(&copies, region)
}
