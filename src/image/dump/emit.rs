// audited: 2026-09-14
//! What the file gets from the copied graph: page bytes, the four relocation
//! streams, the object index, and the two watermarks.
//!
//! docs/impl/image.md
//! docs/impl/image/format.md
//!
//! One pass over the scratch region's live objects. Every record it writes —
//! an object slot, a struct entry, a syntax node, a code payload — is
//! assembled from probed extents into a zeroed buffer rather than copied
//! (backing.rs holds the writers), so no construction temporary's padding
//! reaches the artifact.

use std::collections::{HashMap, HashSet};
use std::mem::size_of;

use crate::hir::region::StaticRegion;
use crate::syntax::{ScopeId, Syntax, SyntaxKind};
use crate::value::closure::{CodePayload, LocEntry};
use crate::value::fiberheap::regionpool::{PageLayout, RegionPool};
use crate::value::heap::HeapObject;
use crate::value::region_slice::RegionSlice;
use crate::value::Value;

use super::super::format::Ctor;
use super::super::layout;
use super::super::ImageError;
use super::backing::Backing;
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
    /// One past the highest id any parameter carries.
    pub param_watermark: u32,
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
        param_watermark: 0,
    };
    let mut backings: Vec<Backing> = Vec::new();
    // Image offsets of the payloads already walked: a shared payload's inner
    // slots relocate once, however many headers name it (docs/impl/image/plan.md
    // § Test plan, "Closures"). Membership only — never iterated.
    let mut payloads_walked: HashSet<u64> = HashSet::new();

    for obj in pool.into_iter().flat_map(|p| p.live_objects()) {
        let addr = obj as *const HeapObject as usize;
        let obj_off = at.offset(addr)?;
        out.index.push((obj_off, obj.tag() as u64));
        let dst = obj_off as usize;
        layout::write_canonical(obj, &mut out.pages[dst..dst + size_of::<HeapObject>()]);
        out.traits_slot(obj, addr, at)?;
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
            // A default the hydrating instance rebuilds is nil in the copy, so
            // the slot call records nothing for it and the entry loop does the
            // work instead.
            HeapObject::Parameter { id, default, .. } => {
                out.param_watermark = out.param_watermark.max(id.saturating_add(1));
                out.value_slot(default, at)?;
            }
            // A closure's template field sits behind the `TemplateRef` seam,
            // so its slot comes off the field reference the seam hands out.
            HeapObject::Closure { closure, .. } => {
                out.value_slot(closure.template.value_ref(), at)?;
                out.values_backing(&closure.env, at, &mut backings)?;
                for v in closure.env.iter() {
                    out.value_slot(v, at)?;
                }
            }
            // A header is one slot — its payload slice — and the payload
            // behind it is a record of its own, assembled from probed
            // offsets like a node is. The blueprint field is not probed, so
            // the canonical shell leaves it zero and it hydrates as absent.
            // A later header naming the same payload records its own slot
            // and nothing else.
            HeapObject::ClosureTemplate(t) => {
                if let Some((rel, _)) = out.slice_backing(t.payload_slice(), at)? {
                    if payloads_walked.insert(rel) {
                        let payload = t.payload();
                        backings.push(Backing::payload(rel, payload));
                        out.payload(payload, at, &mut backings)?;
                    }
                }
            }
            HeapObject::Float(_) => {}
            other => {
                // copy_value only allocates the variants above.
                unreachable!("unexpected {:?} in dump scratch", other.tag());
            }
        }
    }
    // The copy walk named every reconstructed field by its address in the
    // scratch region, so all that is left is where each address lands.
    for (&field, &ctor) in reconstructions {
        let entry = (at.offset(field)?, ctor.encode());
        out.recons.push(entry);
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

    /// Record the relocation `obj`'s `traits` field needs. A reconstructed
    /// table is nil in the copy, so it leaves through the entry loop instead
    /// and this sees nothing (docs/impl/image/sealing.md).
    ///
    /// The field's own address comes from the probe, because the variants keep
    /// it in different places and the copy this walks is read through a shared
    /// `HeapObject` reference.
    fn traits_slot(
        &mut self,
        obj: &HeapObject,
        addr: usize,
        at: &Placement,
    ) -> Result<(), ImageError> {
        let Some(target) = obj.traits().as_heap_ptr() else {
            return Ok(());
        };
        let field = addr
            + layout::traits_slot_in(obj.tag())
                .expect("a traited object's variant carries a probed traits field");
        self.slot(field + layout::payload_in_value(), target as usize, at)
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

    /// Record every relocation a code payload's inner fields need, and their
    /// backing bytes. The payload struct itself is written canonically by its
    /// [`Backing`]; this walks what the struct names.
    fn payload(
        &mut self,
        p: &CodePayload,
        at: &Placement,
        backings: &mut Vec<Backing>,
    ) -> Result<(), ImageError> {
        if let Some((rel, src)) = self.slice_backing(&p.bytecode, at)? {
            backings.push(Backing::raw::<u8>(rel, src, p.bytecode.len()));
        }
        self.values_backing(&p.constants, at, backings)?;
        for v in p.constants.iter() {
            self.value_slot(v, at)?;
        }
        // A child is a header object of its own, so the walk above reaches it
        // like any other live object and this records only the slot that
        // names it (docs/impl/image/sealing.md).
        self.values_backing(&p.children, at, backings)?;
        for v in p.children.iter() {
            self.value_slot(v, at)?;
        }
        if let Some((rel, src)) = self.slice_backing(&p.locations, at)? {
            backings.push(Backing::raw::<LocEntry>(rel, src, p.locations.len()));
        }
        self.bytes_slices(&p.files, at, backings)?;
        for (field, len) in [(&p.name, p.name.len()), (&p.doc, p.doc.len())] {
            if let Some((rel, src)) = self.slice_backing(field, at)? {
                backings.push(Backing::raw::<u8>(rel, src, len));
            }
        }
        if let Some((rel, src)) = self.slice_backing(&p.region_table, at)? {
            backings.push(Backing::raw::<StaticRegion>(rel, src, p.region_table.len()));
        }
        if let Some((rel, src)) = self.slice_backing(&p.merged_slots, at)? {
            backings.push(Backing::raw::<u32>(rel, src, p.merged_slots.len()));
        }
        if let Some((rel, src)) = self.slice_backing(&p.frame_release_slots, at)? {
            backings.push(Backing::raw::<u16>(rel, src, p.frame_release_slots.len()));
        }
        if let Some((rel, src)) = self.slice_backing(&p.frame_release_regions, at)? {
            backings.push(Backing::raw::<u32>(rel, src, p.frame_release_regions.len()));
        }
        if let Some((rel, src)) = self.slice_backing(&p.capture_locals, at)? {
            backings.push(Backing::raw::<u64>(rel, src, p.capture_locals.len()));
        }
        self.bytes_slices(&p.strict_keys, at, backings)
    }

    /// A slice of byte slices — a payload's file names or its `&named` keys.
    /// The outer backing is slice headers the dumper assembles, because a
    /// header has padding after its length; each inner slice's bytes and
    /// `ptr` slot are recorded like a string's.
    fn bytes_slices(
        &mut self,
        s: &RegionSlice<RegionSlice<u8>>,
        at: &Placement,
        backings: &mut Vec<Backing>,
    ) -> Result<(), ImageError> {
        let Some((rel, src)) = self.slice_backing(s, at)? else {
            return Ok(());
        };
        backings.push(Backing::slice_headers(rel, src, s.len()));
        for inner in s.iter() {
            if let Some((inner_rel, inner_src)) = self.slice_backing(inner, at)? {
                backings.push(Backing::raw::<u8>(inner_rel, inner_src, inner.len()));
            }
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
