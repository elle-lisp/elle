//! `ClosureTemplate` — the region-resident header of a code object.
//!
//! Two words: a `RegionSlice` naming the shared payload, and an `Rc` to the
//! blueprint it was materialized from. `MakeClosure` allocates one of these per
//! closure creation, which is what makes a closure built in a loop cheap
//! (docs/impl/region/template.md).

use std::cell::OnceCell;
use std::rc::Rc;

use crate::hir::region::StaticRegion;
use crate::signals::Signal;
use crate::value::region_slice::RegionSlice;
use crate::value::types::Arity;
use crate::value::Value;

use super::payload::{CodePayload, LocationTable, MaskRef, MergedSlots, StrKeys, VarargTag};
use super::proto::TemplateProto;

/// The code object a closure instance references: a shared payload plus the
/// blueprint that made it.
///
/// Never user-visible — it carries no `traits` and is never compared, hashed,
/// or serialized as a user value.
#[derive(Clone)]
pub struct ClosureTemplate {
    /// The shared payload, length one. Its backing lives in a payload region of
    /// the heap's own, so allocating a header takes a counted cross-region
    /// reference to it (docs/impl/region/rules.md Rule 5).
    payload: RegionSlice<CodePayload>,
    /// The blueprint this header came from — the one Rust-heap owner left on a
    /// code object. It answers what the payload cannot yet hold: the
    /// nested-lambda blueprints a `MakeClosure` indexes, the LIR the JIT
    /// promotes from, the defining span, and the SPIR-V cache. Holding it
    /// strongly is also what stops the heap's payload cache from sweeping a
    /// payload this header still reads.
    proto: Rc<TemplateProto>,
}

impl std::fmt::Debug for ClosureTemplate {
    /// What a reader needs from a code object: which function it is, how it is
    /// called, and how big its body is. The payload slice and the blueprint
    /// pointer are addresses — nothing a diagnostic can use.
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
    pub(super) fn new(payload: RegionSlice<CodePayload>, proto: Rc<TemplateProto>) -> Self {
        ClosureTemplate { payload, proto }
    }

    /// A header over a payload the caller allocated, for the store-level tests
    /// that have no `FiberHeap` to materialize a blueprint through.
    #[cfg(test)]
    pub(crate) fn test_header(payload: RegionSlice<CodePayload>, proto: Rc<TemplateProto>) -> Self {
        ClosureTemplate { payload, proto }
    }

    /// The code object for `proto` on `heap`, with no header allocated in any
    /// region.
    ///
    /// An entry thunk's or module body's code object is *executed*, never
    /// referenced by a closure, so it needs no heap identity — but it must
    /// still reach its bytecode the way every other code object does, or the
    /// entry paths become a second shape for a synthetic thunk to drift into.
    /// The blueprint held here keeps the payload's cache entry, and so its
    /// region, alive for as long as this code object.
    pub fn for_proto(
        heap: &mut crate::value::fiberheap::FiberHeap,
        proto: &Rc<TemplateProto>,
    ) -> Self {
        ClosureTemplate::new(heap.template_payload(proto), Rc::clone(proto))
    }

    /// The shared payload.
    #[inline]
    pub fn payload(&self) -> &CodePayload {
        &self.payload.as_slice()[0]
    }

    /// The pointer the payload's region owns — what the alloc scan turns into
    /// this header's counted cross-region reference.
    #[inline]
    pub(crate) fn payload_backing(&self) -> *const () {
        self.payload.as_ptr() as *const ()
    }

    /// The blueprint this header was materialized from.
    #[inline]
    pub fn proto(&self) -> &Rc<TemplateProto> {
        &self.proto
    }

    // ── payload ────────────────────────────────────────────────────────

    #[inline]
    pub fn bytecode(&self) -> &[u8] {
        self.payload().bytecode()
    }

    #[inline]
    pub fn constants(&self) -> &[Value] {
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

    #[inline]
    pub fn strict_keys(&self) -> StrKeys<'_> {
        self.payload().strict_keys()
    }

    #[inline]
    pub fn wasm_func_idx(&self) -> Option<u32> {
        self.payload().wasm_func_idx()
    }

    // ── blueprint ──────────────────────────────────────────────────────

    #[inline]
    pub fn child_protos(&self) -> &[Rc<TemplateProto>] {
        &self.proto.child_protos
    }

    #[inline]
    pub fn lir_function(&self) -> Option<&Rc<crate::lir::LirFunction>> {
        self.proto.lir_function.as_ref()
    }

    #[inline]
    pub fn origin(&self) -> Option<crate::syntax::Span> {
        self.proto.origin
    }

    #[inline]
    pub fn spirv(&self) -> &OnceCell<Vec<u8>> {
        &self.proto.spirv
    }

    // ── owned re-forms ─────────────────────────────────────────────────
    //
    // The boundaries that rebuild a *blueprint* out of a code object — the
    // cross-thread `send` encoder and the stdlib disk cache — want the
    // compile-time shapes back. Each allocates, so they are for those
    // boundaries and not for the running VM, which reads the payload's own
    // form.

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
    /// nothing else, so building one copies two words and bumps one refcount.
    #[inline]
    pub fn code(&self) -> crate::value::Code {
        crate::value::Code::new(self.clone())
    }

    /// True if signal and structural checks pass for GPU eligibility.
    ///
    /// Necessary but not sufficient — the full `LirFunction::is_gpu_eligible`
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
