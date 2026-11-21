// audited: 2026-11-21
//! Variable-reference lowering: the `HirKind::Var` arm of `lower_expr`, and the one reading of where a binding's value lives.
//!
//! src/lir/lower/overview.md
//!
//! Split out because resolving a binding (immutable-value inline, self-closure
//! `LoadSelf`, upvalue vs. local slot, capture-cell unwrap) is its own concern
//! with several independent fast paths, distinct from the dispatch shell.

use super::*;

/// Where a read of a binding finds its value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::lir::lower) enum VarRead {
    /// An immutable binding with a compile-time value, loaded as `ValueConst`.
    Constant(Value),
    /// The enclosing lambda's own self-recursive binding: the executing
    /// closure, `LoadSelf`.
    SelfRef,
    /// A slot.
    Slot(SlotRead),
    /// No constant and no slot: the analyzer's fallback for an undefined
    /// variable.
    Undefined,
}

/// The slot a binding's value lives in, and whether a capture cell wraps it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::lir::lower) enum SlotRead {
    /// An upvalue of the lambda being lowered: a capture, a parameter or an
    /// env-celled local, at its env index.
    Capture { index: u16, cell: bool },
    /// A stack slot: every binding outside a lambda, and inside one a binding
    /// that is no upvalue, a compiled-cell letrec binding among them
    /// (`compiled_forward_cell`), whose slot holds the `MakeCaptureCell`.
    Local { slot: u16, cell: bool },
}

impl<'a> Lowerer<'a> {
    /// Where a read of `binding` finds its value. A constant comes first,
    /// because a primitive or an immutable global with a literal value needs
    /// no slot. The self-reference comes next, in EVERY position: in value
    /// position the closure is materialized and used, and in call position the
    /// call re-enters the same code and env with new arguments (self-call
    /// re-dispatch). Both are RC-identical to the forward-cell load without
    /// naming the cell. `current_self_binding` is set only inside that lambda's
    /// body, and only for a same-binding self-edge (a sibling or foreign
    /// capture keeps its cell for the closure-cycle merge).
    pub(in crate::lir::lower) fn var_read(&self, binding: &Binding) -> VarRead {
        if let Some(&value) = self.immutable_values.get(binding) {
            return VarRead::Constant(value);
        }
        if self.current_self_binding == Some(*binding) {
            return VarRead::SelfRef;
        }
        match self.slot_read(binding) {
            Some(read) => VarRead::Slot(read),
            None => VarRead::Undefined,
        }
    }

    /// The slot `binding` holds its value in, whether or not a constant or
    /// the self-reference stands in for the read.
    pub(in crate::lir::lower) fn slot_read(&self, binding: &Binding) -> Option<SlotRead> {
        let &slot = self.binding_to_slot.get(binding)?;
        let cell = self.arena.get(*binding).needs_capture();
        Some(
            if self.in_lambda && self.upvalue_bindings.contains(binding) {
                SlotRead::Capture { index: slot, cell }
            } else {
                SlotRead::Local { slot, cell }
            },
        )
    }

    pub(super) fn lower_var(&mut self, binding: &Binding, span: &Span) -> Result<Reg, String> {
        match self.var_read(binding) {
            VarRead::Constant(value) => self.emit_value_const(value),
            VarRead::SelfRef => {
                let dst = self.fresh_reg();
                self.emit(InstrRef::LoadSelf { dst });
                Ok(dst)
            }
            // `LoadCapture` unwraps a cell itself.
            VarRead::Slot(SlotRead::Capture { index, cell }) => {
                let dst = self.fresh_reg();
                if cell {
                    self.emit(InstrRef::LoadCapture { dst, index });
                } else {
                    self.emit(InstrRef::LoadCaptureRaw { dst, index });
                }
                Ok(dst)
            }
            VarRead::Slot(SlotRead::Local { slot, cell }) => {
                let dst = self.fresh_reg();
                self.emit(InstrRef::LoadLocal { dst, slot });
                if !cell {
                    return Ok(dst);
                }
                let value_reg = self.fresh_reg();
                self.emit(InstrRef::LoadCaptureCell {
                    dst: value_reg,
                    cell: dst,
                });
                Ok(value_reg)
            }
            VarRead::Undefined => {
                let sym_id = self.arena.get(*binding).name;
                let name = self
                    .symbols
                    .and_then(|s| s.name(sym_id))
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| format!("symbol #{:#x}", sym_id.0));
                Err(format!("{}: undefined variable: {}", span, name))
            }
        }
    }
}
