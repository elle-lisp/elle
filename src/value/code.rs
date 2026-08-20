//! The template-derived execution context.
//!
//! When the VM runs a function body, it threads everything that comes from the
//! function's *code object* (its `ClosureTemplate`) through the dispatch loop,
//! the tail-call trampoline, and the suspend/resume frames: the bytecode, the
//! constant pool, and the location map. These always travel together — they are
//! the same code object — so `Code` carries them together as one value rather
//! than as separate `Rc` parameters on every execution function and separate
//! fields on every suspended frame.
//!
//! Bundling them into one value lets the core thread a single reference,
//! and — load-bearing for the closure-template work in docs/impl/region/model.md
//! (§ "Constants lower as ordinary allocations") — so a NEW code-object field
//! (`child_protos`, the per-definition nested-lambda blueprints) is added in one
//! place and rides through suspend/resume/tail-call for free.
//!
//! The per-instance captured environment is deliberately NOT part of `Code`: an
//! `env` belongs to a closure *instance*, while a `Code` is shared by every
//! instance of the same lambda. The VM threads `(code, env)` as the full
//! execution context; a tail call to a different function swaps both.
//!
//! Every field is an `Rc`, so `Code` is cheap to clone (three refcount bumps) —
//! exactly what the yield/call handlers and tail-call swap need.

use std::rc::Rc;

use rustc_hash::FxHashSet;

use crate::error::LocationMap;
use crate::value::closure::ClosureTemplate;
use crate::value::Value;

thread_local! {
    /// Shared empty merged-slot set — the default for `Code` objects built
    /// without a region-merging function (the bootstrap/eval thunks). Cloning it
    /// is one `Rc` bump, no allocation, so `Code::new` never allocates an empty
    /// set per call.
    static EMPTY_MERGED_SLOTS: Rc<FxHashSet<u32>> = Rc::new(FxHashSet::default());
}

/// The shared empty merged-slot set (an `Rc` bump). Used by `Code::new`'s default
/// and by any site that has no merge metadata to carry.
pub(crate) fn empty_merged_slots() -> Rc<FxHashSet<u32>> {
    EMPTY_MERGED_SLOTS.with(Rc::clone)
}

thread_local! {
    /// Shared empty frame-release tables — the default for `Code` objects built
    /// without a lowered function (the bootstrap/eval thunks). One `Rc` bump,
    /// no allocation.
    static EMPTY_FRAME_RELEASE_SLOTS: Rc<Vec<u16>> = Rc::new(Vec::new());
    static EMPTY_FRAME_RELEASE_REGIONS: Rc<Vec<u32>> = Rc::new(Vec::new());
}

/// The shared empty value-route release table (an `Rc` bump). Used by `Code::new`'s
/// default and by any site with no lowered function to carry them from.
pub(crate) fn empty_frame_release_slots() -> Rc<Vec<u16>> {
    EMPTY_FRAME_RELEASE_SLOTS.with(Rc::clone)
}

/// The shared empty slot-route release table (an `Rc` bump).
pub(crate) fn empty_frame_release_regions() -> Rc<Vec<u32>> {
    EMPTY_FRAME_RELEASE_REGIONS.with(Rc::clone)
}

/// The per-function region tables a `Code` carries beside its bytecode: the
/// builder-idiom merge set the alloc dispatch mint-or-reuses, and the two
/// abandoned-frame release tables an error exit walks. They travel together
/// because they come from one place — the function's `LirFunction`, through its
/// `ClosureTemplate` or its `Bytecode` — so an entry that builds a `Code` from raw
/// parts hands them over as one value rather than as three parameters.
#[derive(Debug, Clone)]
pub struct CodeTables {
    pub merged_slots: Rc<FxHashSet<u32>>,
    pub frame_release_slots: Rc<Vec<u16>>,
    pub frame_release_regions: Rc<Vec<u32>>,
}

impl Default for CodeTables {
    /// The shared empty tables — three `Rc` bumps, no allocation. What a body
    /// with no lowered function carries (a bootstrap or synthetic thunk).
    fn default() -> Self {
        CodeTables {
            merged_slots: empty_merged_slots(),
            frame_release_slots: empty_frame_release_slots(),
            frame_release_regions: empty_frame_release_regions(),
        }
    }
}

/// A code object's executable context: bytecode, constant pool, location map,
/// and nested-lambda blueprints, shared across all instances of one lambda.
/// See the module docs.
#[derive(Debug, Clone)]
pub struct Code {
    /// Compiled bytecode for this code object.
    pub bytecode: Rc<Vec<u8>>,
    /// Constant pool referenced by `LoadConst` and friends.
    pub constants: Rc<Vec<Value>>,
    /// Bytecode offset → source location, for error reporting.
    pub location_map: Rc<LocationMap>,
    /// Blueprints for this code object's `MakeClosure` instructions. A
    /// `MakeClosure` indexes this list and materializes a fresh region-allocated
    /// `HeapObject::ClosureTemplate` per execution, reclaimed by region RC.
    /// Rides through suspend/resume and tail-call with the rest of the code
    /// object (see the module docs).
    pub child_protos: Rc<Vec<Rc<ClosureTemplate>>>,
    /// The static region slots this function's allocations SHARE after a
    /// builder-idiom merge (docs/impl/region/merging.md § Merging). The alloc
    /// dispatch consults it through `runtime_region_for_alloc_slot_maybe_merged`:
    /// a slot here mint-or-reuses (child mints, parent reuses) instead of always
    /// minting. Empty unless a merge fired (a nested `%pair` literal seeding the
    /// builder idiom), so byte-identical to the plain mint when no merge exists.
    pub merged_slots: Rc<FxHashSet<u32>>,
    /// The local slots this function's **value-routed** releases read, ascending
    /// (docs/impl/region/mechanism.md § "An abandoned frame runs the releases it
    /// still owes"). A value route is `LoadLocal s; DecrefValueRegion;
    /// StoreLocal s nil`, so the slot is the release's whole identity and the nil
    /// stamp records that it ran. An error exit walks these slots and releases
    /// what each still holds — the releases the abandoned frame owed and can no
    /// longer reach. Empty for a body with no value route, so the walk is then a
    /// single length check.
    pub frame_release_slots: Rc<Vec<u16>>,
    /// The static region slots this function's **slot-routed** releases name,
    /// ascending — the `DecrefRegion` half of [`Self::frame_release_slots`]
    /// (docs/impl/region/mechanism.md § "An abandoned frame runs the releases it
    /// still owes"). That route's receipt is the activation region map: the alloc
    /// mints the mapping and the release takes it, so a slot still mapped when the
    /// frame is abandoned is a release that did not run.
    pub frame_release_regions: Rc<Vec<u32>>,
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
    pub reserved_locals: usize,
}

impl Code {
    /// Bundle the code-object Rcs into a `Code`. The three region tables default
    /// to the shared empty ones ([`CodeTables`]) and `reserved_locals` to `0`; a
    /// caller that carries a lowered function's tables or a local-reserving
    /// prologue (the per-call `ClosureTemplate::code`) sets them after
    /// construction.
    pub fn new(
        bytecode: Rc<Vec<u8>>,
        constants: Rc<Vec<Value>>,
        location_map: Rc<LocationMap>,
        child_protos: Rc<Vec<Rc<ClosureTemplate>>>,
    ) -> Self {
        Code {
            bytecode,
            constants,
            location_map,
            child_protos,
            merged_slots: empty_merged_slots(),
            frame_release_slots: empty_frame_release_slots(),
            frame_release_regions: empty_frame_release_regions(),
            reserved_locals: 0,
        }
    }

    /// Install the function's region tables. The entry path
    /// (`VM::execute_bytecode`) builds a `Code` from raw parts and calls this with
    /// what its `Bytecode`/`ClosureTemplate` carries.
    pub fn with_tables(mut self, tables: CodeTables) -> Self {
        self.merged_slots = tables.merged_slots;
        self.frame_release_slots = tables.frame_release_slots;
        self.frame_release_regions = tables.frame_release_regions;
        self
    }
}
