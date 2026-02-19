//! Lambda lowering: closure construction and body compilation

use super::*;
use crate::hir::CaptureInfo;

impl Lowerer {
    /// Lower a lambda expression (creates closure with captures)
    pub(super) fn lower_lambda_expr(
        &mut self,
        params: &[BindingId],
        captures: &[CaptureInfo],
        body: &Hir,
        num_locals: u16,
        inferred_effect: &crate::effects::Effect,
    ) -> Result<Reg, String> {
        // Collect capture registers
        let mut capture_regs = Vec::new();
        for cap in captures {
            use crate::hir::CaptureKind;

            let reg = self.fresh_reg();

            // Check if this binding needs a cell (captured locals, mutated params)
            // We need to preserve the cell when capturing so mutations are shared
            let binding_needs_cell = self
                .bindings
                .get(&cap.binding)
                .map(|info| info.needs_cell())
                .unwrap_or(false);

            match cap.kind {
                CaptureKind::Local { index: _ } => {
                    // Load from parent's local/parameter slot
                    // Use binding_to_slot to find where this binding is in the current context
                    if let Some(&slot) = self.binding_to_slot.get(&cap.binding) {
                        // Check if this is an upvalue or a local in the current context
                        let is_upvalue = self.upvalue_bindings.contains(&cap.binding);
                        if self.in_lambda && is_upvalue {
                            // In a lambda, captures and params are accessed via LoadCapture
                            // Use LoadCaptureRaw for bindings that need cells to preserve the cell
                            if binding_needs_cell {
                                self.emit(LirInstr::LoadCaptureRaw {
                                    dst: reg,
                                    index: slot,
                                });
                            } else {
                                self.emit(LirInstr::LoadCapture {
                                    dst: reg,
                                    index: slot,
                                });
                            }
                        } else {
                            // Local variables (including those defined inside lambda) use LoadLocal
                            self.emit(LirInstr::LoadLocal { dst: reg, slot });
                        }
                    } else {
                        // Binding not found in current context - this shouldn't happen
                        return Err(format!(
                            "Capture binding {:?} not found in current context",
                            cap.binding
                        ));
                    }
                    capture_regs.push(reg);
                }
                CaptureKind::Capture { index } => {
                    // Load from parent's capture (transitive capture)
                    // The index refers to the parent's capture array
                    if self.in_lambda {
                        // We're in a nested lambda - load from parent's captures
                        // Use LoadCaptureRaw for bindings that need cells to preserve the cell
                        if binding_needs_cell {
                            self.emit(LirInstr::LoadCaptureRaw { dst: reg, index });
                        } else {
                            self.emit(LirInstr::LoadCapture { dst: reg, index });
                        }
                    } else {
                        // We're in the main function - this shouldn't happen
                        // (main function doesn't have captures to forward)
                        self.emit(LirInstr::LoadLocal {
                            dst: reg,
                            slot: index,
                        });
                    }
                    capture_regs.push(reg);
                }
                CaptureKind::Global { sym } => {
                    // Load global directly
                    self.emit(LirInstr::LoadGlobal { dst: reg, sym });
                    capture_regs.push(reg);
                }
            }
        }

        // Lower the lambda body to a separate LirFunction
        let nested_lir =
            self.lower_lambda_body(params, captures, body, num_locals, inferred_effect.clone())?;

        // Create closure with the nested function
        let dst = self.fresh_reg();
        self.emit(LirInstr::MakeClosure {
            dst,
            func: Box::new(nested_lir),
            captures: capture_regs,
        });
        Ok(dst)
    }

    /// Lower a lambda body to a separate LirFunction
    fn lower_lambda_body(
        &mut self,
        params: &[BindingId],
        captures: &[CaptureInfo],
        body: &Hir,
        _num_locals: u16,
        inferred_effect: crate::effects::Effect,
    ) -> Result<LirFunction, String> {
        // Save state
        let saved_func = std::mem::replace(
            &mut self.current_func,
            LirFunction::new(params.len() as u16),
        );
        let saved_block = std::mem::replace(&mut self.current_block, BasicBlock::new(Label(0)));
        let saved_reg = self.next_reg;
        let saved_label = self.next_label;
        let saved_bindings = std::mem::take(&mut self.binding_to_slot);
        let saved_in_lambda = self.in_lambda;
        let saved_num_captures = self.num_captures;
        let saved_upvalue_bindings = std::mem::take(&mut self.upvalue_bindings);

        self.next_reg = 0;
        self.next_label = 1;
        // num_locals should be params + locals (NOT including captures)
        // This matches the HIR definition and is what the VM expects
        // The environment layout is: [captures..., parameters..., locally_defined_cells...]
        // But num_locals only counts the parameters and locally-defined variables
        self.current_func.num_locals = params.len() as u16;
        self.current_func.num_captures = captures.len() as u16;
        self.in_lambda = true;
        self.num_captures = captures.len() as u16;

        // In a closure, the environment is laid out as:
        // [captured_vars..., parameters..., locally_defined_cells...]
        // So:
        // - Captured variables are at indices [0, num_captures)
        // - Parameters are at indices [num_captures, num_captures + num_params)

        // Bind captured variables to upvalue indices
        for (i, cap) in captures.iter().enumerate() {
            self.binding_to_slot.insert(cap.binding, i as u16);
            self.upvalue_bindings.insert(cap.binding);
        }

        // Build cell_params_mask and bind parameters to upvalue indices
        // Parameters that need cells will be wrapped by the VM when the closure is called
        let mut cell_params_mask: u64 = 0;
        for (i, param) in params.iter().enumerate() {
            let upvalue_idx = self.num_captures + i as u16;

            let needs_cell = self
                .bindings
                .get(param)
                .map(|info| info.needs_cell())
                .unwrap_or(false);

            if needs_cell && i < 64 {
                // Set the bit for this parameter
                cell_params_mask |= 1 << i;
            }

            // All parameters are upvalues - the VM will wrap them in cells if needed
            self.binding_to_slot.insert(*param, upvalue_idx);
            self.upvalue_bindings.insert(*param);
        }
        self.current_func.cell_params_mask = cell_params_mask;

        // Lower body
        let result_reg = self.lower_expr(body)?;
        self.terminate(Terminator::Return(result_reg));
        self.finish_block();

        self.current_func.entry = Label(0);
        self.current_func.num_regs = self.next_reg;
        // Propagate inferred effect to LIR function
        self.current_func.effect = inferred_effect.clone();

        let func = std::mem::replace(&mut self.current_func, saved_func);

        // Restore state
        self.current_block = saved_block;
        self.next_reg = saved_reg;
        self.next_label = saved_label;
        self.binding_to_slot = saved_bindings;
        self.in_lambda = saved_in_lambda;
        self.num_captures = saved_num_captures;
        self.upvalue_bindings = saved_upvalue_bindings;

        Ok(func)
    }
}
