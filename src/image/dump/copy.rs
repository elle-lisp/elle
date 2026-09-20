// audited: 2026-09-20
//! The compacting copy: what the dumper accepts into an image's body, and
//! the spellings it records on the way through.
//!
//! docs/impl/image/sealing.md
//!
//! One walk builds a sealed twin of the graph in a scratch region, sharing
//! preserved through a map keyed on source payload address. A value outside
//! the sealed set fails the copy, naming the variant, before any byte is
//! written. Every symbol and keyword the walk meets — in a value position or
//! as a struct key — leaves its spelling in the name table. A closure's
//! header crosses without its blueprint; code.rs owns what the payload
//! behind it costs, and crossing.rs owns the two fields that may name a
//! process-owned resource (docs/impl/image/sealing.md).

use std::collections::{BTreeSet, HashMap};

use crate::hir::region::RuntimeRegion;
use crate::symbol::SymbolTable;
use crate::syntax::SyntaxArena;
use crate::value::closure::{Closure, ClosureTemplate, CodePayload, TemplateRef};
use crate::value::fiberheap::FiberHeap;
use crate::value::heap::{deref, CellOrigin, HeapObject, Pair};
use crate::value::region_slice::RegionSlice;
use crate::value::repr::{
    TAG_EMPTY_LIST, TAG_FALSE, TAG_FLOAT, TAG_INT, TAG_KEYWORD, TAG_NATIVE_FN, TAG_NIL, TAG_SYMBOL,
    TAG_TRUE,
};
use crate::value::{TableKey, Value};

use super::super::format::Ctor;
use super::super::layout;
use super::code::copy_payload;
use super::crossing::{copy_default, copy_traits, Crossing};
use super::{primitive_name, ImageError};

/// What the copying walk carries: the sharing map, and the spellings met so
/// far. Both are per-dump state the walk threads through every value.
pub(super) struct Walk<'a> {
    /// Source payload address → its copy in the scratch region.
    visited: HashMap<usize, Value>,
    /// Source code-payload backing → its copy, so two headers materialized
    /// from one blueprint keep one payload copy (docs/impl/image/sealing.md).
    /// code.rs is the only reader.
    pub(super) payloads: HashMap<usize, RegionSlice<CodePayload>>,
    /// Source code-payload backing → the body header the walk built for it as
    /// somebody's child. Keyed like `payloads`, because one payload is one
    /// code object, and separate from it because a child table names the
    /// header rather than the payload.
    pub(super) children: HashMap<usize, Value>,
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
            children: HashMap::new(),
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
    // A list's `rest` spine is walked by loop rather than by recursion, so a
    // list costs memory rather than stack (docs/impl/image.md).
    if matches!(obj, HeapObject::Pair(_)) {
        return copy_pair_spine(heap, region, v, walk);
    }
    let traits = copy_traits(heap, region, obj.traits(), walk)?;
    let carried = traits.value();
    let default = match obj {
        HeapObject::Parameter { default, .. } => copy_default(heap, region, *default, walk)?,
        _ => Crossing::None,
    };
    let copy = match obj {
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
        // A closure is its template, its env, and its squelch mask. The copy
        // is reserved before its children are walked: a letrec cycle through
        // two snapped cells re-enters this closure, and the reservation's
        // visited entry is what closes the cycle onto one copy. Every patched
        // value lands in the same scratch region, so the patch writes
        // self-edges the RC ledger never counts.
        HeapObject::Closure { closure, .. } => {
            let nils = vec![Value::NIL; closure.env.len()];
            let env = heap.alloc_region_slice_in_region(&nils, region);
            let copy = heap.alloc_in_region(
                HeapObject::Closure {
                    closure: Closure::new(
                        TemplateRef::region(Value::NIL),
                        env,
                        closure.squelch_mask,
                    ),
                    traits: carried,
                },
                region,
            );
            walk.visited.insert(key, copy);
            let template = copy_value(heap, region, closure.template.value(), walk)?;
            let slots = env.as_ptr() as *mut Value;
            for (i, &slot) in closure.env.iter().enumerate() {
                let v = copy_value(heap, region, slot, walk)?;
                unsafe { slots.add(i).write(v) };
            }
            let obj = copy.as_heap_ptr().expect("a copied object is heap") as *mut HeapObject;
            match unsafe { &mut *obj } {
                HeapObject::Closure { closure, .. } => {
                    closure.template = TemplateRef::region(template);
                }
                _ => unreachable!("the reserved copy is a closure"),
            }
            copy
        }
        // A compiled cell for a never-assigned binding snaps: the copy is the
        // content, keyed under the cell so every capturer shares it
        // (docs/impl/image/sealing.md). The refusals are the assigned
        // binding, by name, and the run-time cell, by variant.
        HeapObject::CaptureCell { cell, origin, .. } => match origin {
            CellOrigin::Compiled { mutated: false, .. } => {
                let content = *cell.borrow();
                let copy = copy_value(heap, region, content, walk)?;
                walk.visited.insert(key, copy);
                return Ok(copy);
            }
            CellOrigin::Compiled {
                name,
                mutated: true,
            } => {
                let spelled = walk
                    .memo
                    .name(*name)
                    .map(str::to_owned)
                    .unwrap_or_else(|| format!("#<symbol:{:#x}>", name.0));
                return Err(ImageError::Unsupported(format!(
                    "top-level {spelled} is assigned, so its capture cell is \
                     mutable state no image carries"
                )));
            }
            CellOrigin::Runtime => {
                return Err(ImageError::Unsupported(
                    "a run-time CaptureCell is mutable state no image carries".into(),
                ));
            }
        },
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

/// Write one field of a reserved pair copy, which is scratch-region memory
/// this walk allocated and nothing else has seen.
fn patch_pair(copy: Value, write: impl FnOnce(&mut Pair)) {
    let obj = copy.as_heap_ptr().expect("a copied object is heap") as *mut HeapObject;
    match unsafe { &mut *obj } {
        HeapObject::Pair(pair) => write(pair),
        _ => unreachable!("the reserved copy is a pair"),
    }
}

/// Copy a list, walking its `rest` spine with a loop.
///
/// A list's length is bounded by memory rather than by the text that built
/// it, so a recursive walk down `rest` aborts the process on a stack overflow
/// where this writes the image (docs/impl/image.md). Each copy is reserved
/// before anything it holds is walked, exactly as a closure's is, so a cycle
/// that re-enters a pair closes onto the one copy.
///
/// The spine ends at the first link that is not an unvisited pair: an empty
/// list, an improper tail, or a link some earlier walk already copied. That
/// value goes through the ordinary walk, so a tail two lists share is copied
/// once.
fn copy_pair_spine(
    heap: &mut FiberHeap,
    region: RuntimeRegion,
    head: Value,
    walk: &mut Walk,
) -> Result<Value, ImageError> {
    let mut spine: Vec<(Value, Value)> = Vec::new();
    let mut cur = head;
    let tail = loop {
        if !cur.is_heap() {
            break cur;
        }
        let key = cur.payload as usize;
        if walk.visited.contains_key(&key) {
            break cur;
        }
        let obj = unsafe { deref(cur) };
        let HeapObject::Pair(pair) = obj else {
            break cur;
        };
        let (first, rest) = (pair.first, pair.rest);
        let traits = copy_traits(heap, region, obj.traits(), walk)?;
        let copy = heap.alloc_in_region(
            HeapObject::Pair(Pair::with_traits(Value::NIL, Value::NIL, traits.value())),
            region,
        );
        walk.visited.insert(key, copy);
        if let Crossing::Reconstruct(ctor) = traits {
            let at = copy.as_heap_ptr().expect("a copied object is heap") as usize;
            walk.reconstruct(at, layout::traits_slot_in(obj.tag()), ctor);
        }
        spine.push((first, copy));
        cur = rest;
    };

    // Every value written below lands in the same scratch region as the copy
    // holding it, so each patch writes a self-edge the RC ledger never counts.
    for &(first, copy) in &spine {
        let first = copy_value(heap, region, first, walk)?;
        patch_pair(copy, |p| p.first = first);
    }
    let mut acc = copy_value(heap, region, tail, walk)?;
    for &(_, copy) in spine.iter().rev() {
        patch_pair(copy, |p| p.rest = acc);
        acc = copy;
    }
    Ok(acc)
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
