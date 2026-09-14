// audited: 2026-09-14
// docs/impl/region/template.md
// docs/impl/image/sealing.md
//! `CodePayload` — a code object's variable-length data, inline in region pages.
//!
//! One payload per compile-time blueprint, materialized once per heap and
//! shared by every header built from that blueprint. Nothing here owns Rust
//! heap memory: the payload's bytes *are* the payload, which is what an image
//! needs of body data.

use crate::hir::region::StaticRegion;
use crate::reader::SourceLoc;
use crate::signals::Signal;
use crate::syntax::Span;
use crate::value::region_slice::RegionSlice;
use crate::value::types::Arity;
use crate::value::Value;

/// How a lambda's rest parameter collects its arguments. The `&named` key set
/// itself is variable-length, so it lives beside this tag in the payload rather
/// than inside it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum VarargTag {
    /// `&rest` — collect into a list.
    List,
    /// `&keys` — collect into an immutable struct, any key.
    Struct,
    /// `&named` — collect into an immutable struct, keys validated against the
    /// declared set ([`CodePayload::strict_keys`]).
    StrictStruct,
}

/// One bytecode offset's source location: the offset, an index into the
/// payload's interned file table, and the line and column.
///
/// Four `u32`s rather than a `SourceLoc`, whose `file: String` is a Rust heap
/// owner per entry — unsealable at any price, and a hash map of them is not
/// byte-self-contained either.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct LocEntry {
    /// The bytecode offset this location describes.
    pub offset: u32,
    /// Index into the payload's file table.
    pub file: u32,
    pub line: u32,
    pub col: u32,
}

/// A code object's payload: every variable-length field of one lambda, inline
/// in region pages. `Copy`, so a header names it with a `RegionSlice` of length
/// one and copies nothing.
/// The fields are `pub(crate)` rather than `pub(super)` for one consumer: the
/// image dumper reads them to copy a payload and to name its relocation slots
/// (docs/impl/image/sealing.md). Everything else goes through the accessors.
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct CodePayload {
    pub(crate) bytecode: RegionSlice<u8>,
    pub(crate) constants: RegionSlice<Value>,
    /// Ascending by `offset`, so a lookup is a binary search and the
    /// smallest-offset location is the first entry.
    pub(crate) locations: RegionSlice<LocEntry>,
    /// File names interned once per payload; `LocEntry::file` indexes this.
    pub(crate) files: RegionSlice<RegionSlice<u8>>,
    pub(crate) name: RegionSlice<u8>,
    pub(crate) doc: RegionSlice<u8>,
    pub(crate) region_table: RegionSlice<StaticRegion>,
    /// Ascending, so membership is a binary search.
    pub(crate) merged_slots: RegionSlice<u32>,
    pub(crate) frame_release_slots: RegionSlice<u16>,
    pub(crate) frame_release_regions: RegionSlice<u32>,
    /// The capture-locals mask's words. Unbounded in width, so an uncaptured
    /// local at any index gets a bare-NIL env slot rather than a dead cell.
    pub(crate) capture_locals: RegionSlice<u64>,
    /// The `&named` key set, empty unless `vararg` is `StrictStruct`.
    pub(crate) strict_keys: RegionSlice<RegionSlice<u8>>,
    /// The code objects this function's `MakeClosure` instructions index, in
    /// instruction order, each a `HeapObject::ClosureTemplate`. Empty on a
    /// materialized payload, because the header that owns it answers from its
    /// blueprint; the image dumper fills it, because the blueprint is the
    /// part that cannot cross (docs/impl/image/sealing.md).
    pub(crate) children: RegionSlice<Value>,
    /// Where the source lambda was written, for `(meta/origin f)`. Meaningful
    /// only when `has_origin`, because a span of zeros is a real answer for
    /// the first form of a file.
    ///
    /// The `FileId` inside it indexes a process-wide interner, so it is the
    /// one process-local number a payload holds and an image rewrites it from
    /// the file table (docs/impl/image/format.md).
    pub(crate) origin: Span,
    pub(crate) has_origin: bool,
    pub(crate) arity: Arity,
    pub(crate) signal: Signal,
    pub(crate) capture_params_mask: u64,
    pub(crate) num_locals: u32,
    pub(crate) num_captures: u32,
    pub(crate) num_params: u32,
    /// The WASM function-table index, meaningful only when `has_wasm_idx`.
    /// Split from an `Option<u32>` so every byte of the field is written: an
    /// enum's niche-less `None` carries uninitialized payload bytes, which the
    /// image's canonical writer could not reproduce.
    pub(crate) wasm_func_idx: u32,
    pub(crate) has_wasm_idx: bool,
    pub(crate) vararg: VarargTag,
    /// Whether `name`/`doc` are present at all: an absent docstring and an
    /// empty one are different answers to `(doc f)`, and both are empty slices.
    pub(crate) has_name: bool,
    pub(crate) has_doc: bool,
}

/// A payload's source-location table.
#[derive(Clone, Copy, Debug)]
pub struct LocationTable<'a> {
    entries: &'a [LocEntry],
    files: &'a [RegionSlice<u8>],
}

impl<'a> LocationTable<'a> {
    pub(super) fn new(entries: &'a [LocEntry], files: &'a [RegionSlice<u8>]) -> Self {
        LocationTable { entries, files }
    }

    /// The location recorded for `offset`, if any. A binary search — correct
    /// only because the table is built ascending.
    pub fn get(&self, offset: usize) -> Option<SourceLoc> {
        let offset = u32::try_from(offset).ok()?;
        let ix = self
            .entries
            .binary_search_by_key(&offset, |e| e.offset)
            .ok()?;
        Some(self.source_loc(&self.entries[ix]))
    }

    /// The smallest-offset location, which is the first entry.
    pub fn first(&self) -> Option<SourceLoc> {
        self.entries.first().map(|e| self.source_loc(e))
    }

    pub fn entries(&self) -> &'a [LocEntry] {
        self.entries
    }

    pub fn files(&self) -> &'a [RegionSlice<u8>] {
        self.files
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Every entry as a `(offset, location)` pair, ascending.
    pub fn iter(&self) -> impl Iterator<Item = (usize, SourceLoc)> + '_ {
        self.entries
            .iter()
            .map(|e| (e.offset as usize, self.source_loc(e)))
    }

    fn source_loc(&self, e: &LocEntry) -> SourceLoc {
        let file = self
            .files
            .get(e.file as usize)
            .map(|f| str_of(*f))
            .unwrap_or("");
        SourceLoc::new(file, e.line as usize, e.col as usize)
    }
}

/// A payload's builder-idiom merge set (docs/impl/region/merging.md § Merging).
/// Empty unless a merge fired, so the common case is a length check.
#[derive(Clone, Copy, Debug)]
pub struct MergedSlots<'a>(&'a [u32]);

impl<'a> MergedSlots<'a> {
    /// View `slots` as a merge set. The slice must be ascending — membership is
    /// a binary search, so an unsorted one answers wrongly rather than slowly.
    /// The payload's own set is sorted where it is built, which is where that
    /// invariant is asserted; this constructor is on the alloc-resolution path
    /// and does not re-walk the slice.
    pub fn from_sorted(slots: &'a [u32]) -> Self {
        MergedSlots(slots)
    }

    /// Membership by binary search — correct only because the set is stored
    /// ascending.
    #[inline]
    pub fn contains(&self, slot: u32) -> bool {
        !self.0.is_empty() && self.0.binary_search(&slot).is_ok()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn as_slice(&self) -> &'a [u32] {
        self.0
    }
}

/// A borrowed capture-locals mask: which local slots need a capture cell.
#[derive(Clone, Copy, Debug)]
pub struct MaskRef<'a>(&'a [u64]);

impl<'a> MaskRef<'a> {
    pub(super) fn new(words: &'a [u64]) -> Self {
        MaskRef(words)
    }

    #[inline]
    pub fn is_set(&self, i: usize) -> bool {
        let w = i / 64;
        w < self.0.len() && (self.0[w] & (1u64 << (i % 64))) != 0
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.iter().all(|&w| w == 0)
    }

    pub fn words(&self) -> &'a [u64] {
        self.0
    }
}

/// The `&named` key set a strict-struct collector validates against.
#[derive(Clone, Copy, Debug)]
pub struct StrKeys<'a>(&'a [RegionSlice<u8>]);

impl<'a> StrKeys<'a> {
    pub(super) fn new(keys: &'a [RegionSlice<u8>]) -> Self {
        StrKeys(keys)
    }

    pub fn contains(&self, key: &str) -> bool {
        self.0.iter().any(|k| str_of(*k) == key)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &'a str> + 'a {
        self.0.iter().map(|k| str_of(*k))
    }
}

/// Read a region-inline byte slice as `str`. The bytes came from a `String` or
/// `&str` at materialization, so they are UTF-8 by construction; a torn read
/// would mean the payload region was freed under a live header, which the
/// counted backing reference exists to prevent.
pub(super) fn str_of(slice: RegionSlice<u8>) -> &'static str {
    let bytes: &'static [u8] = unsafe { std::slice::from_raw_parts(slice.as_ptr(), slice.len()) };
    std::str::from_utf8(bytes).expect("a code payload's strings are UTF-8 by construction")
}

impl CodePayload {
    /// A payload carrying `constants` and nothing else, for the store-level
    /// tests that build heap objects straight into a `RegionStore` and so have
    /// no `FiberHeap` to materialize a blueprint through.
    #[cfg(test)]
    pub(crate) fn test_with_constants(constants: RegionSlice<Value>) -> Self {
        CodePayload {
            bytecode: RegionSlice::empty(),
            constants,
            locations: RegionSlice::empty(),
            files: RegionSlice::empty(),
            name: RegionSlice::empty(),
            doc: RegionSlice::empty(),
            region_table: RegionSlice::empty(),
            merged_slots: RegionSlice::empty(),
            frame_release_slots: RegionSlice::empty(),
            frame_release_regions: RegionSlice::empty(),
            capture_locals: RegionSlice::empty(),
            strict_keys: RegionSlice::empty(),
            children: RegionSlice::empty(),
            origin: Span::synthetic(),
            has_origin: false,
            arity: Arity::Exact(0),
            signal: Signal::silent(),
            capture_params_mask: 0,
            num_locals: 0,
            num_captures: 0,
            num_params: 0,
            wasm_func_idx: 0,
            has_wasm_idx: false,
            vararg: VarargTag::List,
            has_name: false,
            has_doc: false,
        }
    }

    pub fn bytecode(&self) -> &[u8] {
        self.bytecode.as_slice()
    }

    pub fn constants(&self) -> &[Value] {
        self.constants.as_slice()
    }

    pub fn locations(&self) -> LocationTable<'_> {
        LocationTable::new(self.locations.as_slice(), self.files.as_slice())
    }

    pub fn name(&self) -> Option<&'static str> {
        self.has_name.then(|| str_of(self.name))
    }

    pub fn doc(&self) -> Option<&'static str> {
        self.has_doc.then(|| str_of(self.doc))
    }

    pub fn region_table(&self) -> &[StaticRegion] {
        self.region_table.as_slice()
    }

    pub fn merged_slots(&self) -> MergedSlots<'_> {
        MergedSlots::from_sorted(self.merged_slots.as_slice())
    }

    pub fn frame_release_slots(&self) -> &[u16] {
        self.frame_release_slots.as_slice()
    }

    pub fn frame_release_regions(&self) -> &[u32] {
        self.frame_release_regions.as_slice()
    }

    pub fn capture_locals_mask(&self) -> MaskRef<'_> {
        MaskRef::new(self.capture_locals.as_slice())
    }

    pub fn strict_keys(&self) -> StrKeys<'_> {
        StrKeys::new(self.strict_keys.as_slice())
    }

    /// The code objects a `MakeClosure` indexes, empty unless this payload
    /// came out of an image (docs/impl/image/sealing.md).
    pub fn children(&self) -> &[Value] {
        self.children.as_slice()
    }

    /// Where the source lambda was written, or `None` for a lambda no reader
    /// produced.
    pub fn origin(&self) -> Option<Span> {
        self.has_origin.then_some(self.origin)
    }

    pub fn arity(&self) -> Arity {
        self.arity
    }

    pub fn signal(&self) -> Signal {
        self.signal
    }

    pub fn capture_params_mask(&self) -> u64 {
        self.capture_params_mask
    }

    pub fn num_locals(&self) -> usize {
        self.num_locals as usize
    }

    pub fn num_captures(&self) -> usize {
        self.num_captures as usize
    }

    pub fn num_params(&self) -> usize {
        self.num_params as usize
    }

    pub fn wasm_func_idx(&self) -> Option<u32> {
        self.has_wasm_idx.then_some(self.wasm_func_idx)
    }

    pub fn vararg_tag(&self) -> VarargTag {
        self.vararg
    }
}
