//! HIR to LIR lowering

use super::types::*;
use crate::hir::{BindingId, BindingInfo, BindingKind, Hir, HirKind, HirPattern, PatternLiteral};
use crate::value::Value;
use std::collections::HashMap;

/// Lowers HIR to LIR
pub struct Lowerer {
    /// Current function being built
    current_func: LirFunction,
    /// Current block being built
    current_block: BasicBlock,
    /// Next register ID
    next_reg: u32,
    /// Next label ID
    next_label: u32,
    /// Mapping from BindingId to local slot
    binding_to_slot: HashMap<BindingId, u16>,
    /// Binding metadata from analysis
    bindings: HashMap<BindingId, BindingInfo>,
    /// Whether we're currently lowering a lambda (closure)
    in_lambda: bool,
    /// Number of captured variables (for lambda context)
    num_captures: u16,
    /// Set of bindings that are upvalues (captures/parameters in lambda)
    /// These use LoadCapture/StoreCapture, not LoadLocal/StoreLocal
    upvalue_bindings: std::collections::HashSet<BindingId>,
}

impl Lowerer {
    pub fn new() -> Self {
        Lowerer {
            current_func: LirFunction::new(0),
            current_block: BasicBlock::new(Label(0)),
            next_reg: 0,
            next_label: 1, // 0 is entry
            binding_to_slot: HashMap::new(),
            bindings: HashMap::new(),
            in_lambda: false,
            num_captures: 0,
            upvalue_bindings: std::collections::HashSet::new(),
        }
    }

    /// Set binding info from analysis
    pub fn with_bindings(mut self, bindings: HashMap<BindingId, BindingInfo>) -> Self {
        self.bindings = bindings;
        self
    }

    /// Lower a HIR expression to LIR
    pub fn lower(&mut self, hir: &Hir) -> Result<LirFunction, String> {
        self.current_func = LirFunction::new(0);
        self.current_block = BasicBlock::new(Label(0));
        self.next_reg = 0;
        self.next_label = 1;
        self.binding_to_slot.clear();

        let result_reg = self.lower_expr(hir)?;
        self.terminate(Terminator::Return(result_reg));
        self.finish_block();

        self.current_func.entry = Label(0);
        self.current_func.num_regs = self.next_reg;

        Ok(std::mem::replace(
            &mut self.current_func,
            LirFunction::new(0),
        ))
    }

    /// Lower a lambda to a separate LirFunction
    pub fn lower_lambda(
        &mut self,
        params: &[BindingId],
        captures: &[crate::hir::CaptureInfo],
        body: &Hir,
        _num_locals: u16,
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

    fn lower_expr(&mut self, hir: &Hir) -> Result<Reg, String> {
        match &hir.kind {
            HirKind::Nil => self.emit_const(LirConst::Nil),
            HirKind::EmptyList => self.emit_const(LirConst::EmptyList),
            HirKind::Bool(b) => self.emit_const(LirConst::Bool(*b)),
            HirKind::Int(n) => self.emit_const(LirConst::Int(*n)),
            HirKind::Float(f) => self.emit_const(LirConst::Float(*f)),
            HirKind::String(s) => self.emit_const(LirConst::String(s.clone())),
            HirKind::Keyword(sym) => self.emit_const(LirConst::Keyword(*sym)),

            HirKind::Var(binding_id) => {
                if let Some(&slot) = self.binding_to_slot.get(binding_id) {
                    // Check if this binding needs cell unwrapping
                    let needs_cell = self
                        .bindings
                        .get(binding_id)
                        .map(|info| info.needs_cell())
                        .unwrap_or(false);

                    // Check if this is an upvalue (capture or parameter) or a local
                    let is_upvalue = self.upvalue_bindings.contains(binding_id);

                    let dst = self.fresh_reg();
                    if self.in_lambda && is_upvalue {
                        // In a lambda, captures, parameters, and locally-defined variables are accessed via LoadCapture
                        // Note: LoadCapture (which emits LoadUpvalue) auto-unwraps LocalCell,
                        // so we don't need to emit LoadCell for captured variables
                        self.emit(LirInstr::LoadCapture { dst, index: slot });
                        Ok(dst)
                    } else {
                        // Outside lambdas, local variables use LoadLocal
                        self.emit(LirInstr::LoadLocal { dst, slot });

                        if needs_cell {
                            // Unwrap the cell to get the actual value
                            // Only needed for locals, not captures (LoadCapture auto-unwraps)
                            let value_reg = self.fresh_reg();
                            self.emit(LirInstr::LoadCell {
                                dst: value_reg,
                                cell: dst,
                            });
                            Ok(value_reg)
                        } else {
                            Ok(dst)
                        }
                    }
                } else if let Some(info) = self.bindings.get(binding_id) {
                    match info.kind {
                        BindingKind::Global => {
                            let sym = info.name;
                            let dst = self.fresh_reg();
                            self.emit(LirInstr::LoadGlobal { dst, sym });
                            Ok(dst)
                        }
                        _ => Err(format!("Unbound variable: {:?}", binding_id)),
                    }
                } else {
                    Err(format!("Unknown binding: {:?}", binding_id))
                }
            }

            HirKind::Let { bindings, body } => {
                // Allocate slots and lower initializers
                for (binding_id, init) in bindings {
                    let init_reg = self.lower_expr(init)?;
                    let slot = self.allocate_slot(*binding_id);

                    // Check if this binding needs to be wrapped in a cell
                    let needs_cell = self
                        .bindings
                        .get(binding_id)
                        .map(|info| info.needs_cell())
                        .unwrap_or(false);

                    if needs_cell {
                        // Wrap the value in a cell
                        let cell_reg = self.fresh_reg();
                        self.emit(LirInstr::MakeCell {
                            dst: cell_reg,
                            value: init_reg,
                        });
                        self.emit(LirInstr::StoreLocal {
                            slot,
                            src: cell_reg,
                        });
                    } else {
                        self.emit(LirInstr::StoreLocal {
                            slot,
                            src: init_reg,
                        });
                    }
                }
                self.lower_expr(body)
            }

            HirKind::Letrec { bindings, body } => {
                // First allocate all slots with nil (or cells containing nil)
                for (binding_id, _) in bindings {
                    let nil_reg = self.emit_const(LirConst::Nil)?;
                    let slot = self.allocate_slot(*binding_id);

                    // Check if this binding needs to be wrapped in a cell
                    let needs_cell = self
                        .bindings
                        .get(binding_id)
                        .map(|info| info.needs_cell())
                        .unwrap_or(false);

                    if needs_cell {
                        // Create a cell containing nil initially
                        let cell_reg = self.fresh_reg();
                        self.emit(LirInstr::MakeCell {
                            dst: cell_reg,
                            value: nil_reg,
                        });
                        self.emit(LirInstr::StoreLocal {
                            slot,
                            src: cell_reg,
                        });
                    } else {
                        self.emit(LirInstr::StoreLocal { slot, src: nil_reg });
                    }
                }
                // Then initialize
                for (binding_id, init) in bindings {
                    let init_reg = self.lower_expr(init)?;
                    let slot = self.binding_to_slot[binding_id];

                    // Check if this binding needs cell update
                    let needs_cell = self
                        .bindings
                        .get(binding_id)
                        .map(|info| info.needs_cell())
                        .unwrap_or(false);

                    if needs_cell {
                        // Load the cell and update it
                        let cell_reg = self.fresh_reg();
                        self.emit(LirInstr::LoadLocal {
                            dst: cell_reg,
                            slot,
                        });
                        self.emit(LirInstr::StoreCell {
                            cell: cell_reg,
                            value: init_reg,
                        });
                    } else {
                        self.emit(LirInstr::StoreLocal {
                            slot,
                            src: init_reg,
                        });
                    }
                }
                self.lower_expr(body)
            }

            HirKind::Lambda {
                params,
                captures,
                body,
                num_locals,
            } => {
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
                let nested_lir = self.lower_lambda(params, captures, body, *num_locals)?;

                // Create closure with the nested function
                let dst = self.fresh_reg();
                self.emit(LirInstr::MakeClosure {
                    dst,
                    func: Box::new(nested_lir),
                    captures: capture_regs,
                });
                Ok(dst)
            }

            HirKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                // Lower condition
                let cond_reg = self.lower_expr(cond)?;

                // Allocate a result register for the if expression
                let result_reg = self.fresh_reg();

                // Allocate label IDs for inline jumps
                let else_label_id = self.next_label;
                self.next_label += 1;
                let end_label_id = self.next_label;
                self.next_label += 1;

                // Emit conditional jump to else (will jump if condition is false)
                self.emit(LirInstr::JumpIfFalseInline {
                    cond: cond_reg,
                    label_id: else_label_id,
                });

                // Lower then branch
                let then_reg = self.lower_expr(then_branch)?;
                // Move then result to result register
                self.emit(LirInstr::Move {
                    dst: result_reg,
                    src: then_reg,
                });

                // Emit unconditional jump to end
                self.emit(LirInstr::JumpInline {
                    label_id: end_label_id,
                });

                // Emit else label marker
                self.emit(LirInstr::LabelMarker {
                    label_id: else_label_id,
                });

                // Lower else branch
                let else_reg = self.lower_expr(else_branch)?;
                // Move else result to result register
                self.emit(LirInstr::Move {
                    dst: result_reg,
                    src: else_reg,
                });

                // Emit end label marker
                self.emit(LirInstr::LabelMarker {
                    label_id: end_label_id,
                });

                // Return the result register
                Ok(result_reg)
            }

            HirKind::Begin(exprs) => {
                // Pre-allocate slots for all LocalDefine bindings
                // This enables mutual recursion where lambda A captures variable B
                // before B's LocalDefine has been lowered
                for expr in exprs {
                    if let HirKind::LocalDefine { binding, .. } = &expr.kind {
                        // Allocate slot now so captures can find it
                        if !self.binding_to_slot.contains_key(binding) {
                            let slot = self.allocate_slot(*binding);

                            // Inside lambdas, local variables are part of the closure environment
                            if self.in_lambda {
                                self.upvalue_bindings.insert(*binding);
                            }

                            // Check if this binding needs a cell
                            let needs_cell = self
                                .bindings
                                .get(binding)
                                .map(|info| info.needs_cell())
                                .unwrap_or(false);

                            // Only create cells for top-level locals (outside lambdas)
                            // Inside lambdas, the VM creates cells for locally-defined variables
                            // when building the closure environment
                            if needs_cell && !self.in_lambda {
                                // Create a cell containing nil
                                // This cell will be captured by nested lambdas
                                // and updated when the LocalDefine is lowered
                                let nil_reg = self.emit_const(LirConst::Nil)?;
                                let cell_reg = self.fresh_reg();
                                self.emit(LirInstr::MakeCell {
                                    dst: cell_reg,
                                    value: nil_reg,
                                });
                                self.emit(LirInstr::StoreLocal {
                                    slot,
                                    src: cell_reg,
                                });
                            }
                        }
                    }
                }

                // Now lower all expressions (slots are available for capture lookup)
                // Pop intermediate results to keep the stack clean
                if exprs.is_empty() {
                    return self.emit_const(LirConst::Nil);
                }
                let mut last_reg = self.lower_expr(&exprs[0])?;
                for expr in exprs.iter().skip(1) {
                    // Pop the previous result before evaluating the next expression
                    self.emit(LirInstr::Pop { src: last_reg });
                    last_reg = self.lower_expr(expr)?;
                }
                Ok(last_reg)
            }

            HirKind::Block(exprs) => {
                // Pop intermediate results to keep the stack clean
                if exprs.is_empty() {
                    return self.emit_const(LirConst::Nil);
                }
                let mut last_reg = self.lower_expr(&exprs[0])?;
                for expr in exprs.iter().skip(1) {
                    // Pop the previous result before evaluating the next expression
                    self.emit(LirInstr::Pop { src: last_reg });
                    last_reg = self.lower_expr(expr)?;
                }
                Ok(last_reg)
            }

            HirKind::Call {
                func,
                args,
                is_tail,
            } => {
                // Lower arguments first, then function
                // This ensures the stack is in the right order for the Call instruction
                let mut arg_regs = Vec::new();
                for arg in args {
                    arg_regs.push(self.lower_expr(arg)?);
                }
                let func_reg = self.lower_expr(func)?;

                if *is_tail {
                    self.emit(LirInstr::TailCall {
                        func: func_reg,
                        args: arg_regs,
                    });
                    // After tail call, we need a placeholder reg
                    Ok(self.fresh_reg())
                } else {
                    let dst = self.fresh_reg();
                    self.emit(LirInstr::Call {
                        dst,
                        func: func_reg,
                        args: arg_regs,
                    });
                    Ok(dst)
                }
            }

            HirKind::Set { target, value } => {
                let value_reg = self.lower_expr(value)?;

                // Check if this binding needs cell update
                let needs_cell = self
                    .bindings
                    .get(target)
                    .map(|info| info.needs_cell())
                    .unwrap_or(false);

                // Check if this is an upvalue (capture or parameter) or a local
                let is_upvalue = self.upvalue_bindings.contains(target);

                if let Some(&slot) = self.binding_to_slot.get(target) {
                    if self.in_lambda && is_upvalue {
                        // For captured variables, use StoreCapture which handles cells automatically
                        // StoreUpvalue checks if the upvalue is a cell and updates it
                        self.emit(LirInstr::StoreCapture {
                            index: slot,
                            src: value_reg,
                        });
                    } else if needs_cell {
                        // For local variables that need cells, load the cell and update it
                        let cell_reg = self.fresh_reg();
                        self.emit(LirInstr::LoadLocal {
                            dst: cell_reg,
                            slot,
                        });
                        self.emit(LirInstr::StoreCell {
                            cell: cell_reg,
                            value: value_reg,
                        });
                    } else {
                        // For simple local variables, store directly
                        self.emit(LirInstr::StoreLocal {
                            slot,
                            src: value_reg,
                        });
                    }
                } else if let Some(info) = self.bindings.get(target) {
                    let sym = info.name;
                    match info.kind {
                        BindingKind::Global => {
                            self.emit(LirInstr::StoreGlobal {
                                sym,
                                src: value_reg,
                            });
                        }
                        _ => {
                            return Err(format!("Cannot set unbound variable: {:?}", target));
                        }
                    }
                } else {
                    return Err(format!("Unknown binding: {:?}", target));
                }
                Ok(value_reg)
            }

            HirKind::Define { name, value } => {
                let value_reg = self.lower_expr(value)?;
                self.emit(LirInstr::StoreGlobal {
                    sym: *name,
                    src: value_reg,
                });
                Ok(value_reg)
            }

            HirKind::LocalDefine { binding, value } => {
                // Allocate the slot BEFORE lowering the value so that recursive
                // references can find the binding (like letrec)
                // The slot might already be allocated by the Begin pre-pass
                let slot = if let Some(&existing_slot) = self.binding_to_slot.get(binding) {
                    existing_slot
                } else {
                    self.allocate_slot(*binding)
                };

                // Inside lambdas, local variables are part of the closure environment
                if self.in_lambda {
                    self.upvalue_bindings.insert(*binding);
                }

                // Check if this binding needs to be wrapped in a cell
                let needs_cell = self
                    .bindings
                    .get(binding)
                    .map(|info| info.needs_cell())
                    .unwrap_or(false);

                // Now lower the value (which can reference the binding)
                let value_reg = self.lower_expr(value)?;

                if self.in_lambda {
                    // Inside a lambda, use closure environment via StoreCapture
                    // StoreCapture handles cells automatically
                    self.emit(LirInstr::StoreCapture {
                        index: slot,
                        src: value_reg,
                    });
                } else {
                    // Outside lambdas (at top level), use stack-based locals
                    if needs_cell {
                        // The cell was already created in the Begin pre-pass
                        let cell_reg = self.fresh_reg();
                        self.emit(LirInstr::LoadLocal {
                            dst: cell_reg,
                            slot,
                        });
                        self.emit(LirInstr::StoreCell {
                            cell: cell_reg,
                            value: value_reg,
                        });
                    } else {
                        self.emit(LirInstr::StoreLocal {
                            slot,
                            src: value_reg,
                        });
                    }
                }
                Ok(value_reg)
            }

            HirKind::While { cond, body } => {
                let loop_label_id = self.next_label;
                self.next_label += 1;
                let exit_label_id = self.next_label;
                self.next_label += 1;

                // Emit loop label marker
                self.emit(LirInstr::LabelMarker {
                    label_id: loop_label_id,
                });

                // Evaluate condition
                let cond_reg = self.lower_expr(cond)?;

                // Jump to exit if condition is false
                self.emit(LirInstr::JumpIfFalseInline {
                    cond: cond_reg,
                    label_id: exit_label_id,
                });

                // Evaluate body
                self.lower_expr(body)?;

                // Jump back to loop start
                self.emit(LirInstr::JumpInline {
                    label_id: loop_label_id,
                });

                // Exit label
                self.emit(LirInstr::LabelMarker {
                    label_id: exit_label_id,
                });

                self.emit_const(LirConst::Nil)
            }

            HirKind::For { var, iter, body } => {
                // Allocate separate slots for iterator and loop variable
                let iter_slot = self.current_func.num_locals;
                self.current_func.num_locals += 1;

                let var_slot = self.allocate_slot(*var);

                // Store initial iterator
                let iter_reg = self.lower_expr(iter)?;
                self.emit(LirInstr::StoreLocal {
                    slot: iter_slot,
                    src: iter_reg,
                });

                let loop_label_id = self.next_label;
                self.next_label += 1;
                let exit_label_id = self.next_label;
                self.next_label += 1;

                // Loop start
                self.emit(LirInstr::LabelMarker {
                    label_id: loop_label_id,
                });

                // Load iterator and check if it's a pair
                let current_iter = self.fresh_reg();
                self.emit(LirInstr::LoadLocal {
                    dst: current_iter,
                    slot: iter_slot,
                });
                let is_pair = self.fresh_reg();
                self.emit(LirInstr::IsPair {
                    dst: is_pair,
                    src: current_iter,
                });

                // Exit if not a pair
                self.emit(LirInstr::JumpIfFalseInline {
                    cond: is_pair,
                    label_id: exit_label_id,
                });

                // Extract car and store to VAR slot (not iter slot!)
                let iter_for_car = self.fresh_reg();
                self.emit(LirInstr::LoadLocal {
                    dst: iter_for_car,
                    slot: iter_slot,
                });
                let car_reg = self.fresh_reg();
                self.emit(LirInstr::Car {
                    dst: car_reg,
                    pair: iter_for_car,
                });
                self.emit(LirInstr::StoreLocal {
                    slot: var_slot, // Store to loop variable, not iterator!
                    src: car_reg,
                });

                // Evaluate body
                self.lower_expr(body)?;

                // Advance iterator: iter_slot = cdr(iter_slot)
                let iter_for_cdr = self.fresh_reg();
                self.emit(LirInstr::LoadLocal {
                    dst: iter_for_cdr,
                    slot: iter_slot,
                });
                let cdr_reg = self.fresh_reg();
                self.emit(LirInstr::Cdr {
                    dst: cdr_reg,
                    pair: iter_for_cdr,
                });
                self.emit(LirInstr::StoreLocal {
                    slot: iter_slot, // Update iterator, not var
                    src: cdr_reg,
                });

                // Loop back
                self.emit(LirInstr::JumpInline {
                    label_id: loop_label_id,
                });

                // Exit label
                self.emit(LirInstr::LabelMarker {
                    label_id: exit_label_id,
                });

                self.emit_const(LirConst::Nil)
            }

            HirKind::And(exprs) => {
                if exprs.is_empty() {
                    return self.emit_const(LirConst::Bool(true));
                }

                let exit_label_id = self.next_label;
                self.next_label += 1;

                let result_reg = self.fresh_reg();

                for (i, expr) in exprs.iter().enumerate() {
                    let expr_reg = self.lower_expr(expr)?;

                    if i < exprs.len() - 1 {
                        // Not the last expression - need to handle short-circuit
                        // Dup the value: one copy for potential return, one for the test
                        self.emit(LirInstr::Dup {
                            dst: result_reg,
                            src: expr_reg,
                        });

                        // If false, jump to exit (short-circuit)
                        // This pops expr_reg, but result_reg (the dup) remains
                        self.emit(LirInstr::JumpIfFalseInline {
                            cond: expr_reg,
                            label_id: exit_label_id,
                        });

                        // If we didn't short-circuit, pop the duplicate
                        // (we'll compute a new result in the next iteration)
                        self.emit(LirInstr::Pop { src: result_reg });
                    } else {
                        // Last expression - just move to result_reg
                        self.emit(LirInstr::Move {
                            dst: result_reg,
                            src: expr_reg,
                        });
                    }
                }

                // Exit label
                self.emit(LirInstr::LabelMarker {
                    label_id: exit_label_id,
                });

                Ok(result_reg)
            }

            HirKind::Or(exprs) => {
                if exprs.is_empty() {
                    return self.emit_const(LirConst::Bool(false));
                }

                let exit_label_id = self.next_label;
                self.next_label += 1;

                let result_reg = self.fresh_reg();

                for (i, expr) in exprs.iter().enumerate() {
                    let expr_reg = self.lower_expr(expr)?;

                    if i < exprs.len() - 1 {
                        // Not the last expression - need to handle short-circuit
                        // Dup the value: one copy for potential return, one for the test
                        self.emit(LirInstr::Dup {
                            dst: result_reg,
                            src: expr_reg,
                        });

                        // If true, jump to exit (short-circuit)
                        // We use JumpIfFalseInline to skip to next, then JumpInline to exit
                        let next_label_id = self.next_label;
                        self.next_label += 1;

                        // If false, jump to next (don't short-circuit)
                        // This pops expr_reg, but result_reg (the dup) remains
                        self.emit(LirInstr::JumpIfFalseInline {
                            cond: expr_reg,
                            label_id: next_label_id,
                        });

                        // If we get here, expr was true, jump to exit
                        self.emit(LirInstr::JumpInline {
                            label_id: exit_label_id,
                        });

                        // Next label - we didn't short-circuit
                        self.emit(LirInstr::LabelMarker {
                            label_id: next_label_id,
                        });

                        // Pop the duplicate (we'll compute a new result in the next iteration)
                        self.emit(LirInstr::Pop { src: result_reg });
                    } else {
                        // Last expression - just move to result_reg
                        self.emit(LirInstr::Move {
                            dst: result_reg,
                            src: expr_reg,
                        });
                    }
                }

                self.emit(LirInstr::LabelMarker {
                    label_id: exit_label_id,
                });
                Ok(result_reg)
            }

            HirKind::Yield(value) => {
                let value_reg = self.lower_expr(value)?;
                let dst = self.fresh_reg();
                self.emit(LirInstr::Yield {
                    dst,
                    value: value_reg,
                });
                Ok(dst)
            }

            HirKind::Quote(value) => {
                // Quote produces the pre-computed Value as a constant
                self.emit_value_const(*value)
            }

            HirKind::Throw(value) => {
                let value_reg = self.lower_expr(value)?;
                self.emit(LirInstr::Throw { value: value_reg });
                self.emit_const(LirConst::Nil) // Unreachable but need a result
            }

            HirKind::Cond {
                clauses,
                else_branch,
            } => {
                if clauses.is_empty() {
                    return if let Some(else_expr) = else_branch {
                        self.lower_expr(else_expr)
                    } else {
                        self.emit_const(LirConst::Nil)
                    };
                }

                let exit_label_id = self.next_label;
                self.next_label += 1;

                let result_reg = self.fresh_reg();

                for (test, body) in clauses {
                    let test_reg = self.lower_expr(test)?;
                    let next_label_id = self.next_label;
                    self.next_label += 1;

                    // Jump to next clause if test is false
                    self.emit(LirInstr::JumpIfFalseInline {
                        cond: test_reg,
                        label_id: next_label_id,
                    });

                    // Evaluate body
                    let body_reg = self.lower_expr(body)?;
                    self.emit(LirInstr::Move {
                        dst: result_reg,
                        src: body_reg,
                    });

                    // Jump to exit
                    self.emit(LirInstr::JumpInline {
                        label_id: exit_label_id,
                    });

                    // Next clause label
                    self.emit(LirInstr::LabelMarker {
                        label_id: next_label_id,
                    });
                }

                // Else branch
                if let Some(else_expr) = else_branch {
                    let else_reg = self.lower_expr(else_expr)?;
                    self.emit(LirInstr::Move {
                        dst: result_reg,
                        src: else_reg,
                    });
                } else {
                    let nil_reg = self.emit_const(LirConst::Nil)?;
                    self.emit(LirInstr::Move {
                        dst: result_reg,
                        src: nil_reg,
                    });
                }

                // Exit label
                self.emit(LirInstr::LabelMarker {
                    label_id: exit_label_id,
                });

                Ok(result_reg)
            }

            HirKind::Match { value, arms } => {
                // Evaluate the value to match
                let value_reg = self.lower_expr(value)?;

                // Store the match value to a local slot so we can reload it for each arm.
                // This is necessary because the stack-based emitter loses track of registers
                // across control flow (jumps between match arms).
                let match_value_slot = self.current_func.num_locals;
                self.current_func.num_locals += 1;
                self.emit(LirInstr::StoreLocal {
                    slot: match_value_slot,
                    src: value_reg,
                });

                // Allocate result register
                let result_reg = self.fresh_reg();

                // Allocate end label
                let end_label_id = self.next_label;
                self.next_label += 1;

                // Process each arm
                for (pattern, guard, body) in arms {
                    // Allocate label for next arm (if pattern doesn't match)
                    let next_arm_label_id = self.next_label;
                    self.next_label += 1;

                    // Reload the match value from the local slot for each arm.
                    // This ensures the value is available even after control flow jumps.
                    let arm_value_reg = self.fresh_reg();
                    self.emit(LirInstr::LoadLocal {
                        dst: arm_value_reg,
                        slot: match_value_slot,
                    });

                    // Generate pattern matching code
                    // This will emit JumpIfFalseInline to next_arm_label_id if pattern doesn't match
                    self.lower_pattern_match(pattern, arm_value_reg, next_arm_label_id)?;

                    // If we reach here, pattern matched
                    // Check guard if present
                    if let Some(guard_expr) = guard {
                        let guard_reg = self.lower_expr(guard_expr)?;
                        self.emit(LirInstr::JumpIfFalseInline {
                            cond: guard_reg,
                            label_id: next_arm_label_id,
                        });
                    }

                    // Lower body
                    let body_reg = self.lower_expr(body)?;
                    self.emit(LirInstr::Move {
                        dst: result_reg,
                        src: body_reg,
                    });

                    // Jump to end
                    self.emit(LirInstr::JumpInline {
                        label_id: end_label_id,
                    });

                    // Next arm label
                    self.emit(LirInstr::LabelMarker {
                        label_id: next_arm_label_id,
                    });
                }

                // If no arm matched, return nil (or could be an error)
                let nil_reg = self.emit_const(LirConst::Nil)?;
                self.emit(LirInstr::Move {
                    dst: result_reg,
                    src: nil_reg,
                });

                // End label
                self.emit(LirInstr::LabelMarker {
                    label_id: end_label_id,
                });

                Ok(result_reg)
            }
            HirKind::HandlerCase { body, handlers } => {
                let result_reg = self.fresh_reg();

                // Labels
                let handler_start_label = self.next_label;
                self.next_label += 1;
                let end_label = self.next_label;
                self.next_label += 1;

                // Emit PushHandler pointing to handler code
                self.emit(LirInstr::PushHandler {
                    handler_label: Label(handler_start_label),
                });

                // Compile protected body
                let body_reg = self.lower_expr(body)?;
                self.emit(LirInstr::Move {
                    dst: result_reg,
                    src: body_reg,
                });

                // Success path: pop handler and jump to end
                self.emit(LirInstr::PopHandler);
                self.emit(LirInstr::JumpInline {
                    label_id: end_label,
                });

                // Handler code starts here
                self.emit(LirInstr::LabelMarker {
                    label_id: handler_start_label,
                });
                self.emit(LirInstr::CheckException);

                // Compile each handler clause
                for (exception_id, var_id, handler_body) in handlers.iter() {
                    let next_handler_label = self.next_label;
                    self.next_label += 1;

                    // Match exception type
                    let match_result = self.fresh_reg();
                    self.emit(LirInstr::MatchException {
                        dst: match_result,
                        exception_id: *exception_id as u16,
                    });
                    self.emit(LirInstr::JumpIfFalseInline {
                        cond: match_result,
                        label_id: next_handler_label,
                    });

                    // Allocate slot for the exception variable and store exception to it
                    let var_slot = self.allocate_slot(*var_id);

                    // Load the current exception and store to local
                    let exc_reg = self.fresh_reg();
                    self.emit(LirInstr::LoadException { dst: exc_reg });
                    self.emit(LirInstr::StoreLocal {
                        slot: var_slot,
                        src: exc_reg,
                    });

                    // Clear the exception BEFORE executing the handler body.
                    // This ensures that if the handler body yields, the exception
                    // won't be propagated to the caller.
                    self.emit(LirInstr::ClearException);

                    // Compile handler body (now can find var_id via LoadLocal)
                    let handler_reg = self.lower_expr(handler_body)?;
                    self.emit(LirInstr::Move {
                        dst: result_reg,
                        src: handler_reg,
                    });

                    // Jump to end
                    self.emit(LirInstr::JumpInline {
                        label_id: end_label,
                    });

                    // Next handler label
                    self.emit(LirInstr::LabelMarker {
                        label_id: next_handler_label,
                    });
                }

                // No handler matched — re-raise exception to propagate
                // to next enclosing handler
                self.emit(LirInstr::ReraiseException);
                // Jump to unreachable label to prevent fall-through to ClearException
                // The exception interrupt mechanism will jump to the next handler
                let unreachable_label = self.next_label;
                self.next_label += 1;
                self.emit(LirInstr::JumpInline {
                    label_id: unreachable_label,
                });

                // End label (reached by success path and matched handler paths)
                self.emit(LirInstr::LabelMarker {
                    label_id: end_label,
                });
                // Note: ClearException is now emitted BEFORE the handler body,
                // not after, to ensure it's executed even if the handler yields.

                Ok(result_reg)
            }
            HirKind::HandlerBind { body, .. } => self.lower_expr(body),
            HirKind::Module { body, .. } => self.lower_expr(body),
            HirKind::Import { .. } => self.emit_const(LirConst::Nil),
            HirKind::ModuleRef { .. } => self.emit_const(LirConst::Nil),
        }
    }

    // === Helper Methods ===

    /// Lower pattern matching code
    /// Emits code that checks if value_reg matches the pattern
    /// If it doesn't match, jumps to fail_label_id
    /// If it matches, binds any variables and falls through
    fn lower_pattern_match(
        &mut self,
        pattern: &HirPattern,
        value_reg: Reg,
        fail_label_id: u32,
    ) -> Result<(), String> {
        match pattern {
            HirPattern::Wildcard => {
                // Wildcard always matches, do nothing
                Ok(())
            }
            HirPattern::Nil => {
                // Check if value is nil (NOT empty_list)
                // nil and '() are distinct values with distinct semantics
                let is_nil_reg = self.fresh_reg();
                self.emit(LirInstr::IsNil {
                    dst: is_nil_reg,
                    src: value_reg,
                });

                // If NOT nil, fail
                self.emit(LirInstr::JumpIfFalseInline {
                    cond: is_nil_reg,
                    label_id: fail_label_id,
                });

                Ok(())
            }
            HirPattern::Literal(lit) => {
                // Check if value equals literal
                let lit_reg = match lit {
                    PatternLiteral::Bool(b) => self.emit_const(LirConst::Bool(*b))?,
                    PatternLiteral::Int(n) => self.emit_const(LirConst::Int(*n))?,
                    PatternLiteral::Float(f) => self.emit_const(LirConst::Float(*f))?,
                    PatternLiteral::String(s) => self.emit_const(LirConst::String(s.clone()))?,
                    PatternLiteral::Keyword(sym) => self.emit_const(LirConst::Keyword(*sym))?,
                };

                let eq_reg = self.fresh_reg();
                self.emit(LirInstr::Compare {
                    dst: eq_reg,
                    op: CmpOp::Eq,
                    lhs: value_reg,
                    rhs: lit_reg,
                });
                self.emit(LirInstr::JumpIfFalseInline {
                    cond: eq_reg,
                    label_id: fail_label_id,
                });
                Ok(())
            }
            HirPattern::Var(binding_id) => {
                // Bind the value to the variable
                let slot = self.allocate_slot(*binding_id);
                self.emit(LirInstr::StoreLocal {
                    slot,
                    src: value_reg,
                });
                Ok(())
            }
            HirPattern::Cons { head, tail } => {
                // Check if value is a pair
                let is_pair_reg = self.fresh_reg();
                self.emit(LirInstr::IsPair {
                    dst: is_pair_reg,
                    src: value_reg,
                });
                self.emit(LirInstr::JumpIfFalseInline {
                    cond: is_pair_reg,
                    label_id: fail_label_id,
                });

                // Extract head and tail
                let head_reg = self.fresh_reg();
                self.emit(LirInstr::Car {
                    dst: head_reg,
                    pair: value_reg,
                });

                let tail_reg = self.fresh_reg();
                self.emit(LirInstr::Cdr {
                    dst: tail_reg,
                    pair: value_reg,
                });

                // Recursively match head and tail
                // Both must match, so they both jump to fail_label_id on failure
                self.lower_pattern_match(head, head_reg, fail_label_id)?;
                self.lower_pattern_match(tail, tail_reg, fail_label_id)?;

                Ok(())
            }
            HirPattern::List(patterns) => {
                // Check if value is a list of the right length
                // Iterate through patterns and match each element

                let mut current_reg = value_reg;

                for pat in patterns.iter() {
                    // Duplicate current so we can check if it's a pair without losing it
                    let current_dup = self.fresh_reg();
                    self.emit(LirInstr::Dup {
                        dst: current_dup,
                        src: current_reg,
                    });

                    // Check if current is a pair (using the duplicate)
                    let is_pair_reg = self.fresh_reg();
                    self.emit(LirInstr::IsPair {
                        dst: is_pair_reg,
                        src: current_dup,
                    });
                    self.emit(LirInstr::JumpIfFalseInline {
                        cond: is_pair_reg,
                        label_id: fail_label_id,
                    });

                    // Store current to a temporary slot so we can load it twice
                    let temp_slot = self.current_func.num_locals;
                    self.current_func.num_locals += 1;
                    self.emit(LirInstr::StoreLocal {
                        slot: temp_slot,
                        src: current_reg,
                    });

                    // Load for car extraction
                    let current_for_car = self.fresh_reg();
                    self.emit(LirInstr::LoadLocal {
                        dst: current_for_car,
                        slot: temp_slot,
                    });

                    // Extract head
                    let head_reg = self.fresh_reg();
                    self.emit(LirInstr::Car {
                        dst: head_reg,
                        pair: current_for_car,
                    });

                    // Match head against pattern
                    self.lower_pattern_match(pat, head_reg, fail_label_id)?;

                    // Load for cdr extraction
                    let current_for_cdr = self.fresh_reg();
                    self.emit(LirInstr::LoadLocal {
                        dst: current_for_cdr,
                        slot: temp_slot,
                    });

                    // Extract tail for next iteration
                    let tail_reg = self.fresh_reg();
                    self.emit(LirInstr::Cdr {
                        dst: tail_reg,
                        pair: current_for_cdr,
                    });

                    current_reg = tail_reg;
                }

                // Check that tail is empty_list (list ends)
                // Proper lists end with empty_list ()

                // Load current_reg for the empty_list check
                let empty_list_reg = self.fresh_reg();
                self.emit(LirInstr::ValueConst {
                    dst: empty_list_reg,
                    value: Value::EMPTY_LIST,
                });
                let is_empty_reg = self.fresh_reg();
                self.emit(LirInstr::Compare {
                    dst: is_empty_reg,
                    op: CmpOp::Eq,
                    lhs: current_reg,
                    rhs: empty_list_reg,
                });
                // If NOT empty_list, fail
                self.emit(LirInstr::JumpIfFalseInline {
                    cond: is_empty_reg,
                    label_id: fail_label_id,
                });

                Ok(())
            }
            HirPattern::Vector(_patterns) => {
                // TODO: Implement vector pattern matching
                Err("Vector pattern matching not yet implemented".to_string())
            }
        }
    }

    fn fresh_reg(&mut self) -> Reg {
        let r = Reg::new(self.next_reg);
        self.next_reg += 1;
        r
    }

    fn allocate_slot(&mut self, binding: BindingId) -> u16 {
        // Inside a lambda, slots need to account for the captures offset
        // Environment layout: [captures..., params..., locally_defined...]
        // num_locals tracks params + locally_defined (NOT captures)
        // But binding_to_slot needs the actual index in the environment
        let slot = if self.in_lambda {
            self.num_captures + self.current_func.num_locals
        } else {
            self.current_func.num_locals
        };
        self.current_func.num_locals += 1;
        self.binding_to_slot.insert(binding, slot);
        slot
    }

    fn emit(&mut self, instr: LirInstr) {
        self.current_block.instructions.push(instr);
    }

    fn emit_const(&mut self, c: LirConst) -> Result<Reg, String> {
        let dst = self.fresh_reg();
        self.emit(LirInstr::Const { dst, value: c });
        Ok(dst)
    }

    fn emit_value_const(&mut self, value: crate::value::Value) -> Result<Reg, String> {
        let dst = self.fresh_reg();
        self.emit(LirInstr::ValueConst { dst, value });
        Ok(dst)
    }

    fn terminate(&mut self, term: Terminator) {
        self.current_block.terminator = term;
    }

    fn finish_block(&mut self) {
        let block = std::mem::replace(&mut self.current_block, BasicBlock::new(Label(0)));
        self.current_func.blocks.push(block);
    }
}

impl Default for Lowerer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::Span;

    fn make_span() -> Span {
        Span::new(0, 0, 1, 1)
    }

    #[test]
    fn test_lower_int() {
        let mut lowerer = Lowerer::new();
        let hir = Hir::pure(HirKind::Int(42), make_span());
        let func = lowerer.lower(&hir).unwrap();
        assert!(!func.blocks.is_empty());
    }

    #[test]
    fn test_lower_if() {
        let mut lowerer = Lowerer::new();
        let hir = Hir::pure(
            HirKind::If {
                cond: Box::new(Hir::pure(HirKind::Bool(true), make_span())),
                then_branch: Box::new(Hir::pure(HirKind::Int(1), make_span())),
                else_branch: Box::new(Hir::pure(HirKind::Int(2), make_span())),
            },
            make_span(),
        );
        let func = lowerer.lower(&hir).unwrap();
        // If is now emitted inline in a single block
        assert_eq!(func.blocks.len(), 1);
        // Should have inline jump instructions
        assert!(func.blocks[0]
            .instructions
            .iter()
            .any(|i| matches!(i, LirInstr::JumpIfFalseInline { .. })));
    }

    #[test]
    fn test_lower_begin() {
        let mut lowerer = Lowerer::new();
        let hir = Hir::pure(
            HirKind::Begin(vec![
                Hir::pure(HirKind::Int(1), make_span()),
                Hir::pure(HirKind::Int(2), make_span()),
            ]),
            make_span(),
        );
        let func = lowerer.lower(&hir).unwrap();
        assert!(!func.blocks.is_empty());
    }
}
