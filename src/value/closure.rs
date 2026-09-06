// audited: 2026-09-06
// docs/impl/region/template.md
//! Closure type for the Elle runtime
//!
//! `Closure` pairs a template with a captured environment and an optional
//! per-instance squelch mask. When non-zero, `squelch_mask` modifies the
//! effective signal: squelched bits are cleared and `SIG_ERROR` is added
//! (only when the closure could actually emit them). Use `effective_signal()`
//! externally; `template.signal()` is the underlying code's signal.
//!
//! A code object is three things — a compile-time [`TemplateProto`], a shared
//! region-resident [`CodePayload`], and the [`ClosureTemplate`] header a
//! closure references. docs/impl/region/template.md owns that argument.

use crate::signals::Signal;
use crate::value::fiber::SignalBits;
use crate::value::region_slice::RegionSlice;
use crate::value::Value;

pub(crate) mod cache;
mod header;
mod payload;
mod proto;

pub use header::ClosureTemplate;
pub use payload::{CodePayload, LocEntry, LocationTable, MaskRef, MergedSlots, StrKeys, VarargTag};
pub use proto::{materialize, TemplateProto, WasmClosureMeta};

/// A reference to a closure's per-definition code object.
///
/// The **seam** between a `Closure` and its `ClosureTemplate`, so the header's
/// storage can change without touching the call sites that read
/// `closure.template.<accessor>()`: `Deref` makes every such read transparent.
/// A template is always a region-allocated `HeapObject::ClosureTemplate` — an
/// ordinary, reclaimable allocation, no exception — held here as a `Value`.
///
/// The closure *instance* holds this as a normal cross-region reference
/// (increfed at the instance's alloc by `find_object_cross_refs`,
/// cascade-decrefed when its region frees), so a user-code template is
/// reclaimed by region RC rather than pinned for the process lifetime.
#[derive(Clone, Copy)]
pub struct TemplateRef(Value);

impl std::fmt::Debug for TemplateRef {
    /// Show the code object, not the pointer to it: a raw payload address tells
    /// a reader nothing, and every `Debug` of a `Closure` goes through here.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        (**self).fmt(f)
    }
}

impl TemplateRef {
    /// Wrap a region-allocated `HeapObject::ClosureTemplate` `Value`. The
    /// `Value` must point to a live template; `Deref` asserts the tag on access.
    pub fn region(template: Value) -> Self {
        TemplateRef(template)
    }

    /// The underlying `Value`, for the cross-region scan and for callers that
    /// must record the edge themselves.
    #[inline]
    pub fn value(&self) -> Value {
        self.0
    }
}

impl std::ops::Deref for TemplateRef {
    type Target = ClosureTemplate;
    #[inline]
    fn deref(&self) -> &ClosureTemplate {
        let obj: &'static crate::value::heap::HeapObject =
            unsafe { crate::value::arena::deref(self.0) };
        match obj {
            crate::value::heap::HeapObject::ClosureTemplate(t) => t,
            other => unreachable!(
                "a TemplateRef must point to a ClosureTemplate, got {}",
                other.type_name()
            ),
        }
    }
}

/// Materialize `proto` into a fresh region of `heap` and name it — the
/// test-scaffolding shape of what `MakeClosure` does, for tests that need a
/// code object without an executing frame to name its region.
#[cfg(test)]
pub fn test_template(
    heap: &mut crate::value::fiberheap::FiberHeap,
    proto: TemplateProto,
) -> TemplateRef {
    let region = heap.new_runtime_region();
    TemplateRef::region(materialize(heap, &std::rc::Rc::new(proto), region))
}

/// Closure with captured environment
#[derive(Debug, Clone)]
pub struct Closure {
    /// Shared per-definition data, behind the `TemplateRef` seam.
    pub template: TemplateRef,
    /// Captured environment (upvalues). Region-allocated `RegionSlice`; its
    /// lifetime matches the arena that allocated the enclosing Closure.
    pub env: RegionSlice<Value>,
    /// Per-instance squelch mask. Empty = no squelch; non-empty bits identify
    /// signals that are suppressed at the call boundary and converted to errors.
    pub squelch_mask: SignalBits,
}

impl Closure {
    /// Build a closure from a template reference, a captured environment, and a
    /// squelch mask. Prefer this over the struct literal so the template
    /// representation stays behind the `TemplateRef` seam.
    pub fn new(template: TemplateRef, env: RegionSlice<Value>, squelch_mask: SignalBits) -> Self {
        Closure {
            template,
            env,
            squelch_mask,
        }
    }

    /// Returns the effective signal of this closure, accounting for any squelch mask.
    /// When the squelch mask suppresses signals the closure may emit:
    /// - Suppressed bits are cleared from the result
    /// - SIG_ERROR is added (squelch converts suppressed signals to errors)
    ///
    /// When the mask doesn't suppress anything the closure emits, returns
    /// the template signal unchanged (no spurious SIG_ERROR added).
    pub fn effective_signal(&self) -> Signal {
        let signal = self.template.signal();
        if self.squelch_mask.is_empty() {
            return signal;
        }
        let template_bits = signal.bits;
        let actually_squelched = template_bits.intersection(self.squelch_mask);
        if actually_squelched.is_empty() {
            // Mask doesn't suppress anything this closure actually emits.
            return signal;
        }
        // Clear squelched bits; add SIG_ERROR (squelch converts to error)
        let new_bits = template_bits
            .subtract(self.squelch_mask)
            .union(crate::signals::SIG_ERROR);
        Signal {
            bits: new_bits,
            propagates: signal.propagates,
        }
    }

    /// Returns the underlying template signal, accounting for any squelch mask.
    /// Prefer effective_signal() for external consumers.
    /// Use template.signal() directly in JIT contexts where squelch must not
    /// affect code generation (the underlying bytecode still yields; squelch
    /// enforcement happens at the call boundary, not inside the JIT'd code).
    pub fn signal(&self) -> Signal {
        self.effective_signal()
    }

    /// Calculate the total environment capacity needed for a call.
    /// This is: existing captures + parameters + locally-defined variables.
    pub fn env_capacity(&self) -> usize {
        let num_params = self.template.num_params();
        let num_locally_defined = self.template.num_locals().saturating_sub(num_params);
        self.env.len() + num_params + num_locally_defined
    }
}

impl PartialEq for Closure {
    fn eq(&self, other: &Self) -> bool {
        let (a, b) = (&*self.template, &*other.template);
        // Two headers from one blueprint share a payload, so pointer identity
        // settles every code-object field at once. Distinct blueprints still
        // compare field by field: equality on closures is structural, and two
        // lambdas that compiled to the same code are the same function to a
        // caller.
        if std::ptr::eq(
            a.payload() as *const CodePayload,
            b.payload() as *const CodePayload,
        ) {
            return self.env == other.env && self.squelch_mask == other.squelch_mask;
        }
        a.bytecode() == b.bytecode()
            && a.arity() == b.arity()
            && self.env == other.env
            && a.num_locals() == b.num_locals()
            && a.num_captures() == b.num_captures()
            && a.constants() == b.constants()
            && a.signal() == b.signal()
            && a.capture_params_mask() == b.capture_params_mask()
            && a.capture_locals_mask().words() == b.capture_locals_mask().words()
            && a.locations().iter().eq(b.locations().iter())
            && a.doc() == b.doc()
            && a.vararg_tag() == b.vararg_tag()
            && a.num_params() == b.num_params()
            && a.name() == b.name()
            && self.squelch_mask == other.squelch_mask
    }
}

#[cfg(test)]
mod tests;
