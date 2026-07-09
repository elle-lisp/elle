//! Shared lowering state threaded through the per-instruction and
//! per-terminator emitters.
//!
//! `lower_to_module` builds the MLIR blocks and argument bindings, then walks
//! the LIR driving [`instr::lower_instr`](super::instr::lower_instr) and
//! [`term::lower_terminator`](super::term::lower_terminator) for each block.
//! Both need the same bundle of SSA/type maps plus the interned MLIR type
//! handles; `LowerCtx` carries them so the emitters take one context argument
//! instead of a dozen positional parameters.
//!
//! Splitting the two match statements out does not change the emitted IR: the
//! ops are appended to the same `block` in the same order as the original
//! single-function form.

use super::*;

/// Mutable lowering state shared across instruction and terminator emission.
///
/// Lifetimes: `'c` is the MLIR context; `'a` is the borrow of the `blocks`
/// vector that the SSA `Value`s (and block references) point into.
pub(super) struct LowerCtx<'c, 'a> {
    pub(super) context: &'c Context,
    pub(super) location: Location<'c>,
    pub(super) i64_type: Type<'c>,
    pub(super) f64_type: Type<'c>,
    /// Env-layout argument count (`captures + params`); bounds `LoadCapture`.
    pub(super) total_args: usize,

    /// SSA register map: LIR `Reg` → MLIR value (within-block SSA values).
    pub(super) regs: HashMap<Reg, Value<'c, 'a>>,
    /// Scalar type per register (`Int`/`Float`/`Bool`).
    pub(super) types: HashMap<Reg, ScalarType>,
    /// Entry-block argument values keyed by env index. Held separately from
    /// `regs` so `LoadCapture` lookups are never clobbered by dst writes.
    pub(super) env_vals: HashMap<EnvIndex, (Value<'c, 'a>, ScalarType)>,
    /// Inferred scalar type per local memref slot.
    pub(super) slot_types: HashMap<SlotId, ScalarType>,
    /// `memref.alloca` pointer value per local slot.
    pub(super) local_slots: HashMap<SlotId, Value<'c, 'a>>,
    /// Return type accumulated from `Return` terminators.
    pub(super) return_type: Option<ScalarType>,
}
