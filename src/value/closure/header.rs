// audited: 2026-10-06
//! `ClosureTemplate` — the region-resident header of a code object: one slice naming its payload.
//!
//! docs/impl/region/template.md
//!
//! `MakeClosure` allocates one of these per closure creation, which is what
//! makes a closure built in a loop cheap.

use crate::hir::region::StaticRegion;
use crate::signals::Signal;
use crate::value::region_slice::RegionSlice;
use crate::value::types::Arity;

use super::payload::{
    CodePayload, LocationTable, MaskRef, MergedSlots, RestListLayout, StrKeys, VarargTag,
};

/// The code object a closure instance references: a payload in a code
/// region, and nothing else.
///
/// Never user-visible — it carries no `traits` and is never compared, hashed,
/// or serialized as a user value.
#[derive(Clone)]
pub struct ClosureTemplate {
    /// The payload, length one. Its backing lives in its compile unit's code
    /// region, so a header allocated in another region takes a counted
    /// cross-region reference to it (docs/impl/region/rules.md Rule 5); a
    /// header in the code region itself is a self-edge.
    payload: RegionSlice<CodePayload>,
}

impl std::fmt::Debug for ClosureTemplate {
    /// What a reader needs from a code object: which function it is, how it is
    /// called, and how big its body is. The payload slice is an address —
    /// nothing a diagnostic can use.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClosureTemplate")
            .field("name", &self.display_label())
            .field("arity", &self.arity())
            .field("bytecode", &format_args!("{} bytes", self.bytecode().len()))
            .field("num_locals", &self.num_locals())
            .field("num_captures", &self.num_captures())
            .finish()
    }
}

impl ClosureTemplate {
    /// A header over `payload`.
    pub(crate) fn new(payload: RegionSlice<CodePayload>) -> Self {
        ClosureTemplate { payload }
    }

    /// The payload.
    #[inline]
    pub fn payload(&self) -> &CodePayload {
        &self.payload.as_slice()[0]
    }

    /// The payload slice itself, for the image dumper: the header's one
    /// relocation slot is this slice's `ptr` (docs/impl/image/format.md).
    #[inline]
    pub(crate) fn payload_slice(&self) -> &RegionSlice<CodePayload> {
        &self.payload
    }

    /// Where the header keeps that slice, for the image layout probe. Lives
    /// here because the field is private.
    pub(crate) fn payload_slice_offset() -> usize {
        std::mem::offset_of!(ClosureTemplate, payload)
    }

    /// The pointer the code region owns — what the alloc scan turns into this
    /// header's counted cross-region reference.
    #[inline]
    pub(crate) fn payload_backing(&self) -> *const () {
        self.payload.as_ptr() as *const ()
    }

    // ── payload ────────────────────────────────────────────────────────

    #[inline]
    pub fn bytecode(&self) -> &[u8] {
        self.payload().bytecode()
    }

    #[inline]
    pub fn constants(&self) -> &[crate::value::Value] {
        self.payload().constants()
    }

    #[inline]
    pub fn arity(&self) -> Arity {
        self.payload().arity()
    }

    #[inline]
    pub fn signal(&self) -> Signal {
        self.payload().signal()
    }

    #[inline]
    pub fn num_locals(&self) -> usize {
        self.payload().num_locals()
    }

    #[inline]
    pub fn num_captures(&self) -> usize {
        self.payload().num_captures()
    }

    #[inline]
    pub fn num_params(&self) -> usize {
        self.payload().num_params()
    }

    #[inline]
    pub fn capture_params_mask(&self) -> u64 {
        self.payload().capture_params_mask()
    }

    #[inline]
    pub fn capture_locals_mask(&self) -> MaskRef<'_> {
        self.payload().capture_locals_mask()
    }

    #[inline]
    pub fn locations(&self) -> LocationTable<'_> {
        self.payload().locations()
    }

    #[inline]
    pub fn name(&self) -> Option<&'static str> {
        self.payload().name()
    }

    #[inline]
    pub fn doc(&self) -> Option<&'static str> {
        self.payload().doc()
    }

    #[inline]
    pub fn region_table(&self) -> &[StaticRegion] {
        self.payload().region_table()
    }

    #[inline]
    pub fn merged_slots(&self) -> MergedSlots<'_> {
        self.payload().merged_slots()
    }

    #[inline]
    pub fn frame_release_slots(&self) -> &[u16] {
        self.payload().frame_release_slots()
    }

    #[inline]
    pub fn frame_release_regions(&self) -> &[u32] {
        self.payload().frame_release_regions()
    }

    #[inline]
    pub fn vararg_tag(&self) -> VarargTag {
        self.payload().vararg_tag()
    }

    /// How a call to this code object builds its `&` rest list
    /// (docs/impl/region/restlist.md).
    #[inline]
    pub fn rest_list_layout(&self) -> RestListLayout {
        self.payload().rest_list_layout()
    }

    #[inline]
    pub fn strict_keys(&self) -> StrKeys<'_> {
        self.payload().strict_keys()
    }

    #[inline]
    pub fn wasm_func_idx(&self) -> Option<u32> {
        self.payload().wasm_func_idx()
    }

    /// The frozen function the JIT promotes this code object from, read out of
    /// the payload.
    #[inline]
    pub fn lir(&self) -> Option<crate::lir::LirView<'_>> {
        self.payload().lir()
    }

    /// Whether this code object carries LIR, without building a view: the
    /// question a call path asks before it reaches for a compiled tier.
    #[inline]
    pub fn has_lir(&self) -> bool {
        self.payload().has_lir()
    }

    /// Where the source lambda was written, for `(meta/origin f)`.
    #[inline]
    pub fn origin(&self) -> Option<crate::syntax::Span> {
        self.payload().origin()
    }

    // ── children ───────────────────────────────────────────────────────

    /// How many code objects this one's `MakeClosure` instructions index.
    #[inline]
    pub fn num_children(&self) -> usize {
        self.payload().children().len()
    }

    /// The header the `MakeClosure` at `idx` builds over, read out of the
    /// payload's child table.
    ///
    /// Panics for an index past the table, as a constant-pool read does: the
    /// index is baked into the instruction by the emitter that registered the
    /// child, so an out-of-range one is a corrupt code object.
    pub fn child(&self, idx: usize) -> ClosureTemplate {
        let value = self.payload().children()[idx];
        let obj: &'static crate::value::heap::HeapObject =
            unsafe { crate::value::arena::deref(value) };
        let crate::value::heap::HeapObject::ClosureTemplate(child) = obj else {
            unreachable!(
                "a child table entry is a code object, got {}",
                obj.type_name()
            );
        };
        child.clone()
    }

    // ── owned re-forms ─────────────────────────────────────────────────
    //
    // The boundaries that rebuild a code object somewhere else — the
    // cross-thread `send` encoder, the stdlib disk cache, a copy onto another
    // heap — want the compile-time shapes back. Each allocates, so they are
    // for those boundaries and not for the running VM, which reads the
    // payload's own form.

    /// The vararg kind in its owned compile-time form. The payload keeps the
    /// tag and the `&named` key set apart; this reassembles them.
    pub fn vararg_kind(&self) -> crate::hir::VarargKind {
        match self.vararg_tag() {
            VarargTag::List => crate::hir::VarargKind::List,
            VarargTag::Struct => crate::hir::VarargKind::Struct,
            VarargTag::StrictStruct => crate::hir::VarargKind::StrictStruct(
                self.strict_keys().iter().map(str::to_string).collect(),
            ),
        }
    }

    /// The location table as the emitter's offset-keyed map.
    pub fn location_map(&self) -> crate::error::LocationMap {
        self.locations().iter().collect()
    }

    /// The capture-locals mask as an owned mask.
    pub fn owned_capture_locals_mask(&self) -> crate::value::CaptureMask {
        crate::value::CaptureMask::from_words(self.capture_locals_mask().words().to_vec())
    }

    // ── derived ────────────────────────────────────────────────────────

    /// A human-readable label: the declared name when there is one, else the
    /// smallest-offset source location, else `<anon>`. Lowering names almost
    /// nothing, so the location is the label that actually identifies a
    /// function to a reader — the JIT code-address registry records it
    /// (docs/impl/jit.md § "The code-address registry"). The location table is
    /// ascending, so the smallest offset is its first entry.
    pub fn display_label(&self) -> String {
        if let Some(name) = self.name() {
            return name.to_string();
        }
        self.locations()
            .first()
            .map(|loc| format!("{}", loc))
            .unwrap_or_else(|| "<anon>".to_string())
    }

    /// The executable context for this code object. `Code` is this header and
    /// nothing else, so building one copies one slice.
    #[inline]
    pub fn code(&self) -> crate::value::Code {
        crate::value::Code::new(self.clone())
    }

    /// True if signal and structural checks pass for GPU eligibility.
    ///
    /// Necessary but not sufficient — the full `LirView::is_gpu_eligible`
    /// also walks instructions. Allows error-only signals (arithmetic ops on
    /// unboxed GPU scalars cannot type-error) but rejects yield, I/O, FFI, and
    /// polymorphism.
    pub fn is_gpu_candidate(&self) -> bool {
        let signal = self.signal();
        let non_error_bits = signal.bits.subtract(crate::signals::SIG_ERROR);
        non_error_bits.is_empty()
            && signal.propagates == 0
            && matches!(self.arity(), Arity::Exact(_))
            && self.capture_params_mask() == 0
            && self.capture_locals_mask().is_empty()
    }
}
