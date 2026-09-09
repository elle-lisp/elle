// audited: 2026-09-08
//! The compacting copy: what the dumper accepts into an image's body, and
//! the spellings it records on the way through.
//!
//! docs/impl/image.md
//!
//! One walk builds a sealed twin of the graph in a scratch region, sharing
//! preserved through a map keyed on source payload address. A value outside
//! the sealed set fails the copy, naming the variant, before any byte is
//! written. Every symbol and keyword the walk meets — in a value position or
//! as a struct key — leaves its spelling in the name table.

use std::collections::{BTreeSet, HashMap};

use crate::hir::region::RuntimeRegion;
use crate::symbol::SymbolTable;
use crate::value::fiberheap::FiberHeap;
use crate::value::heap::{deref, HeapObject, Pair};
use crate::value::repr::{
    TAG_EMPTY_LIST, TAG_FALSE, TAG_FLOAT, TAG_INT, TAG_KEYWORD, TAG_NIL, TAG_SYMBOL, TAG_TRUE,
};
use crate::value::{TableKey, Value};

use super::ImageError;

/// What the copying walk carries: the sharing map, and the spellings met so
/// far. Both are per-dump state the recursion threads through every value.
pub(super) struct Walk<'a> {
    /// Source payload address → its copy in the scratch region.
    visited: HashMap<usize, Value>,
    /// The dumping instance's display memo, read for spellings.
    memo: &'a SymbolTable,
    /// The spellings met, deduplicated and ordered by name — the order the
    /// name table is written in, so one graph writes one table whatever order
    /// the memo learned them in.
    names: BTreeSet<Box<str>>,
}

impl<'a> Walk<'a> {
    pub(super) fn new(memo: &'a SymbolTable) -> Self {
        Walk {
            visited: HashMap::new(),
            memo,
            names: BTreeSet::new(),
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

    pub(super) fn into_name_table(self) -> Vec<Box<str>> {
        self.names.into_iter().collect()
    }
}

/// Deep-copy one sealed data value into the scratch region, preserving
/// sharing through the walk's visited map (keyed on source payload address).
/// A value outside the spike's sealed set fails the copy, naming the variant.
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
    if obj.traits() != Value::NIL {
        return Err(ImageError::Unsupported(format!(
            "{:?} carries traits — instance state the image cannot own",
            obj.tag()
        )));
    }
    let copy = match obj {
        HeapObject::Pair(pair) => {
            let first = copy_value(heap, region, pair.first, walk)?;
            let rest = copy_value(heap, region, pair.rest, walk)?;
            heap.alloc_in_region(HeapObject::Pair(Pair::new(first, rest)), region)
        }
        HeapObject::LString { s, .. } => {
            let slice = heap.alloc_region_slice_in_region(s.as_slice(), region);
            heap.alloc_in_region(
                HeapObject::LString {
                    s: slice,
                    traits: Value::NIL,
                },
                region,
            )
        }
        HeapObject::LBytes { data, .. } => {
            let slice = heap.alloc_region_slice_in_region(data.as_slice(), region);
            heap.alloc_in_region(
                HeapObject::LBytes {
                    data: slice,
                    traits: Value::NIL,
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
                    traits: Value::NIL,
                },
                region,
            )
        }
        // A sorted container copies in order and is never re-sorted: every
        // key and element an image may carry ranks by its own content, so the
        // order the copy preserves is the order the hydrating instance's
        // comparator agrees with (docs/impl/image.md § Sealing).
        HeapObject::LSet { data, .. } => {
            let mut copies = Vec::with_capacity(data.len());
            for &el in data.iter() {
                copies.push(copy_value(heap, region, el, walk)?);
            }
            let slice = heap.alloc_region_slice_in_region(&copies, region);
            heap.alloc_in_region(
                HeapObject::LSet {
                    data: slice,
                    traits: Value::NIL,
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
                    traits: Value::NIL,
                },
                region,
            )
        }
        HeapObject::Float(f) => heap.alloc_in_region(HeapObject::Float(*f), region),
        other => {
            return Err(ImageError::Unsupported(format!(
                "{:?} is not sealed data (docs/impl/image.md § Sealing)",
                other.tag()
            )))
        }
    };
    walk.visited.insert(key, copy);
    Ok(copy)
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
