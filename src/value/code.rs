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
//! and — load-bearing for the closure-template work in docs/impl/region-model.md
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
    /// builder-idiom merge (docs/impl/region-model.md § Merging). The alloc
    /// dispatch consults it through `runtime_region_for_alloc_slot_maybe_merged`:
    /// a slot here mint-or-reuses (child mints, parent reuses) instead of always
    /// minting. Empty unless a merge fired (a nested `%pair` literal under
    /// `--checked-intrinsics=off`), so byte-identical to the plain mint on the
    /// default path.
    pub merged_slots: Rc<FxHashSet<u32>>,
}

impl Code {
    /// Bundle the code-object Rcs into a `Code`. `merged_slots` defaults to the
    /// shared empty set; a caller that carries merge metadata (the per-call
    /// `ClosureTemplate::code`) sets `merged_slots` after construction.
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
        }
    }
}
