//! Closure type for the Elle runtime
//!
//! `Closure` pairs a template with a captured environment and an optional
//! per-instance squelch mask. When non-zero, `squelch_mask` modifies the
//! effective signal: squelched bits are cleared and `SIG_ERROR` is added
//! (only when the closure could actually emit them). Use `effective_signal()`
//! externally; `template.signal` is the underlying code's signal.

use crate::error::LocationMap;
use crate::hir::region::StaticRegion;
use crate::signals::Signal;
use crate::value::fiber::SignalBits;
use crate::value::region_slice::RegionSlice;
use crate::value::types::Arity;
use crate::value::CaptureMask;
use crate::value::Value;
use std::collections::HashMap;
use std::rc::Rc;

/// Per-definition closure data shared across all instances of the same lambda.
#[derive(Debug, Clone)]
pub struct ClosureTemplate {
    /// Compiled bytecode for this closure
    pub bytecode: Rc<Vec<u8>>,
    /// Function arity specification
    pub arity: Arity,
    /// Total number of local slots needed
    pub num_locals: usize,
    /// Number of captured variables (for env layout)
    pub num_captures: usize,
    /// Total number of parameter slots (required + optional + rest if present).
    pub num_params: usize,
    /// Constant pool for this closure
    pub constants: Rc<Vec<Value>>,
    /// Signal of the closure body
    pub signal: Signal,
    /// Bitmask indicating which parameters need box wrapping.
    /// Bit i set means parameter i is mutated and needs a LocalLBox.
    pub capture_params_mask: u64,
    /// Which locally-defined variables need box wrapping (a capture cell).
    /// Slot i set means locally-defined variable i needs a LocalLBox. Unbounded
    /// in width (see `CaptureMask`) — an uncaptured local at any index gets a
    /// bare-NIL env slot, never a leaked dead cell.
    pub capture_locals_mask: CaptureMask,
    /// Symbol ID → name mapping for cross-thread portability.
    pub symbol_names: Rc<HashMap<u32, String>>,
    /// Bytecode offset → source location mapping for error reporting.
    pub location_map: Rc<LocationMap>,
    /// LIR function for deferred JIT compilation.
    pub lir_function: Option<Rc<crate::lir::LirFunction>>,
    /// Module's closure list for JIT MakeClosure resolution.
    /// Optional docstring from the source lambda. Plain `Rc<str>` compile-time
    /// data riding the template, never a heap `Value`; `(doc f)` materializes a
    /// fresh ordinary (reclaimable) string from it on demand.
    pub doc: Option<Rc<str>>,
    /// Original syntax node for eval environment reconstruction
    pub syntax: Option<Rc<crate::syntax::Syntax>>,
    /// How varargs are collected (List or Struct).
    /// Only meaningful when arity is AtLeast.
    pub vararg_kind: crate::hir::VarargKind,
    /// Optional name of this closure (for debugging/stack traces).
    pub name: Option<Rc<str>>,
    /// WASM function table index (if compiled to WASM backend).
    /// When set, rt_call dispatches to this WASM function instead of bytecode.
    pub wasm_func_idx: Option<u32>,
    /// Cached SPIR-V bytes (write-once). Populated by `(git f)`.
    /// SPIR-V is a property of the code, not the instance — all closures
    /// from the same lambda share the template, so the cache is shared.
    pub spirv: std::cell::OnceCell<Vec<u8>>,
    /// Per-function region table: the compile-time region slots
    /// (`StaticRegion`, each ≥ 2) this template's function minted, cloned
    /// from its `LirFunction`. Empty when the function has no allocations
    /// (the common case). Every slot is ≥ 2 (slot 1 is reserved and never minted).
    pub region_table: Vec<StaticRegion>,
    /// The static region slots this function's allocations SHARE after a
    /// builder-idiom merge (docs/impl/region/merging.md § Merging), cloned from its
    /// `LirFunction` and threaded into the executing `Code` (`code()`), where the
    /// alloc dispatch consults it for mint-or-reuse. Empty unless a merge fired,
    /// so byte-identical to the plain mint on the default path.
    pub merged_slots: Rc<rustc_hash::FxHashSet<u32>>,
    /// Blueprints for the nested lambdas this code object's `MakeClosure`
    /// instructions materialize. Plain compile-time data (one `Rc` per nested
    /// lambda), **not** heap `Value`s in any region — a `MakeClosure` indexes
    /// this list and materializes a FRESH region-allocated
    /// `HeapObject::ClosureTemplate` per execution (a heap literal is an ordinary,
    /// reclaimable allocation; closure templates are no exception). Reclaimed by
    /// region RC when this template drops.
    pub child_protos: Rc<Vec<Rc<ClosureTemplate>>>,
}

impl ClosureTemplate {
    /// Create a new ClosureTemplate with the three required fields;
    /// all other fields default to zero/empty/None.
    pub fn new(bytecode: Rc<Vec<u8>>, arity: Arity, constants: Rc<Vec<Value>>) -> Self {
        ClosureTemplate {
            bytecode,
            arity,
            num_locals: 0,
            num_captures: 0,
            num_params: 0,
            constants,
            signal: Signal::silent(),
            capture_params_mask: 0,
            capture_locals_mask: CaptureMask::empty(),
            symbol_names: Rc::new(HashMap::new()),
            location_map: Rc::new(LocationMap::new()),
            lir_function: None,
            doc: None,
            syntax: None,
            vararg_kind: crate::hir::VarargKind::List,
            name: None,
            wasm_func_idx: None,
            spirv: std::cell::OnceCell::new(),
            region_table: Vec::new(),
            merged_slots: Rc::new(rustc_hash::FxHashSet::default()),
            child_protos: Rc::new(Vec::new()),
        }
    }

    /// The executable code object for this template: bundles the bytecode,
    /// constant pool, and location map (and the child protos)
    /// that the VM threads as the template-derived half of the execution
    /// context. See [`crate::value::Code`]. Cheap — clones three `Rc`s.
    pub fn code(&self) -> crate::value::Code {
        let mut code = crate::value::Code::new(
            self.bytecode.clone(),
            self.constants.clone(),
            self.location_map.clone(),
            self.child_protos.clone(),
        );
        // Carry this function's merge metadata into the executing context so the
        // alloc dispatch can mint-or-reuse merged slots (an `Rc` bump, not an
        // allocation). Empty unless a merge fired, so inert on the default path.
        code.merged_slots = self.merged_slots.clone();
        // The body's prologue reserves one stack position per local, so operands
        // sit above them; carry the count so the dispatch loop can check that
        // nothing pops into the reserved region (`Code::reserved_locals`).
        code.reserved_locals = self.num_locals;
        code
    }

    /// True if signal and structural checks pass for GPU eligibility.
    ///
    /// This is a necessary but not sufficient condition — the full
    /// `LirFunction::is_gpu_eligible()` also walks instructions.
    /// Use this for cheap runtime queries on compiled closures.
    /// True if signal and structural checks pass for GPU eligibility.
    ///
    /// Allows SIG_ERROR (arithmetic type errors can't happen on unboxed GPU
    /// scalars) but rejects yield, I/O, FFI, and polymorphism.
    pub fn is_gpu_candidate(&self) -> bool {
        // Allow error-only signals (arithmetic ops on unboxed types can't type-error)
        let non_error_bits = self.signal.bits.subtract(crate::signals::SIG_ERROR);
        non_error_bits.is_empty()
            && self.signal.propagates == 0
            && matches!(self.arity, Arity::Exact(_))
            && self.capture_params_mask == 0
            && self.capture_locals_mask.is_empty()
    }
}

/// A reference to a closure's per-definition template.
///
/// This is the **seam** between a `Closure` and its `ClosureTemplate` so the
/// template's storage can change without touching the ~190 call sites that read
/// `closure.template.<field>`: `Deref` makes every such read transparent
/// (a closure template is an ordinary, reclaimable allocation, no exception).
/// Two representations live behind it:
///
/// - `Shared(Rc<ClosureTemplate>)` — the `Rc`-shared form, used by the
///   bootstrap sites that have no natural region (the fiber/ffi thunks, the
///   JIT-suspend reconstruct, cross-thread send deserialization, the top-level
///   entry thunk, and unit tests).
/// - `Region(Value)` — a region-allocated `HeapObject::ClosureTemplate` the
///   `MakeClosure` opcode materializes per execution; the closure *instance*
///   holds this as a normal cross-region reference (increfed at the instance's
///   alloc by `find_object_cross_refs`, cascade-decsumed when its region frees).
///   User-code templates are reclaimed by region RC, not pinned for the
///   process lifetime.
///
/// `Deref` resolves a `Region` Value through the arena; the materialized
/// template lives as long as the closure that references it (co-region RC), so
/// the deref is sound for the instance's lifetime.
#[derive(Debug, Clone)]
pub enum TemplateRef {
    /// `Rc`-shared template — bootstrap sites with no natural region.
    Shared(Rc<ClosureTemplate>),
    /// Region-allocated `HeapObject::ClosureTemplate`, referenced by `Value`.
    Region(Value),
}

impl TemplateRef {
    /// Wrap an `Rc<ClosureTemplate>` as a `Shared` template reference.
    pub fn new(template: Rc<ClosureTemplate>) -> Self {
        TemplateRef::Shared(template)
    }

    /// Wrap a region-allocated `HeapObject::ClosureTemplate` `Value` as a
    /// `Region` template reference. The `Value` must point to a live
    /// `HeapObject::ClosureTemplate`; `Deref` asserts the tag on access.
    pub fn region(template: Value) -> Self {
        TemplateRef::Region(template)
    }
}

impl std::ops::Deref for TemplateRef {
    type Target = ClosureTemplate;
    #[inline]
    fn deref(&self) -> &ClosureTemplate {
        match self {
            TemplateRef::Shared(rc) => rc,
            TemplateRef::Region(v) => {
                let obj: &'static crate::value::heap::HeapObject =
                    unsafe { crate::value::arena::deref(*v) };
                match obj {
                    crate::value::heap::HeapObject::ClosureTemplate(t) => t,
                    other => unreachable!(
                        "TemplateRef::Region must point to a ClosureTemplate, got {}",
                        other.type_name()
                    ),
                }
            }
        }
    }
}

impl From<Rc<ClosureTemplate>> for TemplateRef {
    fn from(t: Rc<ClosureTemplate>) -> Self {
        TemplateRef::Shared(t)
    }
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
    /// Build a closure from a template (anything convertible into a
    /// `TemplateRef` — e.g. an `Rc<ClosureTemplate>`), a captured environment,
    /// and a squelch mask. Prefer this over the struct literal so the template
    /// representation stays behind the `TemplateRef` seam.
    pub fn new(
        template: impl Into<TemplateRef>,
        env: RegionSlice<Value>,
        squelch_mask: SignalBits,
    ) -> Self {
        Closure {
            template: template.into(),
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
        if self.squelch_mask.is_empty() {
            return self.template.signal;
        }
        let template_bits = self.template.signal.bits;
        let actually_squelched = template_bits.intersection(self.squelch_mask);
        if actually_squelched.is_empty() {
            // Mask doesn't suppress anything this closure actually emits.
            return self.template.signal;
        }
        // Clear squelched bits; add SIG_ERROR (squelch converts to error)
        let new_bits = template_bits
            .subtract(self.squelch_mask)
            .union(crate::signals::SIG_ERROR);
        Signal {
            bits: new_bits,
            propagates: self.template.signal.propagates,
        }
    }

    /// Returns the underlying template signal, accounting for any squelch mask.
    /// Prefer effective_signal() for external consumers.
    /// Use template.signal directly in JIT contexts where squelch must not
    /// affect code generation (the underlying bytecode still yields; squelch
    /// enforcement happens at the call boundary, not inside the JIT'd code).
    pub fn signal(&self) -> Signal {
        self.effective_signal()
    }

    /// Calculate the total environment capacity needed for a call.
    /// This is: existing captures + parameters + locally-defined variables.
    pub fn env_capacity(&self) -> usize {
        let num_locally_defined = self
            .template
            .num_locals
            .saturating_sub(self.template.num_params);
        self.env.len() + self.template.num_params + num_locally_defined
    }
}

impl PartialEq for Closure {
    fn eq(&self, other: &Self) -> bool {
        self.template.bytecode == other.template.bytecode
            && self.template.arity == other.template.arity
            && self.env == other.env
            && self.template.num_locals == other.template.num_locals
            && self.template.num_captures == other.template.num_captures
            && self.template.constants == other.template.constants
            && self.template.signal == other.template.signal
            && self.template.capture_params_mask == other.template.capture_params_mask
            && self.template.capture_locals_mask == other.template.capture_locals_mask
            && self.template.symbol_names == other.template.symbol_names
            && self.template.location_map == other.template.location_map
            && self.template.doc == other.template.doc
            && self.template.vararg_kind == other.template.vararg_kind
            && self.template.num_params == other.template.num_params
            && self.template.name == other.template.name
            && self.squelch_mask == other.squelch_mask
    }
}

// Tests migrated to tests/elle/value-closure.lisp

#[cfg(test)]
mod tests;
