//! The template-derived execution context.
//!
//! When the VM runs a function body, it threads everything that comes from the
//! function's *code object* through the dispatch loop, the tail-call
//! trampoline, and the suspend/resume frames: the bytecode, the constant pool,
//! the location table, the nested-lambda blueprints, and the function's region
//! tables. These always travel together — they are the same code object — so
//! `Code` carries the code object itself rather than a bundle of parts.
//!
//! Since a [`ClosureTemplate`] is a payload slice plus a blueprint pointer
//! (docs/impl/region/template.md), `Code` adds no fields of its own and a
//! tail call that swaps the executing code object copies two words and bumps
//! one refcount. A NEW code-object field is added to the payload or the
//! blueprint and rides through suspend/resume and tail-call for free.
//!
//! The per-instance captured environment is deliberately NOT part of `Code`: an
//! `env` belongs to a closure *instance*, while a `Code` is shared by every
//! instance of the same lambda. The VM threads `(code, env)` as the full
//! execution context; a tail call to a different function swaps both.

use std::rc::Rc;

use crate::hir::region::StaticRegion;
use crate::value::closure::{ClosureTemplate, LocationTable, MergedSlots, TemplateProto};
use crate::value::Value;

/// A code object's executable context. See the module docs.
#[derive(Debug, Clone)]
pub struct Code {
    template: ClosureTemplate,
}

impl Code {
    /// Wrap a code object as the executing context.
    pub fn new(template: ClosureTemplate) -> Self {
        Code { template }
    }

    /// The code object itself.
    #[inline]
    pub fn template(&self) -> &ClosureTemplate {
        &self.template
    }

    /// Compiled bytecode for this code object.
    #[inline]
    pub fn bytecode(&self) -> &[u8] {
        self.template.bytecode()
    }

    /// Constant pool referenced by `LoadConst` and friends.
    #[inline]
    pub fn constants(&self) -> &[Value] {
        self.template.constants()
    }

    /// Bytecode offset → source location, for error reporting.
    #[inline]
    pub fn locations(&self) -> LocationTable<'_> {
        self.template.locations()
    }

    /// Blueprints for this code object's `MakeClosure` instructions. A
    /// `MakeClosure` indexes this list and materializes a fresh
    /// region-allocated header per execution, reclaimed by region RC.
    #[inline]
    pub fn child_protos(&self) -> &[Rc<TemplateProto>] {
        self.template.child_protos()
    }

    /// The static region slots this function's allocations SHARE after a
    /// builder-idiom merge (docs/impl/region/merging.md § Merging). The alloc
    /// dispatch consults it through `runtime_region_for_alloc_slot_maybe_merged`:
    /// a slot here mint-or-reuses (child mints, parent reuses) instead of always
    /// minting. Empty unless a merge fired (a nested `%pair` literal seeding the
    /// builder idiom), so byte-identical to the plain mint when no merge exists.
    #[inline]
    pub fn merged_slots(&self) -> MergedSlots<'_> {
        self.template.merged_slots()
    }

    /// The local slots this function's **value-routed** releases read, ascending
    /// (docs/impl/region/mechanism.md § "An abandoned frame runs the releases it
    /// still owes"). A value route is `LoadLocal s; DecrefValueRegion;
    /// StoreLocal s nil`, so the slot is the release's whole identity and the nil
    /// stamp records that it ran. An error exit walks these slots and releases
    /// what each still holds — the releases the abandoned frame owed and can no
    /// longer reach. Empty for a body with no value route, so the walk is then a
    /// single length check.
    #[inline]
    pub fn frame_release_slots(&self) -> &[u16] {
        self.template.frame_release_slots()
    }

    /// The static region slots this function's **slot-routed** releases name,
    /// ascending — the `DecrefRegion` half of [`Self::frame_release_slots`]
    /// (docs/impl/region/mechanism.md § "An abandoned frame runs the releases it
    /// still owes"). That route's receipt is the activation region map: the alloc
    /// mints the mapping and the release takes it, so a slot still mapped when the
    /// frame is abandoned is a release that did not run.
    #[inline]
    pub fn frame_release_regions(&self) -> &[u32] {
        self.template.frame_release_regions()
    }

    /// This function's compile-time region slots, each ≥ 2.
    #[inline]
    pub fn region_table(&self) -> &[StaticRegion] {
        self.template.region_table()
    }

    /// How many stack positions this code object's entry prologue reserves for
    /// the frame's locals.
    ///
    /// The VM addresses local `n` as `frame_base + n` on the same stack it uses
    /// for operands, so the emitter opens every function body with this many
    /// `Nil` pushes and operands then live above them (`lir::emit::emit_block`).
    /// That makes the count a floor the operand stack must never fall through:
    /// popping into the reserved region destroys a live local, and the damage
    /// only surfaces later, when a `LoadLocal` of a high slot reads past the end
    /// of the stack. `VM::debug_assert_locals_intact` checks the floor at each
    /// instruction in debug builds.
    ///
    /// `0` for a code object with no prologue — a bootstrap or `eval` thunk —
    /// where the check is vacuous.
    #[inline]
    pub fn reserved_locals(&self) -> usize {
        self.template.num_locals()
    }
}
