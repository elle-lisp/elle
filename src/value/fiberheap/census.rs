//! The post-boot heap census (docs/impl/image.md § "Open risks and dispatch
//! experiments", item 2): enumerate every live object in this instance's
//! region store and report the graph a boot image must dump. Reached from
//! `Runtime::build_with` under `--trace=census` and from the sealing
//! regression net in `tests/integration/census.rs`.
//!
//! Byte accounting is a dump-size estimate, not an allocator audit. Each
//! object contributes its `HeapObject` shell plus its payload wherever the
//! payload lives today — region pages (`RegionSlice` backings) or the Rust
//! heap (struct `Vec`s, template bytecode, syntax trees), since the
//! foundations move the latter into the body. Payloads shared through `Rc`
//! or an aliased `RegionSlice` are counted once (first holder). Not counted:
//! per-container `Rc`/`RefCell` bookkeeping, template location maps and
//! masks, and fiber-internal state (fibers are refused from the body).
//!
//! Pointer-slot counting follows the image format's relocation definition
//! (docs/impl/image.md § "Relocation slots"): a heap-tagged `Value` slot or
//! a non-empty `RegionSlice`'s `ptr`; native-fn slots (the primitive
//! stream, remapped by name) are counted separately. `Rc` pointers are not
//! counted — the foundations delete them from persistable objects.

use std::mem::size_of;

use rustc_hash::FxHashSet;

use crate::syntax::{Syntax, SyntaxKind};
use crate::value::heap::{HeapObject, HeapTag};
use crate::value::region_slice::RegionSlice;
use crate::value::types::TableKey;
use crate::value::Value;

use super::regionstore::RegionStore;
use super::FiberHeap;

/// How the image dumper treats a `HeapTag` (docs/impl/image.md § Sealing).
/// The design doc owns the argument; this is its executable form, pinned by
/// `tests/integration/census.rs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sealing {
    /// Body data: byte-self-contained after the foundations.
    Sealed,
    /// `CaptureCell`: rewritten to its final value by the dumper, never
    /// persisted as an object.
    Snapped,
    /// Refused from the body: mutable stores, process and foreign handles.
    Unsealed,
}

/// Classify a tag against the sealed set. Exhaustive on purpose: a new
/// `HeapTag` variant must choose its dump treatment here to compile.
pub fn sealing(tag: HeapTag) -> Sealing {
    use HeapTag::*;
    match tag {
        LString | Pair | LStruct | Closure | LArray | LBytes | LSet | Syntax | Float
        | Parameter | ClosureTemplate => Sealing::Sealed,
        CaptureCell => Sealing::Snapped,
        LArrayMut | LStructMut | LStringMut | LBytesMut | LSetMut | LBox | LibHandle
        | ThreadHandle | Fiber | FFISignature | FFIType | ManagedPointer | External => {
            Sealing::Unsealed
        }
    }
}

/// Per-tag census row.
#[derive(Debug, Clone)]
pub struct TagCensus {
    pub tag: HeapTag,
    pub count: usize,
    /// Shell + payload bytes (module docs describe the estimate).
    pub bytes: usize,
    /// Heap-tagged `Value` slots (relocation candidates).
    pub ptr_slots: usize,
    /// Non-empty `RegionSlice` fields (each `ptr` is a relocation slot).
    pub region_slices: usize,
}

/// One kind of dump-refused object found live, aggregated.
#[derive(Debug, Clone)]
pub struct UnsealedLeaf {
    pub tag: HeapTag,
    /// The `External` type name; empty for other tags.
    pub detail: String,
    pub count: usize,
}

/// The whole-store census.
#[derive(Debug, Clone)]
pub struct HeapCensus {
    /// Active regions in the store.
    pub regions: usize,
    /// Live objects across all regions.
    pub objects: usize,
    /// Committed region-page bytes.
    pub region_bytes: usize,
    /// Shell + payload bytes summed over all rows.
    pub payload_bytes: usize,
    /// Rows for every tag present, largest `bytes` first.
    pub tags: Vec<TagCensus>,
    /// Snapping candidates (`CaptureCell` count).
    pub capture_cells: usize,
    /// Total heap-tagged `Value` slots.
    pub ptr_slots: usize,
    /// Total native-fn `Value` slots (primitive-stream relocations).
    pub prim_slots: usize,
    /// Total non-empty `RegionSlice` fields.
    pub region_slices: usize,
    /// Dump-refused objects, aggregated by (tag, detail).
    pub unsealed: Vec<UnsealedLeaf>,
}

impl FiberHeap {
    /// Census every live object in this instance's region store.
    pub fn census(&self) -> HeapCensus {
        HeapCensus::of_store(&self.region_store)
    }
}

#[derive(Default)]
struct ObjStat {
    bytes: usize,
    ptr_slots: usize,
    prim_slots: usize,
    region_slices: usize,
}

impl HeapCensus {
    pub(crate) fn of_store(store: &RegionStore) -> HeapCensus {
        let mut rows: Vec<Option<TagCensus>> = Vec::new();
        let mut unsealed: Vec<UnsealedLeaf> = Vec::new();
        let mut seen: FxHashSet<usize> = FxHashSet::default();
        let mut objects = 0usize;
        let mut payload_bytes = 0usize;
        let mut ptr_slots = 0usize;
        let mut prim_slots = 0usize;
        let mut region_slices = 0usize;

        for obj in store.live_objects() {
            let tag = obj.tag();
            let stat = measure(obj, &mut seen);
            objects += 1;
            payload_bytes += stat.bytes;
            ptr_slots += stat.ptr_slots;
            prim_slots += stat.prim_slots;
            region_slices += stat.region_slices;

            let idx = tag as usize;
            if rows.len() <= idx {
                rows.resize_with(idx + 1, || None);
            }
            let row = rows[idx].get_or_insert(TagCensus {
                tag,
                count: 0,
                bytes: 0,
                ptr_slots: 0,
                region_slices: 0,
            });
            row.count += 1;
            row.bytes += stat.bytes;
            row.ptr_slots += stat.ptr_slots;
            row.region_slices += stat.region_slices;

            if sealing(tag) == Sealing::Unsealed {
                let detail = match obj {
                    HeapObject::External { obj, .. } => obj.type_name.to_string(),
                    _ => String::new(),
                };
                match unsealed
                    .iter_mut()
                    .find(|l| l.tag == tag && l.detail == detail)
                {
                    Some(l) => l.count += 1,
                    None => unsealed.push(UnsealedLeaf {
                        tag,
                        detail,
                        count: 1,
                    }),
                }
            }
        }

        let capture_cells = rows
            .get(HeapTag::CaptureCell as usize)
            .and_then(|r| r.as_ref())
            .map_or(0, |r| r.count);
        let mut tags: Vec<TagCensus> = rows.into_iter().flatten().collect();
        tags.sort_by_key(|row| std::cmp::Reverse(row.bytes));

        HeapCensus {
            regions: store.active_region_count(),
            objects,
            region_bytes: store.region_bytes(),
            payload_bytes,
            tags,
            capture_cells,
            ptr_slots,
            prim_slots,
            region_slices,
            unsealed,
        }
    }

    /// The census as report lines (no prefix; the caller adds `[trace:census]`).
    pub fn lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.push(format!("regions {}", self.regions));
        out.push(format!(
            "objects {} region-bytes {} payload-bytes {}",
            self.objects, self.region_bytes, self.payload_bytes
        ));
        for row in &self.tags {
            out.push(format!(
                "tag {:?} count {} bytes {} ptr-slots {} slices {}",
                row.tag, row.count, row.bytes, row.ptr_slots, row.region_slices
            ));
        }
        out.push(format!("capture-cells {}", self.capture_cells));
        let kib = (self.region_bytes as f64 / 1024.0).max(1.0);
        out.push(format!(
            "ptr-slots {} prim-slots {} region-slices {} per-kib {:.2} {:.2} {:.2}",
            self.ptr_slots,
            self.prim_slots,
            self.region_slices,
            self.ptr_slots as f64 / kib,
            self.prim_slots as f64 / kib,
            self.region_slices as f64 / kib
        ));
        for leaf in &self.unsealed {
            out.push(format!(
                "unsealed {:?}({}) {}",
                leaf.tag, leaf.detail, leaf.count
            ));
        }
        out.push(format!(
            "unsealed-total {}",
            self.unsealed.iter().map(|l| l.count).sum::<usize>()
        ));
        out
    }
}

fn heap_slot(v: &Value, stat: &mut ObjStat) {
    if v.is_heap() {
        stat.ptr_slots += 1;
    } else if v.is_native_fn() {
        stat.prim_slots += 1;
    }
}

fn slice_stat<T: 'static>(sl: &RegionSlice<T>, seen: &mut FxHashSet<usize>, stat: &mut ObjStat) {
    if sl.is_empty() {
        return;
    }
    stat.region_slices += 1;
    if seen.insert(sl.as_ptr() as usize) {
        stat.bytes += sl.len() * size_of::<T>();
    }
}

fn syntax_bytes(s: &Syntax, seen: &mut FxHashSet<usize>) -> usize {
    let mut b = size_of::<Syntax>() + std::mem::size_of_val(s.scopes());
    match &s.kind {
        SyntaxKind::Symbol(x)
        | SyntaxKind::Keyword(x)
        | SyntaxKind::String(x)
        | SyntaxKind::StringMut(x) => b += x.len(),
        // A syntax literal's captured node is shared by pointer, so it is
        // counted once — the `seen` set is what makes that true.
        SyntaxKind::SyntaxLiteral(inner) => {
            if seen.insert(inner.as_ptr() as usize) {
                b += syntax_bytes(inner, seen);
            }
        }
        other => {
            b += other
                .children()
                .iter()
                .map(|c| syntax_bytes(c, seen))
                .sum::<usize>()
        }
    }
    b
}

/// Payload bytes and relocation slots of one object (module docs describe
/// what is and is not counted).
fn measure(obj: &HeapObject, seen: &mut FxHashSet<usize>) -> ObjStat {
    let mut stat = ObjStat {
        bytes: size_of::<HeapObject>(),
        ..ObjStat::default()
    };
    heap_slot(&obj.traits(), &mut stat);

    match obj {
        HeapObject::LString { s, .. } => slice_stat(s, seen, &mut stat),
        HeapObject::LBytes { data, .. } => slice_stat(data, seen, &mut stat),
        HeapObject::Pair(pair) => {
            heap_slot(&pair.first, &mut stat);
            heap_slot(&pair.rest, &mut stat);
        }
        HeapObject::LArray { elements, .. } => {
            slice_stat(elements, seen, &mut stat);
            for v in elements.iter() {
                heap_slot(v, &mut stat);
            }
        }
        HeapObject::LSet { data, .. } => {
            slice_stat(data, seen, &mut stat);
            for v in data.iter() {
                heap_slot(v, &mut stat);
            }
        }
        HeapObject::LStruct { data, .. } => {
            slice_stat(data, seen, &mut stat);
            for (k, v) in data.iter() {
                k.for_each_heap_value(&mut |kv| heap_slot(kv, &mut stat));
                heap_slot(v, &mut stat);
            }
        }
        HeapObject::Closure { closure, .. } => {
            slice_stat(&closure.env, seen, &mut stat);
            for v in closure.env.iter() {
                heap_slot(v, &mut stat);
            }
            heap_slot(&closure.template.value(), &mut stat);
        }
        HeapObject::ClosureTemplate(t) => {
            // A header is a payload slice and a blueprint pointer, so its own
            // relocation load is the one payload backing. The payload's bytes
            // are counted once per blueprint: every header the same blueprint
            // materializes names the same backing (docs/impl/region/template.md).
            stat.region_slices += 1;
            if seen.insert(t.payload_backing() as usize) {
                let p = t.payload();
                stat.bytes += size_of::<crate::value::closure::CodePayload>()
                    + p.bytecode().len()
                    + std::mem::size_of_val(p.constants())
                    + p.locations().len() * size_of::<crate::value::closure::LocEntry>()
                    + p.name().map_or(0, str::len)
                    + p.doc().map_or(0, str::len);
            }
            for v in t.constants().iter() {
                heap_slot(v, &mut stat);
            }
        }
        HeapObject::Syntax { syntax, .. } => stat.bytes += syntax_bytes(syntax, seen),
        HeapObject::Parameter { default, .. } => heap_slot(default, &mut stat),
        HeapObject::LBox { cell, .. } | HeapObject::CaptureCell { cell, .. } => {
            stat.bytes += size_of::<Value>();
            if let Ok(v) = cell.try_borrow() {
                heap_slot(&v, &mut stat);
            }
        }
        HeapObject::LArrayMut { data, .. } => {
            if let Ok(vs) = data.try_borrow() {
                stat.bytes += vs.len() * size_of::<Value>();
                for v in vs.iter() {
                    heap_slot(v, &mut stat);
                }
            }
        }
        HeapObject::LSetMut { data, .. } => {
            if let Ok(vs) = data.try_borrow() {
                stat.bytes += vs.len() * size_of::<Value>();
                for v in vs.iter() {
                    heap_slot(v, &mut stat);
                }
            }
        }
        HeapObject::LStructMut { data, .. } => {
            if let Ok(m) = data.try_borrow() {
                stat.bytes += m.len() * size_of::<(TableKey, Value)>();
                for (k, v) in m.iter() {
                    k.for_each_heap_value(&mut |kv| heap_slot(kv, &mut stat));
                    heap_slot(v, &mut stat);
                }
            }
        }
        HeapObject::LStringMut { data, .. } | HeapObject::LBytesMut { data, .. } => {
            if let Ok(bs) = data.try_borrow() {
                stat.bytes += bs.len();
            }
        }
        // Shell-only: refused from the body (fiber internals are process
        // state the dump never carries) or payload-free.
        HeapObject::Float(_)
        | HeapObject::LibHandle(_)
        | HeapObject::ThreadHandle { .. }
        | HeapObject::Fiber { .. }
        | HeapObject::FFISignature(_, _)
        | HeapObject::FFIType(_)
        | HeapObject::ManagedPointer { .. }
        | HeapObject::External { .. } => {}
    }
    stat
}
