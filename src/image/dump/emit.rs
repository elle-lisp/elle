// audited: 2026-09-10
//! What the file gets from the copied graph: page bytes, the four relocation
//! streams, the object index, and the scope watermark.
//!
//! docs/impl/image.md
//! docs/impl/image/format.md
//!
//! One pass over the scratch region's live objects. Every record it writes —
//! an object slot, a struct entry, a syntax node — is assembled from probed
//! extents into a zeroed buffer rather than copied, so no construction
//! temporary's padding reaches the artifact.

use std::collections::HashMap;
use std::mem::size_of;

use crate::syntax::{ScopeId, Syntax, SyntaxKind};
use crate::value::fiberheap::regionpool::{PageLayout, RegionPool};
use crate::value::heap::HeapObject;
use crate::value::region_slice::RegionSlice;
use crate::value::{TableKey, Value};

use super::super::format::Ctor;
use super::super::layout;
use super::super::ImageError;
use super::primitive_name;

/// Where the scratch region's addresses land in the image: its pages in
/// placement order, each with the image offset it starts at.
pub(super) struct Placement {
    /// `(base address, byte length, image offset)` per page.
    pages: Vec<(usize, usize, u64)>,
    len: u64,
}

impl Placement {
    /// Pages largest first: packing in descending size order keeps every
    /// page's offset a multiple of its own size (§ File format).
    pub(super) fn new(layouts: &mut [PageLayout]) -> Placement {
        layouts.sort_by_key(|l| std::cmp::Reverse(l.len));
        let mut pages = Vec::with_capacity(layouts.len());
        let mut at = 0u64;
        for l in layouts.iter() {
            pages.push((l.base, l.len, at));
            at += l.len as u64;
        }
        Placement { pages, len: at }
    }

    /// Total bytes of the pages section.
    pub(super) fn len(&self) -> u64 {
        self.len
    }

    /// The image offset of a live address, or a refusal naming what went
    /// wrong: every address the walk meets was copied into this region.
    pub(super) fn offset(&self, addr: usize) -> Result<u64, ImageError> {
        self.pages
            .iter()
            .find(|&&(base, len, _)| addr >= base && addr < base + len)
            .map(|&(base, _, at)| at + (addr - base) as u64)
            .ok_or_else(|| {
                ImageError::Corrupt("dump: a copied value points outside the scratch region".into())
            })
    }
}

/// Everything the walk produces besides the page table.
pub(super) struct Emitted {
    /// The pages section, assembled from a zeroed buffer.
    pub pages: Vec<u8>,
    /// `(slot, target)`, both image offsets.
    pub relocs: Vec<(u64, u64)>,
    /// `(offset, tag)` per heap object.
    pub index: Vec<(u64, u64)>,
    /// `(slot, file name)` per span that names a file. The name becomes an
    /// index into the file table once the whole walk is done and the table's
    /// order is known.
    pub files: Vec<(u64, Box<str>)>,
    /// `(slot, primitive name)` per native-fn payload word, indexed into the
    /// primitive table the same way.
    pub prims: Vec<(u64, &'static str)>,
    /// `(slot, encoded constructor)` per value the hydrating instance builds.
    pub recons: Vec<(u64, u64)>,
    /// One past the highest hygiene scope counter any node carries.
    pub scope_watermark: u32,
}

/// Walk every live object of the copied graph and answer what the file needs.
///
/// `pool` is `None` for a graph that allocated nothing — an image rooted at an
/// immediate — and the answer is then an empty pages section.
pub(super) fn emit(
    pool: Option<&RegionPool>,
    at: &Placement,
    reconstructions: &HashMap<usize, Ctor>,
) -> Result<Emitted, ImageError> {
    let mut out = Emitted {
        pages: vec![0u8; at.len() as usize],
        relocs: Vec::new(),
        index: Vec::new(),
        files: Vec::new(),
        prims: Vec::new(),
        recons: Vec::new(),
        scope_watermark: 0,
    };
    let mut backings: Vec<Backing> = Vec::new();

    for obj in pool.into_iter().flat_map(|p| p.live_objects()) {
        let addr = obj as *const HeapObject as usize;
        let obj_off = at.offset(addr)?;
        out.index.push((obj_off, obj.tag() as u64));
        let dst = obj_off as usize;
        layout::write_canonical(obj, &mut out.pages[dst..dst + size_of::<HeapObject>()]);
        out.traits_slot(obj, addr, reconstructions, at)?;
        match obj {
            HeapObject::Pair(pair) => {
                out.value_slot(&pair.first, at)?;
                out.value_slot(&pair.rest, at)?;
            }
            HeapObject::LString { s, .. } => {
                if let Some((rel, src)) = out.slice_backing(s, at)? {
                    backings.push(Backing::raw::<u8>(rel, src, s.len()));
                }
            }
            HeapObject::LBytes { data, .. } => {
                if let Some((rel, src)) = out.slice_backing(data, at)? {
                    backings.push(Backing::raw::<u8>(rel, src, data.len()));
                }
            }
            HeapObject::LArray { elements, .. } => {
                out.values_backing(elements, at, &mut backings)?;
                for v in elements.iter() {
                    out.value_slot(v, at)?;
                }
            }
            HeapObject::LSet { data, .. } => {
                out.values_backing(data, at, &mut backings)?;
                for v in data.iter() {
                    out.value_slot(v, at)?;
                }
            }
            HeapObject::LStruct { data, .. } => {
                if let Some((rel, src)) = out.slice_backing(data, at)? {
                    backings.push(Backing::entries(rel, src, data.len()));
                }
                // A key holds its own `Value`, so a struct has two pointer
                // slots per entry rather than one.
                for (key, value) in data.iter() {
                    if let Some(v) = key.heap_value() {
                        out.value_slot(v, at)?;
                    }
                    out.value_slot(value, at)?;
                }
            }
            // A syntax object's root node rides in its shell, so the shell's
            // canonical bytes cover `traits` and the walk below writes the
            // node itself, along with every node it reaches.
            HeapObject::Syntax { syntax, .. } => out.node(syntax, at, &mut backings)?,
            HeapObject::Float(_) => {}
            other => {
                // copy_value only allocates the variants above.
                unreachable!("unexpected {:?} in dump scratch", other.tag());
            }
        }
    }
    out.relocs.sort_unstable();
    out.index.sort_unstable();
    out.prims.sort_unstable();
    out.recons.sort_unstable();

    // Object slots and syntax nodes are already canonical in `pages`; add the
    // backings that are not nodes.
    for backing in &backings {
        backing.write(&mut out.pages);
    }
    // Canonicalize every rewritten slot to zero. A relocation slot holds a
    // scratch-region address and a primitive slot a `prim_id` — both are
    // dump-time facts the file must not record, and hydration writes both
    // wholesale. A reconstruction slot takes a whole `Value`, so both its
    // words go.
    let words: Vec<u64> = out
        .relocs
        .iter()
        .map(|&(slot, _)| slot)
        .chain(out.prims.iter().map(|&(slot, _)| slot))
        .collect();
    for slot in words {
        out.pages[slot as usize..slot as usize + 8].fill(0);
    }
    for &(slot, _) in &out.recons {
        out.pages[slot as usize..slot as usize + size_of::<Value>()].fill(0);
    }
    Ok(out)
}

impl Emitted {
    /// Record a relocation from the slot at `slot_addr` to `target`.
    fn slot(&mut self, slot_addr: usize, target: usize, at: &Placement) -> Result<(), ImageError> {
        let entry = (at.offset(slot_addr)?, at.offset(target)?);
        self.relocs.push(entry);
        Ok(())
    }

    /// Record what a `Value` slot needs at hydration: a relocation when it
    /// names a heap object, a primitive entry when it names a native-fn, and
    /// nothing at all for a portable immediate. The slot is named by its own
    /// address, so the walk takes it from the live field rather than computing
    /// an offset from a probe.
    fn value_slot(&mut self, v: &Value, at: &Placement) -> Result<(), ImageError> {
        let slot = &v.payload as *const u64 as usize;
        if let Some(p) = v.as_heap_ptr() {
            return self.slot(slot, p as usize, at);
        }
        if v.is_native_fn() {
            self.prims.push((at.offset(slot)?, primitive_name(*v)?));
        }
        Ok(())
    }

    /// Record what `obj`'s `traits` field needs: a constructor the hydrating
    /// instance runs, a relocation into the body, or nothing
    /// (docs/impl/image.md § Sealing). The field's own address comes from the
    /// probe, because the variants keep it in different places and the copy
    /// this walks is read through a shared `HeapObject` reference.
    fn traits_slot(
        &mut self,
        obj: &HeapObject,
        addr: usize,
        reconstructions: &HashMap<usize, Ctor>,
        at: &Placement,
    ) -> Result<(), ImageError> {
        let traits = obj.traits();
        let ctor = reconstructions.get(&addr);
        if traits.as_heap_ptr().is_none() && ctor.is_none() {
            return Ok(());
        }
        let field = addr
            + layout::traits_slot_in(obj.tag())
                .expect("a traited object's variant carries a probed traits field");
        match ctor {
            Some(&ctor) => {
                self.recons.push((at.offset(field)?, ctor.encode()));
                Ok(())
            }
            None => {
                let target = traits.as_heap_ptr().expect("checked above") as usize;
                self.slot(field + layout::payload_in_value(), target, at)
            }
        }
    }

    /// Record a slice field's relocation slot (its `ptr`, at offset 0 of the
    /// `repr(C)` `RegionSlice`) and answer where its backing goes in the
    /// image. An empty slice has a dangling constant pointer — no slot, no
    /// backing.
    fn slice_backing<T: 'static>(
        &mut self,
        s: &RegionSlice<T>,
        at: &Placement,
    ) -> Result<Option<(u64, usize)>, ImageError> {
        if s.is_empty() {
            return Ok(None);
        }
        let backing = s.as_ptr() as usize;
        self.slot(s as *const RegionSlice<T> as usize, backing, at)?;
        Ok(Some((at.offset(backing)?, backing)))
    }

    /// [`slice_backing`](Self::slice_backing) for the two variants whose
    /// payload is a `Value` slice.
    fn values_backing(
        &mut self,
        s: &RegionSlice<Value>,
        at: &Placement,
        backings: &mut Vec<Backing>,
    ) -> Result<(), ImageError> {
        if let Some((rel, src)) = self.slice_backing(s, at)? {
            backings.push(Backing::raw::<Value>(rel, src, s.len()));
        }
        Ok(())
    }

    /// Write one syntax node into its place in the image and walk what it
    /// names: its scope set, its kind's payload, and every child node.
    ///
    /// A node's own bytes are written here rather than through a backing,
    /// because the walk reaches every node anyway and a child slice is
    /// exactly its nodes end to end — so writing each one canonically covers
    /// the whole slice and leaves the padding inside a node zero.
    fn node(
        &mut self,
        node: &Syntax,
        at: &Placement,
        backings: &mut Vec<Backing>,
    ) -> Result<(), ImageError> {
        let node_off = at.offset(node as *const Syntax as usize)? as usize;
        layout::write_canonical_node(
            node,
            &mut self.pages[node_off..node_off + size_of::<Syntax>()],
        );

        // A file id is an index into a process-wide interner, so the file
        // travels by name and hydration writes the live id into this slot
        // (docs/impl/image/format.md).
        if let Some(name) = node.span.file() {
            self.files
                .push(((node_off + layout::file_slot_in_node()) as u64, name.into()));
        }
        for scope in node.scopes() {
            self.scope_watermark = self.scope_watermark.max(scope.counter() + 1);
        }
        if let Some((rel, src)) = self.slice_backing(&node.scopes, at)? {
            backings.push(Backing::raw::<ScopeId>(rel, src, node.scopes().len()));
        }
        self.kind(&node.kind, at, backings)
    }

    /// The payload of one kind: a region string's backing, or the child nodes
    /// the walk descends into.
    fn kind(
        &mut self,
        kind: &SyntaxKind,
        at: &Placement,
        backings: &mut Vec<Backing>,
    ) -> Result<(), ImageError> {
        use SyntaxKind::*;
        match kind {
            Symbol(s) | Keyword(s) | String(s) | StringMut(s) => {
                let bytes = s.as_bytes();
                if bytes.is_empty() {
                    return Ok(());
                }
                // A `RegionStr` is its byte slice and a `SynRef` is its
                // pointer, so each one's own address is the slot: the layout
                // probe measures both that way and reads them back, so a
                // newtype that grew a field would fail there rather than
                // here.
                let src = bytes.as_ptr() as usize;
                self.slot(s as *const _ as usize, src, at)?;
                backings.push(Backing::raw::<u8>(at.offset(src)?, src, bytes.len()));
                Ok(())
            }
            List(n) | Array(n) | ArrayMut(n) | Struct(n) | StructMut(n) | Set(n) | SetMut(n)
            | Bytes(n) | BytesMut(n) => {
                // No backing: the walk writes each child node canonically at
                // its own place, and a child slice is exactly its nodes end
                // to end.
                self.slice_backing(n, at)?;
                for child in n.as_slice() {
                    self.node(child, at, backings)?;
                }
                Ok(())
            }
            Quote(r) | Quasiquote(r) | Unquote(r) | UnquoteSplicing(r) | Splice(r)
            | SyntaxLiteral(r) => {
                let child: &Syntax = r;
                self.slot(r as *const _ as usize, child as *const Syntax as usize, at)?;
                self.node(child, at, backings)
            }
            Nil | Bool(_) | Int(_) | Float(_) => Ok(()),
        }
    }
}

/// One slice's backing bytes, and how they reach the file.
enum Backing {
    /// Bytes copied as they stand: a string or byte payload, a `Value` slice,
    /// a scope set — none of them has padding to leak.
    Raw { rel: u64, src: usize, len: usize },
    /// Struct entries, each assembled from probed extents. An entry's key is
    /// an enum whose padding a copy would carry into the artifact
    /// (docs/impl/image.md § Dumping).
    Entries { rel: u64, src: usize, count: usize },
}

impl Backing {
    /// `count` elements of `T` starting at `src`, landing at image offset
    /// `rel`, copied as bytes.
    fn raw<T>(rel: u64, src: usize, count: usize) -> Backing {
        Backing::Raw {
            rel,
            src,
            len: count * size_of::<T>(),
        }
    }

    fn entries(rel: u64, src: usize, count: usize) -> Backing {
        Backing::Entries { rel, src, count }
    }

    fn write(&self, pages: &mut [u8]) {
        match *self {
            Backing::Raw { rel, src, len } => {
                let bytes = unsafe { std::slice::from_raw_parts(src as *const u8, len) };
                pages[rel as usize..rel as usize + len].copy_from_slice(bytes);
            }
            Backing::Entries { rel, src, count } => {
                let entries =
                    unsafe { std::slice::from_raw_parts(src as *const (TableKey, Value), count) };
                let stride = size_of::<(TableKey, Value)>();
                for (i, e) in entries.iter().enumerate() {
                    let at = rel as usize + i * stride;
                    layout::write_canonical_entry(e, &mut pages[at..at + stride]);
                }
            }
        }
    }
}
