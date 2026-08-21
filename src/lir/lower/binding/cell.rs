//! Cell/destructure lowering: the transparent MakeCell/DerefCell/SetCell
//! delegations and the Destructure-node entry point plus its `lower_bind_value`
//! helper. Kept together because they all sit on the functionalize/lowerer
//! double-handling contract for capture cells (see `lower_make_cell`).

use super::*;

impl<'a> Lowerer<'a> {
    /// Lower a Destructure node: evaluate the value, then destructure into bindings.
    /// Returns a nil register (destructuring is a statement, not an expression).
    /// `strict`: if true, missing/wrong-type values signal error; if false, produce nil.
    pub(in crate::lir::lower) fn lower_destructure_expr(
        &mut self,
        pattern: &HirPattern,
        value: &Hir,
        strict: bool,
        _span: &Span,
    ) -> Result<Reg, String> {
        let value_reg = self.lower_expr(value)?;
        self.lower_destructure(pattern, value_reg, strict)?;
        // Destructure produces nil as its expression value
        self.emit_const(LirConst::Nil)
    }

    /// Lower MakeCell — currently transparent (just lowers the inner value).
    ///
    /// **Double-handling contract:** Both functionalize AND the lowerer handle
    /// cells. Functionalize inserts explicit MakeCell/DerefCell/SetCell nodes;
    /// the lowerer's lower_let/lower_letrec/lower_define independently wrap
    /// needs_capture bindings in cells. The transparent delegation here works
    /// because both sides agree on which bindings need cells (via
    /// `needs_capture()`). Phase 3 will remove the lowerer's implicit cell
    /// creation and make these methods emit real cell instructions.
    pub(in crate::lir::lower) fn lower_make_cell(&mut self, value: &Hir) -> Result<Reg, String> {
        self.lower_expr(value)
    }

    /// Lower DerefCell — currently transparent (just lowers the inner cell expr).
    ///
    /// See `lower_make_cell` for the double-handling contract. The lowerer's
    /// `lower_var` already unwraps cells for needs_capture bindings, so
    /// DerefCell's child (a Var) produces the unwrapped value directly.
    pub(in crate::lir::lower) fn lower_deref_cell(&mut self, cell: &Hir) -> Result<Reg, String> {
        self.lower_expr(cell)
    }

    /// Lower SetCell — delegates to lower_assign since the lowerer already
    /// handles cell stores. The cell child must be a Var.
    ///
    /// See `lower_make_cell` for the double-handling contract. The lowerer's
    /// `lower_assign` already stores through cells for needs_capture bindings.
    pub(in crate::lir::lower) fn lower_set_cell(
        &mut self,
        cell: &Hir,
        value: &Hir,
    ) -> Result<Reg, String> {
        if let HirKind::Var(binding) = &cell.kind {
            self.lower_assign(binding, value)
        } else {
            Err("SetCell: cell must be a Var".to_string())
        }
    }

    /// Store a value into a binding, consuming it from the stack.
    /// Used by lower_destructure.
    pub(super) fn lower_bind_value(
        &mut self,
        binding: Binding,
        value_reg: Reg,
    ) -> Result<Reg, String> {
        // Evict stale constant — this binding is being (re-)assigned
        // (e.g., file-scope destructure reusing an earlier binding).
        self.immutable_values.remove(&binding);
        // Allocate slot if not already done (Begin pre-pass may have done it)
        let slot = if let Some(&existing_slot) = self.binding_to_slot.get(&binding) {
            existing_slot
        } else {
            self.allocate_slot(binding)
        };

        let needs_capture = self.arena.get(binding).needs_capture();

        if self.in_lambda && needs_capture {
            self.upvalue_bindings.insert(binding);
            self.emit(LirInstr::StoreCapture {
                index: slot,
                src: value_reg,
            });
        } else if self.in_lambda {
            self.emit_binding_store(slot, value_reg);
        } else if needs_capture {
            // cell was already created in Begin pre-pass
            let cell_reg = self.fresh_reg();
            self.emit(LirInstr::LoadLocal {
                dst: cell_reg,
                slot,
            });
            self.emit(LirInstr::StoreCaptureCell {
                cell: cell_reg,
                value: value_reg,
            });
        } else {
            self.emit_binding_store(slot, value_reg);
        }
        Ok(value_reg)
    }
}
