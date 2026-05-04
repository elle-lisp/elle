//! Lambda lowering: closure construction and body compilation

use super::*;
use crate::hir::{CaptureInfo, ParamBound};
use crate::value::Arity;

impl<'a> Lowerer<'a> {
    /// Lower a lambda expression (creates closure with captures)
    #[allow(clippy::too_many_arguments)]
    pub(super) fn lower_lambda_expr(
        &mut self,
        params: &[Binding],
        num_required: usize,
        rest_param: Option<&Binding>,
        vararg_kind: &crate::hir::VarargKind,
        captures: &[CaptureInfo],
        body: &Hir,
        num_locals: u16,
        inferred_signal: &crate::signals::Signal,
        param_bounds: &[ParamBound],
        doc: Option<crate::value::Value>,
        syntax: Option<std::rc::Rc<crate::syntax::Syntax>>,
        assert_numeric: bool,
    ) -> Result<Reg, String> {
        // Collect capture registers
        let mut capture_regs = Vec::new();
        for cap in captures {
            use crate::hir::CaptureKind;

            let reg = self.fresh_reg();

            // Check if this binding needs a cell (captured locals, mutated params)
            // We need to preserve the cell when capturing so mutations are shared
            let binding_needs_capture = self.arena.get(cap.binding).needs_capture();

            match cap.kind {
                CaptureKind::Local => {
                    // Load from parent's local/parameter slot
                    // Use binding_to_slot to find where this binding is in the current context
                    if let Some(&slot) = self.binding_to_slot.get(&cap.binding) {
                        // Check if this is an upvalue or a local in the current context
                        let is_upvalue = self.upvalue_bindings.contains(&cap.binding);
                        if self.in_lambda && is_upvalue {
                            // In a lambda, captures and params are accessed via LoadCapture
                            // Use LoadCaptureRaw for bindings that need cells to preserve the cell
                            if binding_needs_capture {
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
                        if binding_needs_capture {
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
            }
        }

        // Reserve a slot in the module's closure list BEFORE lowering
        // the body. This gives pre-order numbering: parent IDs are lower
        // than children's. Matches collect_nested_functions traversal order.
        let closure_id = ClosureId(self.closures.len() as u32);
        self.closures.push(LirFunction::new(Arity::Exact(0))); // placeholder

        // Lower the lambda body — children get higher IDs
        let mut nested_lir = self.lower_lambda_body(
            params,
            num_required,
            rest_param,
            vararg_kind,
            captures,
            body,
            num_locals,
            *inferred_signal,
            param_bounds,
            doc,
            syntax,
        )?;
        nested_lir.closure_id = Some(closure_id);

        // Check numeric! assertion after lowering
        if assert_numeric && !nested_lir.is_gpu_eligible() {
            return Err("numeric! assertion failed: function is not GPU-eligible".to_string());
        }

        // Fill the reserved slot
        self.closures[closure_id.0 as usize] = nested_lir;

        // Create closure referencing it by ID
        let dst = self.fresh_reg();
        self.emit(LirInstr::MakeClosure {
            dst,
            closure_id,
            captures: capture_regs,
        });
        Ok(dst)
    }

    /// Lower a lambda body to a separate LirFunction
    #[allow(clippy::too_many_arguments)]
    fn lower_lambda_body(
        &mut self,
        params: &[Binding],
        num_required: usize,
        rest_param: Option<&Binding>,
        vararg_kind: &crate::hir::VarargKind,
        captures: &[CaptureInfo],
        body: &Hir,
        _num_locals: u16,
        inferred_signal: crate::signals::Signal,
        param_bounds: &[ParamBound],
        doc: Option<crate::value::Value>,
        syntax: Option<std::rc::Rc<crate::syntax::Syntax>>,
    ) -> Result<LirFunction, String> {
        // Compute arity
        let arity = Arity::for_lambda(rest_param.is_some(), num_required, params.len());

        // Save state
        let saved_func = std::mem::replace(&mut self.current_func, LirFunction::new(arity));
        let saved_block = std::mem::replace(&mut self.current_block, BasicBlock::new(Label(0)));
        let saved_reg = self.next_reg;
        let saved_label = self.next_label;
        let saved_bindings = std::mem::take(&mut self.binding_to_slot);
        let saved_in_lambda = self.in_lambda;
        let saved_num_captures = self.num_captures;
        let saved_num_local_params = self.num_local_params;
        let saved_upvalue_bindings = std::mem::take(&mut self.upvalue_bindings);
        let saved_discard_slot = self.discard_slot;
        let saved_pending_region_exits = self.pending_region_exits;
        let saved_region_depth = self.region_depth;
        let saved_region_refcounted_stack = std::mem::take(&mut self.region_refcounted_stack);
        let saved_flip_depth = self.flip_depth;
        // Save function context. It's set by the caller (lower_letrec,
        // lower_define) before lower_expr so escape analysis can detect
        // self-tail-calls. We save it here and restore it for the
        // post-lowering escape analysis.
        let saved_function_binding = self.current_function_binding.take();
        let saved_function_params = self.current_function_params.take();

        self.next_reg = 0;
        self.next_label = 1;
        // num_locals starts at 0; non-LBox params and let-bound vars
        // will increment it as they're allocated.
        // LBox params go into the env (not counted in num_locals for stack frame).
        self.current_func.num_locals = 0;
        self.current_func.num_captures = captures.len() as u16;
        self.in_lambda = true;
        self.num_captures = captures.len() as u16;
        self.num_local_params = 0;
        self.discard_slot = None;
        self.pending_region_exits = 0;
        self.region_depth = 0;
        self.flip_depth = 0;
        self.current_func.doc = doc;
        self.current_func.syntax = syntax;
        self.current_func.vararg_kind = vararg_kind.clone();
        self.current_func.num_params = params.len();

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

        // Build capture_params_mask and bind parameters.
        // LBox params → upvalues in the env (LoadCapture/StoreCapture).
        // Non-LBox params → locals (LoadLocal/StoreLocal), copied from env at entry.
        let mut capture_params_mask: u64 = 0;
        for (i, param) in params.iter().enumerate() {
            let needs_capture = self.arena.get(*param).needs_capture();

            if needs_capture {
                if i < 64 {
                    capture_params_mask |= 1 << i;
                }
                // LBox param: lives in env as upvalue
                let upvalue_idx = self.num_captures + i as u16;
                self.binding_to_slot.insert(*param, upvalue_idx);
                self.upvalue_bindings.insert(*param);
            } else {
                // Non-LBox param: allocate a local slot.
                // We'll copy from env into this local at function entry.
                let slot = self.current_func.num_locals;
                self.current_func.num_locals += 1;
                self.num_local_params += 1;
                self.binding_to_slot.insert(*param, slot);
                // NOT added to upvalue_bindings → uses LoadLocal/StoreLocal
            }
        }
        self.current_func.capture_params_mask = capture_params_mask;

        // Copy non-LBox params from env into their local slots.
        // The VM/host populates the env as [captures..., params...].
        // Non-LBox params are at env index (num_captures + i).
        for (i, param) in params.iter().enumerate() {
            let needs_capture = self.arena.get(*param).needs_capture();
            if !needs_capture {
                let env_idx = self.num_captures + i as u16;
                let slot = *self.binding_to_slot.get(param).unwrap();
                let tmp = self.fresh_reg();
                self.emit(LirInstr::LoadCaptureRaw {
                    dst: tmp,
                    index: env_idx,
                });
                self.emit(LirInstr::StoreLocal { slot, src: tmp });
            }
        }

        self.current_func.num_local_params = self.num_local_params as usize;

        // Restore function context for body lowering — needed by
        // emit_drop_dead_params to detect self-tail-calls.
        self.current_function_binding = saved_function_binding;
        self.current_function_params = saved_function_params.clone();

        // Emit signal bound checks for each bounded parameter
        for pb in param_bounds {
            if let Some(&slot) = self.binding_to_slot.get(&pb.binding) {
                let src = self.fresh_reg();
                let is_upvalue = self.upvalue_bindings.contains(&pb.binding);
                if is_upvalue {
                    self.emit(LirInstr::LoadCapture {
                        dst: src,
                        index: slot,
                    });
                } else {
                    self.emit(LirInstr::LoadLocal { dst: src, slot });
                }
                self.emit(LirInstr::CheckSignalBound {
                    src,
                    allowed_bits: pb.signal.bits,
                });
            }
        }

        // Lower body
        let result_reg = self.lower_expr(body)?;
        self.terminate(Terminator::Return(result_reg));
        self.finish_block();

        self.current_func.entry = Label(0);
        self.current_func.num_regs = self.next_reg;
        // Propagate inferred signal to LIR function
        self.current_func.signal = inferred_signal;

        // Compute escape analysis flags for fiber shared-alloc decisions.
        // current_function_binding/params are already set (restored before
        // body lowering above), so body_escapes_heap_values can detect
        // self-tail-calls with per-parameter analysis.
        self.current_func.result_is_immediate = self.result_is_safe(body, &[]);
        self.current_func.has_outward_heap_set =
            self.body_contains_dangerous_outward_set(body, &[]);
        self.current_func.rotation_safe = !self.body_escapes_heap_values(body);
        // Clear function context — will be restored to parent's state below.
        self.current_function_binding = None;
        self.current_function_params = None;

        let func = std::mem::replace(&mut self.current_func, saved_func);

        // Restore state
        self.current_block = saved_block;
        self.next_reg = saved_reg;
        self.next_label = saved_label;
        self.binding_to_slot = saved_bindings;
        self.in_lambda = saved_in_lambda;
        self.num_captures = saved_num_captures;
        self.num_local_params = saved_num_local_params;
        self.upvalue_bindings = saved_upvalue_bindings;
        self.discard_slot = saved_discard_slot;
        self.pending_region_exits = saved_pending_region_exits;
        self.region_depth = saved_region_depth;
        self.region_refcounted_stack = saved_region_refcounted_stack;
        self.flip_depth = saved_flip_depth;

        Ok(func)
    }
}
