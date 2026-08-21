//! Variable-reference lowering — the `HirKind::Var` arm of `lower_expr`.
//!
//! Split out because resolving a binding (immutable-value inline, self-closure
//! `LoadSelf`, upvalue vs. local slot, capture-cell unwrap) is its own concern
//! with several independent fast paths, distinct from the dispatch shell.

use super::*;

impl<'a> Lowerer<'a> {
    pub(super) fn lower_var(&mut self, binding: &Binding, span: &Span) -> Result<Reg, String> {
        // Check immutable_values first — primitive bindings and immutable
        // globals with literal values are compiled to LoadConst without
        // needing a slot allocation.
        if let Some(&literal_value) = self.immutable_values.get(binding) {
            return self.emit_value_const(literal_value);
        }

        // A reference to the enclosing lambda's own self-recursive binding resolves
        // to the executing closure (`LoadSelf`), not a load of its forward cell — in
        // EVERY position. In value position the closure is materialized and used; in
        // call position the callee IS the executing closure, so the call re-enters the
        // same code+env with new args (self-call re-dispatch). Both are RC-identical to
        // the forward-cell load without naming the cell. `current_self_binding` is set
        // only inside that lambda's body, and only for a same-binding self-edge (a
        // sibling/foreign capture stays `Local`/`Capture` and keeps its cell for the
        // closure-cycle merge), so this fires for exactly the self-references.
        if self.current_self_binding == Some(*binding) {
            let dst = self.fresh_reg();
            self.emit(LirInstr::LoadSelf { dst });
            return Ok(dst);
        }

        if let Some(&slot) = self.binding_to_slot.get(binding) {
            // Check if this binding needs cell unwrapping
            let needs_capture = self.arena.get(*binding).needs_capture();

            // Check if this is an upvalue (capture or parameter) or a local
            let is_upvalue = self.upvalue_bindings.contains(binding);

            let dst = self.fresh_reg();
            if self.in_lambda && is_upvalue {
                if needs_capture {
                    self.emit(LirInstr::LoadCapture { dst, index: slot });
                } else {
                    self.emit(LirInstr::LoadCaptureRaw { dst, index: slot });
                }
                Ok(dst)
            } else {
                // A plain stack slot: every non-upvalue binding — outside
                // lambdas, and in-lambda for a compiled-cell letrec binding
                // (letrec_compiled_cell) whose slot holds the MakeCaptureCell.
                self.emit(LirInstr::LoadLocal { dst, slot });

                if needs_capture {
                    // Unwrap the cell to get the actual value
                    // Only needed for locals, not captures (LoadCapture auto-unwraps)
                    let value_reg = self.fresh_reg();
                    self.emit(LirInstr::LoadCaptureCell {
                        dst: value_reg,
                        cell: dst,
                    });
                    Ok(value_reg)
                } else {
                    Ok(dst)
                }
            }
        } else {
            // Binding not found in immutable_values or binding_to_slot.
            // This happens when the analyzer's resolve_primitive fallback
            // creates a dangling binding for an undefined variable.
            let sym_id = self.arena.get(*binding).name;
            let name = self
                .symbol_names
                .get(&sym_id.0)
                .cloned()
                .unwrap_or_else(|| format!("symbol #{}", sym_id.0));
            Err(format!("{}: undefined variable: {}", span, name))
        }
    }
}
