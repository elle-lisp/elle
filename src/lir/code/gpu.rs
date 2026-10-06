// audited: 2026-10-06
//! Whether a frozen function may run on the GPU or the MLIR CPU tier: numeric work only.
//!
//! docs/impl/mlir.md
//! docs/impl/region/diagnostics.md

use super::instr::InstrRef;
use super::operand::ConstRef;
use super::view::LirView;
use crate::lir::{ConvOp, Reg, Terminator};
use crate::value::Arity;
use std::collections::HashSet;

/// True if this instruction is safe for GPU compilation: numeric constants,
/// arithmetic, comparison, local and parameter access. Everything else needs
/// the heap, closures, calls or signals.
///
/// `LoadCapture`/`LoadCaptureRaw` are parameter or capture loads; captures
/// arrive as extra parameters at the MLIR level.
fn is_gpu_instruction(i: &InstrRef<'_>) -> bool {
    match i {
        InstrRef::Const {
            value: ConstRef::Int(_) | ConstRef::Float(_) | ConstRef::Bool(_) | ConstRef::Nil,
            ..
        }
        | InstrRef::BinOp { .. }
        | InstrRef::UnaryOp { .. }
        | InstrRef::Compare { .. }
        | InstrRef::Convert { .. }
        | InstrRef::LoadLocal { .. }
        | InstrRef::StoreLocal { .. }
        | InstrRef::StoreLocalRefcounted { .. }
        | InstrRef::LoadCapture { .. }
        | InstrRef::LoadCaptureRaw { .. } => true,
        // Value-targeted region refcounts are no-ops on unboxed GPU
        // scalars (ints/floats carry no region) — the MLIR/SPIR-V
        // lowerers skip them. Every instruction that could put a heap
        // value in a register is rejected by this whitelist, so the
        // skipped refcounts can never unbalance a real region.
        InstrRef::IncrefValueRegion { .. } | InstrRef::DecrefValueRegion { .. } => true,
        // ValueConst of numeric/bool/nil types is GPU-safe — these are
        // immutable binding constants inlined by the lowerer.
        InstrRef::ValueConst { value, .. } => {
            value.is_int() || value.is_float() || value.as_bool().is_some() || value.is_nil()
        }
        // LoadSelf reads the executing-closure register — VM/JIT execution-context
        // state that has no meaning on an unboxed GPU scalar tier. Excluded
        // explicitly so a value-position self-reference is never GPU-dispatched.
        InstrRef::LoadSelf { .. } => false,
        // The activation adopt reaches the fiber's owner-node stack — VM/JIT
        // execution-context state with no meaning on the GPU tier. Excluded
        // explicitly so a function carrying it is never GPU-dispatched.
        InstrRef::AdoptIntoActivation { .. } => false,
        _ => false,
    }
}

/// True if this block terminator is safe for GPU compilation: return, jump,
/// branch. An `Emit` means the function deliberately signals — even `:error`
/// via `(error ...)` is not GPU-safe.
fn is_gpu_terminator(t: &Terminator) -> bool {
    matches!(
        t,
        Terminator::Return(_) | Terminator::Jump(_) | Terminator::Branch { .. }
    )
}

impl<'a> LirView<'a> {
    /// True if this function is eligible for GPU compilation: numeric
    /// operations (arithmetic, comparison, local variable access, control
    /// flow) with no heap allocation, closures, calls or signal emission.
    ///
    /// Checked in order of increasing cost: the signal, then the structure
    /// (arity, captures, cells), then every instruction.
    pub fn is_gpu_eligible(&self) -> bool {
        // Signal: allow error-only (arithmetic type errors can't happen on
        // unboxed GPU scalars), reject yield/IO/FFI/polymorphic.
        let signal = self.signal();
        let non_error = signal.bits.subtract(crate::signals::SIG_ERROR);
        if !non_error.is_empty() || signal.propagates != 0 {
            return false;
        }
        // Structural: no variadics, no mutable cells.
        if !matches!(self.arity(), Arity::Exact(_)) {
            return false;
        }
        if self.capture_params_mask() != 0 || !self.capture_locals_mask().is_empty() {
            return false;
        }
        self.blocks().all(|b| {
            b.instrs().all(|i| is_gpu_instruction(&i)) && is_gpu_terminator(&b.terminator())
        })
    }

    /// True if this function is safe for the CPU MLIR tier-2 path.
    ///
    /// Stricter than `is_gpu_eligible`: the return register must be
    /// producible from numeric operations only. MLIR represents all values as
    /// i64, so nil (→ 0) can't round-trip back when the function is called
    /// from regular Elle code. Bool/Compare results are safe — the caller
    /// reboxes them as `Value::bool(result != 0)`.
    ///
    /// GPU dispatch (via `gpu:map`) doesn't have this problem — the caller
    /// reads integers out of a buffer and treats them as integers.
    pub fn is_mlir_cpu_eligible(&self) -> bool {
        if !self.is_gpu_eligible() {
            return false;
        }
        self.blocks().all(|b| match b.terminator() {
            Terminator::Return(reg) => !self.register_reaches_non_int(reg),
            _ => true,
        })
    }

    /// True if `target` is transitively produced by a non-numeric value
    /// source (a nil constant or an IntToFloat conversion). Walks backward
    /// through definitions — constants, and LoadLocal/StoreLocal chains.
    /// LoadCapture counts as int (args are validated at the call site).
    /// Bool constants and Compare results are i64 0/1 at the MLIR level; the
    /// caller reboxes them as `Value::bool(result != 0)`.
    fn register_reaches_non_int(&self, target: Reg) -> bool {
        let mut regs_to_check: Vec<Reg> = vec![target];
        let mut seen_regs: HashSet<u32> = HashSet::new();
        let mut seen_slots: HashSet<u16> = HashSet::new();
        while let Some(r) = regs_to_check.pop() {
            if !seen_regs.insert(r.0) {
                continue;
            }
            for instr in self.nodes().map(|n| n.instr()) {
                match instr {
                    InstrRef::Const {
                        dst,
                        value: ConstRef::Nil,
                    } if dst == r => return true,
                    // ValueConst nil is non-int (same as Const nil)
                    InstrRef::ValueConst { dst, value } if dst == r && value.is_nil() => {
                        return true;
                    }
                    InstrRef::Convert {
                        dst,
                        op: ConvOp::IntToFloat,
                        ..
                    } if dst == r => return true,
                    // FloatToInt produces an int — safe, no action needed
                    InstrRef::LoadLocal { dst, slot } if dst == r && seen_slots.insert(slot) => {
                        for other in self.nodes().map(|n| n.instr()) {
                            if let InstrRef::StoreLocal { slot: s, src } = other {
                                if s == slot {
                                    regs_to_check.push(src);
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        false
    }
}
