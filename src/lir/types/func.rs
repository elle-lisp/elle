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
    /// Signal of this function (Pure, Yields, or Polymorphic)
    pub signal: Signal,
    /// Optional docstring from the source lambda. Plain `Rc<str>` compile-time
    /// data, never a heap `Value` — materialized as a fresh ordinary
    /// (reclaimable) allocation on `(doc f)`.
    #[serde(skip)]
    pub doc: Option<std::rc::Rc<str>>,
    /// Original lambda Syntax node for eval environment reconstruction
    #[serde(skip)]
    pub syntax: Option<std::rc::Rc<crate::syntax::Syntax>>,
    /// How varargs are collected: List (pair chain) or Struct (immutable struct).
    /// Only meaningful when arity is AtLeast.
    pub vararg_kind: crate::hir::VarargKind,
    /// Total number of parameter slots (required + optional + rest if present).
    /// Used by VM populate_env to know how many fixed slots to fill.
    pub num_params: usize,
    /// Number of non-LBox parameters copied to local slots.
    /// These occupy the first `num_local_params` positions in `num_locals`.
    /// The `capture_locals_mask` indexes from position `num_local_params`.
    pub num_local_params: usize,
    /// Yield point metadata, populated during bytecode emission.
    /// Indexed by yield point order (0, 1, 2, ...).
    /// Empty for non-yielding functions.
    pub yield_points: Vec<YieldPointInfo>,
    /// Call site metadata, populated during bytecode emission.
    /// Only populated for functions where `signal.may_suspend()`.
    /// Indexed by call instruction order (0, 1, 2, ...).
    pub call_sites: Vec<CallSiteInfo>,
    /// Per-function region table: the set of compile-time region slots
    /// (`StaticRegion`, each ≥ 2) the lowerer minted for this function.
    /// Built by the lowerer from region inference; propagated to
    /// `ClosureTemplate`.
    pub region_table: Vec<StaticRegion>,
    /// Static region slots SHARED by ≥2 of this function's allocations after a
    /// builder-idiom merge (docs/impl/region/merging.md § Merging). Recorded by
    /// `record_merged_slots` (the root slot a merge tree's allocations resolve to,
    /// via `static_slot`'s `merged_root` canonicalization), and propagated to
    /// `ClosureTemplate`/`Bytecode` so the alloc dispatch mint-or-reuses them. Empty
    /// unless a merge fired, so byte-identical to the plain mint on the default path.
    pub merged_slots: Vec<StaticRegion>,
    /// The local slots this function's **value-routed** releases read, ascending
    /// and deduplicated (docs/impl/region/mechanism.md § "An abandoned frame runs
    /// the releases it still owes"). Recorded by `emit_decref_for_region` where it
    /// emits the plain `LoadLocal s; DecrefValueRegion; StoreLocal s nil` route —
    /// so a route the emitter declined records nothing — and propagated to
    /// `ClosureTemplate`/`Bytecode` so an error exit can run the releases the
    /// abandoned frame still owed.
    pub frame_release_slots: Vec<u16>,
    /// The static region slots this function's **slot-routed** releases name — the
    /// `DecrefRegion` half of the same table (docs/impl/region/mechanism.md § "An
    /// abandoned frame runs the releases it still owes"). Recorded by
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
    /// (call_inner line 192: `self.fiber.stack.drain(..).collect()`).
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
            syntax: None,
            vararg_kind: crate::hir::VarargKind::List,
            num_params,
            num_local_params: 0,
            yield_points: Vec::new(),
            call_sites: Vec::new(),
            region_table: Vec::new(),
            merged_slots: Vec::new(),
            frame_release_slots: Vec::new(),
            frame_release_regions: Vec::new(),
        }
    }

    /// True if any block contains a SuspendingCall instruction.
    pub fn has_suspending_call(&self) -> bool {
        self.blocks.iter().any(|b| {
            b.instructions
                .iter()
                .any(|si| matches!(si.instr, LirInstr::SuspendingCall { .. }))
        })
    }

    /// True if this function is eligible for GPU compilation.
    ///
    /// GPU-eligible functions use only numeric operations (arithmetic,
    /// comparison, local variable access, control flow) with no heap
    /// allocation, closures, function calls, or signal emission.
    ///
    /// Checked in order of increasing cost:
    /// 1. Signal check (cheapest — just field reads)
    /// 2. Structural check (arity, captures, cells)
    /// 3. Instruction whitelist (walks all basic blocks)
    pub fn is_gpu_eligible(&self) -> bool {
        // Signal: allow error-only (arithmetic type errors can't happen on
        // unboxed GPU scalars), reject yield/IO/FFI/polymorphic
        let non_error = self.signal.bits.subtract(crate::signals::SIG_ERROR);
        if !non_error.is_empty() || self.signal.propagates != 0 {
            return false;
        }
        // Structural: no variadics, no mutable cells
        if !matches!(self.arity, Arity::Exact(_)) {
            return false;
        }
        if self.capture_params_mask != 0 || !self.capture_locals_mask.is_empty() {
            return false;
        }
        // Instruction whitelist: every instruction and terminator must be GPU-safe
        self.blocks.iter().all(|b| {
            b.instructions
                .iter()
                .all(|si| is_gpu_instruction(&si.instr))
                && is_gpu_terminator(&b.terminator.terminator)
        })
    }

    /// True if this function is safe for the CPU MLIR tier-2 path.
    ///
    /// Stricter than `is_gpu_eligible`: the return register must be
    /// producible from numeric operations only. MLIR represents all
    /// values as i64, so nil (→ 0) can't round-trip back when the
    /// function is called from regular Elle code. Bool/Compare results
    /// are safe — the caller reboxes them as `Value::bool(result != 0)`.
    ///
    /// GPU dispatch (via `gpu:map`) doesn't have this problem — the
    /// caller reads integers out of a buffer and treats them as integers.
    pub fn is_mlir_cpu_eligible(&self) -> bool {
        if !self.is_gpu_eligible() {
            return false;
        }
        for block in &self.blocks {
            if let Terminator::Return(reg) = &block.terminator.terminator {
                if self.register_reaches_non_int(*reg) {
                    return false;
                }
            }
        }
        true
    }

    /// True if `target` is transitively produced by a non-numeric value
    /// source (Nil constant or IntToFloat conversion). Walks backward
    /// through definitions — Const sources, LoadLocal/StoreLocal chains.
    /// LoadCapture is treated as int (args are validated at call site).
    /// Bool constants and Compare results are i64 0/1 at the MLIR level;
    /// the caller reboxes as `Value::bool(result != 0)`.
    fn register_reaches_non_int(&self, target: Reg) -> bool {
        use std::collections::HashSet;
        let mut regs_to_check: Vec<Reg> = vec![target];
        let mut seen_regs: HashSet<u32> = HashSet::new();
        let mut seen_slots: HashSet<u16> = HashSet::new();
        while let Some(r) = regs_to_check.pop() {
            if !seen_regs.insert(r.0) {
                continue;
            }
            for block in &self.blocks {
                for si in &block.instructions {
                    match &si.instr {
                        LirInstr::Const {
                            dst,
                            value: LirConst::Nil,
                        } if *dst == r => return true,
                        // ValueConst nil is non-int (same as Const nil)
                        LirInstr::ValueConst { dst, value } if *dst == r && value.is_nil() => {
                            return true;
                        }
                        LirInstr::Convert {
                            dst,
                            op: ConvOp::IntToFloat,
                            ..
                        } if *dst == r => return true,
                        // FloatToInt produces an int — safe, no action needed
                        LirInstr::LoadLocal { dst, slot }
                            if *dst == r && seen_slots.insert(*slot) =>
                        {
                            for b2 in &self.blocks {
                                for si2 in &b2.instructions {
                                    if let LirInstr::StoreLocal { slot: s, src } = &si2.instr {
                                        if *s == *slot {
                                            regs_to_check.push(*src);
                                        }
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        false
    }

    /// Convert ValueConst instructions to Const (LirConst) for safe cross-thread transfer.
    /// NativeFn ValueConsts are safe to keep as-is (function pointers are Send+Sync).
    /// Closure ValueConsts are converted to `ClosureRef(idx)` using the intern table.
    /// Returns false if any ValueConst contains a non-sendable, non-closure heap value.
    pub fn convert_value_consts_for_send(
        &mut self,
        visited: &std::collections::HashMap<u64, usize>,
    ) -> bool {
        for block in &mut self.blocks {
            for si in &mut block.instructions {
                if let LirInstr::ValueConst { dst, value } = &si.instr {
                    if value.is_native_fn() {
                        continue; // function pointers are thread-safe
                    }
                    let dst = *dst;
                    if let Some(lir_const) = value_to_lir_const(*value) {
                        si.instr = LirInstr::Const {
                            dst,
                            value: lir_const,
                        };
                    } else if value.is_closure() {
                        // Closure ValueConst: look up in intern table.
                        //
                        // This branch fires whenever a closure being sent
                        // across a `sys/spawn` boundary contains, in its
                        // LIR, a `ValueConst` holding a closure Value. That
                        // happens because stdlib functions are registered
                        // as primitives (see
                        // `src/primitives/module_init.rs::register_stdlib_exports`
                        // which calls `CompileCtx::register_stdlib_exports`), so user
                        // code referencing a stdlib function inside a
                        // lambda lowers the reference to `ValueConst` via
                        // `immutable_values` in the lowerer. A spawned
                        // closure that transitively calls e.g. `inc` or
                        // `map` from stdlib will trip this branch.
                        //
                        // `CLOSURE_VALUE_CONST_COUNT` tracks the live count;
                        // see the `lir/closure-value-const-count` primitive
                        // and `--stats` output.
                        CLOSURE_VALUE_CONST_COUNT.fetch_add(1, Ordering::Relaxed);
                        if let Some(&idx) = visited.get(&value.payload) {
                            si.instr = LirInstr::Const {
                                dst,
                                value: LirConst::ClosureRef(idx),
                            };
                        } else {
                            return false;
                        }
                    } else {
                        // unsendable ValueConst (compound heap value)
                        return false;
                    }
                }
            }
        }
        true
    }
}

#[cfg(test)]
mod tests;
