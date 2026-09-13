// audited: 2026-09-13
//! The compacting copy: what the dumper accepts into an image's body, and
//! the spellings it records on the way through.
//!
//! docs/impl/image/sealing.md
//!
//! One walk builds a sealed twin of the graph in a scratch region, sharing
//! preserved through a map keyed on source payload address. A value outside
//! the sealed set fails the copy, naming the variant, before any byte is
//! written. Every symbol and keyword the walk meets — in a value position or
//! as a struct key — leaves its spelling in the name table. The two fields
//! that may name a process-owned resource — a `traits` field and a
//! `Parameter`'s `default` — either copy as program data or become a
//! reconstruction the hydrating instance answers for itself. A closure's
//! code payload copies once per blueprint, and its header crosses without
//! the blueprint (docs/impl/image/sealing.md).

use std::collections::{BTreeSet, HashMap};

use crate::hir::region::RuntimeRegion;
use crate::port::Port;
use crate::symbol::SymbolTable;
use crate::syntax::SyntaxArena;
use crate::value::closure::{Closure, ClosureTemplate, CodePayload, TemplateRef};
use crate::value::fiberheap::FiberHeap;
use crate::value::heap::{deref, HeapObject, Pair};
use crate::value::region_slice::RegionSlice;
use crate::value::repr::{
    TAG_EMPTY_LIST, TAG_FALSE, TAG_FLOAT, TAG_INT, TAG_KEYWORD, TAG_NATIVE_FN, TAG_NIL, TAG_SYMBOL,
    TAG_TRUE,
};
use crate::value::{TableKey, Value};

use super::super::format::{Ctor, Stdio};
use super::super::layout;
use super::{primitive_name, ImageError};

/// What the copying walk carries: the sharing map, and the spellings met so
/// far. Both are per-dump state the recursion threads through every value.
pub(super) struct Walk<'a> {
    /// Source payload address → its copy in the scratch region.
    visited: HashMap<usize, Value>,
    /// Source code-payload backing → its copy, so two headers materialized
    /// from one blueprint keep one payload copy (docs/impl/image/sealing.md).
    payloads: HashMap<usize, RegionSlice<CodePayload>>,
    /// The dumping instance's display memo, read for spellings.
    memo: &'a SymbolTable,
    /// The spellings met, deduplicated and ordered by name — the order the
    /// name table is written in, so one graph writes one table whatever order
    /// the memo learned them in.
    names: BTreeSet<Box<str>>,
    /// The fields the hydrating instance fills in for itself, by the address
    /// of the field inside its copy. Emit turns each into a reconstruction
    /// entry once it knows where that address lands in the image. Keyed by the
    /// field rather than by the object, because one object has two such fields.
    reconstructions: HashMap<usize, Ctor>,
}

impl<'a> Walk<'a> {
    pub(super) fn new(memo: &'a SymbolTable) -> Self {
        Walk {
            visited: HashMap::new(),
            payloads: HashMap::new(),
            memo,
            names: BTreeSet::new(),
            reconstructions: HashMap::new(),
        }
    }

    /// Record the spelling of a symbol or keyword the walk just met. A value
    /// whose spelling this instance never learned contributes none: it still
    /// dumps, and it still prints as `#<symbol:hash>` on the other side
    /// (docs/impl/symbol.md).
    fn note_name(&mut self, v: Value) {
        let memo = self.memo;
        let name = match v.as_symbol() {
            Some(id) => memo.name(id),
            None => crate::value::keyword::resolve_keyword_name(Some(memo), v.payload),
        };
        if let Some(name) = name {
            if !self.names.contains(name) {
                self.names.insert(name.into());
            }
        }
    }

    /// Record that the field at `offset` inside the copy at `at` is one the
    /// hydrating instance builds. `offset` comes from the layout probe, which
    /// has a slot for every field a reconstruction may name.
    fn reconstruct(&mut self, at: usize, offset: Option<usize>, ctor: Ctor) {
        let offset = offset.expect("a reconstructed field's variant carries a probed slot");
        self.reconstructions.insert(at + offset, ctor);
    }

    pub(super) fn into_tables(self) -> (Vec<Box<str>>, HashMap<usize, Ctor>) {
        (self.names.into_iter().collect(), self.reconstructions)
    }
}

/// What the copy of an object carries in a field that may name a
/// process-owned resource (docs/impl/image/sealing.md).
enum Crossing {
    /// Nothing: the source field was nil.
    None,
    /// The hydrating instance's own value, named by a constructor. The copy
    /// carries nil until hydration fills the slot in.
    Reconstruct(Ctor),
    /// Program data, copied into the image like any other value.
    Carried(Value),
}

impl Crossing {
    /// What the copy is built with. A reconstructed slot is written at
    /// hydration, so the dump leaves it nil and the emitter zeroes it.
    fn value(&self) -> Value {
        match self {
            Crossing::Carried(v) => *v,
            _ => Value::NIL,
        }
    }
}

/// Decide what a source object's `traits` field crosses as, copying a user
/// table into `region` on the way.
///
/// The identity test runs over the whole default table rather than the
/// object's own tag: one traitset serves several tags, and `with-traits` can
/// attach any of them to any value.
fn copy_traits(
    heap: &mut FiberHeap,
    region: RuntimeRegion,
    traits: Value,
    walk: &mut Walk,
) -> Result<Crossing, ImageError> {
    let Some(ptr) = traits.as_heap_ptr() else {
        return Ok(Crossing::None);
    };
    let default = heap
        .default_traits_table()
        .iter()
        .position(|t| t.as_heap_ptr() == Some(ptr));
    if let Some(i) = default {
        let tag = super::super::format::tag_from_u64(i as u64)?;
        return Ok(Crossing::Reconstruct(Ctor::DefaultTraits(tag)));
    }
    Ok(Crossing::Carried(copy_value(heap, region, traits, walk)?))
}

/// Decide what a `Parameter`'s `default` field crosses as.
///
/// An `External` is the one thing a default may hold that is not sealed data,
/// and among externals only a standard stream travels: the hydrating instance
/// opens its own. Everything else fails the dump here, naming what it met.
fn copy_default(
    heap: &mut FiberHeap,
    region: RuntimeRegion,
    default: Value,
    walk: &mut Walk,
) -> Result<Crossing, ImageError> {
    let Some(type_name) = default.external_type_name() else {
        return Ok(Crossing::Carried(copy_value(heap, region, default, walk)?));
    };
    let kind = default.as_external::<Port>().map(|p| p.kind());
    match kind.and_then(Stdio::of) {
        Some(stream) => Ok(Crossing::Reconstruct(Ctor::StdioPort(stream))),
        None => Err(ImageError::Unsupported(match kind {
            Some(kind) => format!(
                "a {kind:?} port owns a descriptor this process opened, so no image carries it"
            ),
            None => format!(
                "a parameter's default holds a {type_name} external, which the image cannot rebuild"
            ),
        })),
    }
}

/// Deep-copy one sealed data value into the scratch region, preserving
/// sharing through the walk's visited map (keyed on source payload address).
/// A value outside the sealed set fails the copy, naming the variant.
pub(super) fn copy_value(
    heap: &mut FiberHeap,
    region: RuntimeRegion,
    v: Value,
    walk: &mut Walk,
) -> Result<Value, ImageError> {
    if !v.is_heap() {
        return match v.tag {
            TAG_INT | TAG_FLOAT | TAG_NIL | TAG_TRUE | TAG_FALSE | TAG_EMPTY_LIST => Ok(v),
            // A symbol and a keyword payload are name hashes, so the value
            // means the same thing in every process; only the spelling has to
            // travel beside it.
            TAG_SYMBOL | TAG_KEYWORD => {
                walk.note_name(v);
                Ok(v)
            }
            // A native-fn is its `prim_id`, which means nothing in another
            // process — so the value only crosses if a canonical name carries
            // it. The emitter records the slot; this is where an unnameable
            // def fails, at the value rather than at the byte.
            TAG_NATIVE_FN => {
                primitive_name(v)?;
                Ok(v)
            }
            _ => Err(ImageError::Unsupported(format!(
                "immediate {} is not portable data",
                v.type_name()
            ))),
        };
    }
    let key = v.payload as usize;
    if let Some(&copy) = walk.visited.get(&key) {
        return Ok(copy);
    }
    let obj = unsafe { deref(v) };
    let traits = copy_traits(heap, region, obj.traits(), walk)?;
    let carried = traits.value();
    let default = match obj {
        HeapObject::Parameter { default, .. } => copy_default(heap, region, *default, walk)?,
        _ => Crossing::None,
    };
    let copy = match obj {
        HeapObject::Pair(pair) => {
            let first = copy_value(heap, region, pair.first, walk)?;
            let rest = copy_value(heap, region, pair.rest, walk)?;
            heap.alloc_in_region(
                HeapObject::Pair(Pair::with_traits(first, rest, carried)),
                region,
            )
        }
        HeapObject::LString { s, .. } => {
            let slice = heap.alloc_region_slice_in_region(s.as_slice(), region);
            heap.alloc_in_region(
                HeapObject::LString {
                    s: slice,
                    traits: carried,
                },
                region,
            )
        }
        HeapObject::LBytes { data, .. } => {
            let slice = heap.alloc_region_slice_in_region(data.as_slice(), region);
            heap.alloc_in_region(
                HeapObject::LBytes {
                    data: slice,
                    traits: carried,
                },
                region,
            )
        }
        HeapObject::LArray { elements, .. } => {
            let mut copies = Vec::with_capacity(elements.len());
            for &el in elements.iter() {
                copies.push(copy_value(heap, region, el, walk)?);
            }
            let slice = heap.alloc_region_slice_in_region(&copies, region);
            heap.alloc_in_region(
                HeapObject::LArray {
                    elements: slice,
                    traits: carried,
                },
                region,
            )
        }
        // A sorted container copies in order and is never re-sorted: every
        // key and element an image may carry ranks by its own content, so the
        // order the copy preserves is the order the hydrating instance's
        // comparator agrees with (docs/impl/image/sealing.md).
        HeapObject::LSet { data, .. } => {
            let mut copies = Vec::with_capacity(data.len());
            for &el in data.iter() {
                copies.push(copy_value(heap, region, el, walk)?);
            }
            let slice = heap.alloc_region_slice_in_region(&copies, region);
            heap.alloc_in_region(
                HeapObject::LSet {
                    data: slice,
                    traits: carried,
                },
                region,
            )
        }
        HeapObject::LStruct { data, .. } => {
            let mut copies = Vec::with_capacity(data.len());
            for &(key, value) in data.iter() {
                let key = copy_key(heap, region, key, walk)?;
                copies.push((key, copy_value(heap, region, value, walk)?));
            }
            let slice = heap.alloc_region_slice_in_region(&copies, region);
            heap.alloc_in_region(
                HeapObject::LStruct {
                    data: slice,
                    traits: carried,
                },
                region,
            )
        }
        // A tree is region-resident POD with no Rust-heap ownership, so the
        // copy the syntax module already performs between arenas is the copy
        // an image needs — every node, child slice, string payload, and scope
        // set rebuilt in the destination.
        HeapObject::Syntax { syntax, .. } => {
            let arena = unsafe { SyntaxArena::from_raw(heap as *mut FiberHeap, region) };
            let owned = syntax.copy_into(&arena);
            heap.alloc_in_region(
                HeapObject::Syntax {
                    syntax: owned,
                    traits: carried,
                },
                region,
            )
        }
        // A parameter is sealed POD whose id crosses unchanged: resolution is
        // by id, so the image records a watermark and hydration mints above it
        // rather than renumbering anything here.
        HeapObject::Parameter { id, .. } => heap.alloc_in_region(
            HeapObject::Parameter {
                id: *id,
                default: default.value(),
                traits: carried,
            },
            region,
        ),
        // A closure is its template, its env, and its squelch mask. The env
        // values go through the ordinary walk, so a capture cell in one
        // refuses here as unsealed data until snapping lands
        // (docs/impl/image/sealing.md).
        HeapObject::Closure { closure, .. } => {
            let template = copy_value(heap, region, closure.template.value(), walk)?;
            let mut env = Vec::with_capacity(closure.env.len());
            for &slot in closure.env.iter() {
                env.push(copy_value(heap, region, slot, walk)?);
            }
            let env = heap.alloc_region_slice_in_region(&env, region);
            heap.alloc_in_region(
                HeapObject::Closure {
                    closure: Closure::new(TemplateRef::region(template), env, closure.squelch_mask),
                    traits: carried,
                },
                region,
            )
        }
        // A header crosses as its payload alone: the blueprint is Rust-heap
        // data the hydrating instance never holds, so the copy carries none
        // and the hydrated header answers its questions with absence.
        HeapObject::ClosureTemplate(t) => {
            let payload = copy_payload(heap, region, t, walk)?;
            heap.alloc_in_region(
                HeapObject::ClosureTemplate(ClosureTemplate::new(payload, None)),
                region,
            )
        }
        HeapObject::Float(f) => heap.alloc_in_region(HeapObject::Float(*f), region),
        other => {
            return Err(ImageError::Unsupported(format!(
                "{:?} is not sealed data (docs/impl/image/sealing.md)",
                other.tag()
            )))
        }
    };
    let at = copy.as_heap_ptr().expect("a copied object is heap") as usize;
    let tag = obj.tag();
    if let Crossing::Reconstruct(ctor) = traits {
        walk.reconstruct(at, layout::traits_slot_in(tag), ctor);
    }
    if let Crossing::Reconstruct(ctor) = default {
        walk.reconstruct(at, layout::default_slot_in(tag), ctor);
    }
    walk.visited.insert(key, copy);
    Ok(copy)
}

/// Copy one code payload into the scratch region, deduplicated on the source
/// backing so every header from one blueprint keeps one copy. Constants go
/// through the value walk; every other field is plain data.
///
/// The two refusals live here because only the blueprint can answer them: a
/// template whose `MakeClosure` instructions index child blueprints would
/// hydrate as a closure that cannot build its lambdas, and a WASM-built
/// closure dispatches through a function table this process holds
/// (docs/impl/image/sealing.md).
fn copy_payload(
    heap: &mut FiberHeap,
    region: RuntimeRegion,
    t: &ClosureTemplate,
    walk: &mut Walk,
) -> Result<RegionSlice<CodePayload>, ImageError> {
    if !t.child_protos().is_empty() {
        return Err(ImageError::Unsupported(format!(
            "closure {} builds nested lambdas, which the body cannot carry yet \
             (docs/impl/image/sealing.md)",
            t.display_label()
        )));
    }
    if t.wasm_func_idx().is_some() {
        return Err(ImageError::Unsupported(format!(
            "closure {} dispatches into a WASM module this process holds, so no \
             image carries it",
            t.display_label()
        )));
    }
    let key = t.payload_backing() as usize;
    if let Some(&copy) = walk.payloads.get(&key) {
        return Ok(copy);
    }
    let src = *t.payload();
    let mut constants = Vec::with_capacity(src.constants.len());
    for &c in src.constants.iter() {
        constants.push(copy_value(heap, region, c, walk)?);
    }
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
        ..src
    };
    let copy = heap.alloc_region_slice_in_region(&[payload], region);
    walk.payloads.insert(key, copy);
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

/// Copy one struct key into the scratch region: its own value goes through
/// the value walk, so a key's string or array is shared and relocated exactly
/// as the same value in a struct's *value* position would be.
///
/// A key is also a name site. Keys never pass through [`copy_value`], so a
/// symbol or keyword key's spelling is recorded here or nowhere.
fn copy_key(
    heap: &mut FiberHeap,
    region: RuntimeRegion,
    key: TableKey,
    walk: &mut Walk,
) -> Result<TableKey, ImageError> {
    match key {
        TableKey::Symbol(id) => walk.note_name(Value::symbol(id)),
        TableKey::Keyword(hash) => walk.note_name(Value::keyword_from_hash(hash)),
        _ => {}
    }
    match key.heap_value() {
        Some(&v) => Ok(key.with_heap_value(copy_value(heap, region, v, walk)?)),
        None => Ok(key),
    }
}
