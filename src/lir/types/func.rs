// audited: 2026-10-06
//! A lowered function in the working form — its blocks, registers and constants —
//! and the site records emission produces for it.
//!
//! docs/impl/lir.md

use super::*;

/// A LIR function (compilation unit)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LirFunction {
    /// This closure's identity in the module's closure list.
    /// `None` for the entry function and standalone tests.
    pub closure_id: Option<ClosureId>,
    /// Function name (for debugging)
    pub name: Option<String>,
    /// Function arity (Exact for fixed, AtLeast for variadic)
    pub arity: Arity,
    /// Basic blocks
    pub blocks: Vec<BasicBlock>,
    /// Entry block label
    pub entry: Label,
    /// Constants used by this function
    pub constants: Vec<LirConst>,
    /// Number of registers used
    pub num_regs: u32,
    /// Number of local slots needed
    pub num_locals: u16,
    /// Number of captured variables
    /// Used by JIT to distinguish captures (from env) from parameters (from args)
    pub num_captures: u16,
    /// Bitmask indicating which parameters need to be wrapped in capture cells
    /// Bit i is set if parameter i needs a capture cell (for mutable parameters)
    pub capture_params_mask: u64,
    /// Which locally-defined variables need capture cells.
    /// Slot i is set if locally-defined variable i needs a capture cell (captured
    /// or mutated). Slots without the bit are stored directly (stack slot, no
    /// cell), avoiding heap allocation on every call. Unbounded in width (see
    /// `CaptureMask`): a local at any index is named precisely, so an uncaptured
    /// high local is never conservatively (and leakily) celled.
    pub capture_locals_mask: crate::value::CaptureMask,
    /// Signal of this function (Silent, Yields, or Polymorphic)
    pub signal: Signal,
    /// Optional docstring from the source lambda. Plain `Rc<str>` compile-time
    /// data, never a heap `Value` — materialized as a fresh ordinary
    /// (reclaimable) allocation on `(doc f)`.
    #[serde(skip)]
    pub doc: Option<std::rc::Rc<str>>,
    /// Where this lambda was written, for `(meta/origin f)`.
    ///
    /// Skipped by serde, like `doc` beside it: nothing rebuilds a
    /// `ClosureTemplate` from serialized LIR — the stdlib cache restores
    /// templates from cached bytecode — so encoding it would grow every cache
    /// file for a field no restore path reads.
    #[serde(skip)]
    pub origin: Option<crate::syntax::Span>,
    /// How varargs are collected: List (pair chain) or Struct (immutable struct).
    /// Only meaningful when arity is AtLeast.
    pub vararg_kind: crate::hir::VarargKind,
    /// How a `&` rest list is built: the region analysis' verdict on this
    /// lambda (docs/impl/region/restlist.md), written by the lowerer and
    /// frozen with the function for the JIT prologue and the blueprint.
    pub rest_list_layout: crate::value::RestListLayout,
    /// Total number of parameter slots (required + optional + rest if present).
    /// Used by VM populate_env to know how many fixed slots to fill.
    pub num_params: usize,
    /// Number of non-LBox parameters copied to local slots.
    /// These occupy the first `num_local_params` positions in `num_locals`.
    /// The `capture_locals_mask` indexes from position `num_local_params`.
    pub num_local_params: usize,
    /// Per-function region table: the set of compile-time region slots
    /// (`StaticRegion`, each ≥ 2) the lowerer minted for this function.
    /// Built by the lowerer from region inference; propagated to
    /// `ClosureTemplate`.
    pub region_table: Vec<StaticRegion>,
    /// Static region slots SHARED by ≥2 of this function's allocations after a
    /// builder-idiom merge (docs/impl/region/merging.md). Recorded by
    /// `record_merged_slots` (the root slot a merge tree's allocations resolve to,
    /// via `static_slot`'s `merged_root` canonicalization), and propagated to
    /// `ClosureTemplate`/`Bytecode` so the alloc dispatch mint-or-reuses them. Empty
    /// unless a merge fired, so byte-identical to the plain mint on the default path.
    pub merged_slots: Vec<StaticRegion>,
    /// The local slots this function's **value-routed** releases read, ascending
    /// and deduplicated (docs/impl/region/mechanism.md). Recorded by
    /// `emit_decref_for_region` where it emits the plain `LoadLocal s;
    /// DecrefValueRegion; StoreLocal s nil` route — so a route the emitter
    /// declined records nothing — and propagated to `ClosureTemplate`/`Bytecode`
    /// so an error exit can run the releases the abandoned frame still owed.
    pub frame_release_slots: Vec<u16>,
    /// The static region slots this function's **slot-routed** releases name — the
    /// `DecrefRegion` half of the same table (docs/impl/region/mechanism.md). Recorded by
    /// `emit_decref_region`, so a suppressed or phantom release records nothing;
    /// the activation map is that route's receipt, the release taking the mapping
    /// as it runs.
    pub frame_release_regions: Vec<StaticRegion>,
}

/// Metadata about a yield point, collected during bytecode emission.
/// The JIT reads this to know how to spill registers and where to
/// resume in the interpreter.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct YieldPointInfo {
    /// Bytecode IP to resume at (the instruction after the Yield opcode).
    /// This is the IP stored in the SuspendedFrame so the interpreter
    /// can resume from the correct point.
    pub resume_ip: usize,
    /// Registers on the operand stack at the yield point, bottom-to-top.
    /// The JIT spills these Cranelift variables in this order to
    /// reconstruct the interpreter's operand stack on resume.
    pub stack_regs: Vec<Reg>,
    /// Number of local variable slots (params + locally-defined).
    /// The interpreter stores locals at `[frame_base, frame_base + num_locals)`.
    /// The JIT must spill local values first, then operand stack registers,
    /// so the SuspendedFrame stack matches the interpreter's layout.
    pub num_locals: u16,
}

/// Metadata about a call site, collected during bytecode emission.
/// The JIT reads this to know the bytecode IP at each call instruction,
/// which is needed to build SuspendedFrames for yield-through-call.
///
/// Only populated for functions where `signal.may_suspend()`.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CallSiteInfo {
    /// Bytecode IP after the Call instruction and its operands.
    /// This is the IP the interpreter would store in SuspendedFrame.ip
    /// when yield propagates through this call.
    pub resume_ip: usize,
    /// Registers on the operand stack at the call site, after popping
    /// func and args but before pushing the result. This matches the
    /// interpreter's stack state when yield propagates through a call
    /// (`complete_call` parks it with `self.fiber.stack.drain(..).collect()`).
    pub stack_regs: Vec<Reg>,
    /// Number of local variable slots (params + locally-defined).
    /// The interpreter stores locals at `[frame_base, frame_base + num_locals)`.
    /// The JIT must spill local values first, then operand stack registers,
    /// so the SuspendedFrame stack matches the interpreter's layout.
    pub num_locals: u16,
}

impl LirFunction {
    pub fn new(arity: Arity) -> Self {
        let num_params = arity.fixed_params();
        LirFunction {
            closure_id: None,
            name: None,
            arity,
            blocks: Vec::new(),
            entry: Label(0),
            constants: Vec::new(),
            num_regs: 0,
            num_locals: 0,
            num_captures: 0,
            capture_params_mask: 0,
            capture_locals_mask: crate::value::CaptureMask::empty(),
            signal: Signal::silent(),
            doc: None,
            origin: None,
            vararg_kind: crate::hir::VarargKind::List,
            rest_list_layout: crate::value::RestListLayout::PerCell,
            num_params,
            num_local_params: 0,
            region_table: Vec::new(),
            merged_slots: Vec::new(),
            frame_release_slots: Vec::new(),
            frame_release_regions: Vec::new(),
        }
    }
}
