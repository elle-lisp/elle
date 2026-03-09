//! Binding-related lowering: let, letrec, define, set

use super::*;
use crate::hir::PatternKey;

impl Lowerer {
    pub(super) fn lower_let(
        &mut self,
        bindings: &[(Binding, Hir)],
        body: &Hir,
    ) -> Result<Reg, String> {
        let scoped = self.can_scope_allocate_let(bindings, body);
        if scoped {
            self.emit_region_enter();
        }

        // Allocate slots and lower initializers
        for (binding, init) in bindings {
            let init_reg = self.lower_expr(init)?;
            let slot = self.allocate_slot(*binding);

            // Inside lambdas, let-bound variables live in the closure
            // environment and must be accessed via LoadCapture/StoreCapture
            if self.in_lambda {
                self.upvalue_bindings.insert(*binding);
            }

            // Check if this binding needs to be wrapped in a cell
            let needs_cell = binding.needs_cell();

            if self.in_lambda {
                // Inside a lambda, use closure environment via StoreCapture.
                // The VM's Call handler already creates LocalCell(NIL) slots
                // for locally-defined variables, so we don't need MakeCell here.
                // StoreCapture handles updating cells automatically.
                self.emit(LirInstr::StoreCapture {
                    index: slot,
                    src: init_reg,
                });
            } else {
                // Outside lambdas, use stack-based locals
                if needs_cell {
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
        }
        let result = self.lower_expr(body)?;
        if scoped {
            self.emit_region_exit();
        }
        Ok(result)
    }

    pub(super) fn lower_letrec(
        &mut self,
        bindings: &[(Binding, Hir)],
        body: &Hir,
    ) -> Result<Reg, String> {
        let scoped = self.can_scope_allocate_letrec(bindings, body);
        if scoped {
            self.emit_region_enter();
        }

        // First allocate all slots with nil (or cells containing nil)
        for (binding, _) in bindings.iter() {
            let nil_reg = self.emit_const(LirConst::Nil)?;
            let slot = self.allocate_slot(*binding);

            // Inside lambdas, letrec-bound variables live in the closure
            // environment and must be accessed via LoadCapture/StoreCapture
            if self.in_lambda {
                self.upvalue_bindings.insert(*binding);
            }

            // Check if this binding needs to be wrapped in a cell
            let needs_cell = binding.needs_cell();

            if self.in_lambda {
                // Inside a lambda, the VM's Call handler already creates
                // LocalCell(NIL) slots. No need to initialize here.
                // StoreCapture will update the cell contents.
                self.emit(LirInstr::StoreCapture {
                    index: slot,
                    src: nil_reg,
                });
            } else if needs_cell {
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
        for (binding, init) in bindings.iter() {
            let init_reg = self.lower_expr(init)?;
            let slot = self.binding_to_slot[binding];

            // Check if this binding needs cell update
            let needs_cell = binding.needs_cell();

            if self.in_lambda {
                // Inside a lambda, StoreCapture handles cell update
                self.emit(LirInstr::StoreCapture {
                    index: slot,
                    src: init_reg,
                });
            } else if needs_cell {
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
        let result = self.lower_expr(body)?;
        if scoped {
            self.emit_region_exit();
        }
        Ok(result)
    }

    pub(super) fn lower_define(&mut self, binding: Binding, value: &Hir) -> Result<Reg, String> {
        // Local define
        // Allocate the slot BEFORE lowering the value so that recursive
        // references can find the binding (like letrec)
        // The slot might already be allocated by the Begin pre-pass
        let slot = if let Some(&existing_slot) = self.binding_to_slot.get(&binding) {
            existing_slot
        } else {
            self.allocate_slot(binding)
        };

        // Inside lambdas, local variables are part of the closure environment
        if self.in_lambda {
            self.upvalue_bindings.insert(binding);
        }

        // Check if this binding needs to be wrapped in a cell
        let needs_cell = binding.needs_cell();

        // Now lower the value (which can reference the binding)
        let value_reg = self.lower_expr(value)?;

        if self.in_lambda {
            // Inside a lambda, use closure environment via StoreCapture
            // StoreCapture handles cells automatically
            self.emit(LirInstr::StoreCapture {
                index: slot,
                src: value_reg,
            });
            let result = self.fresh_reg();
            self.emit(LirInstr::LoadCapture {
                dst: result,
                index: slot,
            });
            Ok(result)
        } else if needs_cell {
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
            // Reload from cell
            let cell_reg2 = self.fresh_reg();
            self.emit(LirInstr::LoadLocal {
                dst: cell_reg2,
                slot,
            });
            let result = self.fresh_reg();
            self.emit(LirInstr::LoadCell {
                dst: result,
                cell: cell_reg2,
            });
            Ok(result)
        } else {
            self.emit(LirInstr::StoreLocal {
                slot,
                src: value_reg,
            });
            let result = self.fresh_reg();
            self.emit(LirInstr::LoadLocal { dst: result, slot });
            Ok(result)
        }
    }

    pub(super) fn lower_assign(&mut self, target: &Binding, value: &Hir) -> Result<Reg, String> {
        let value_reg = self.lower_expr(value)?;

        // Check if this binding needs cell update
        let needs_cell = target.needs_cell();

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
                let result = self.fresh_reg();
                self.emit(LirInstr::LoadCapture {
                    dst: result,
                    index: slot,
                });
                Ok(result)
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
                let cell_reg2 = self.fresh_reg();
                self.emit(LirInstr::LoadLocal {
                    dst: cell_reg2,
                    slot,
                });
                let result = self.fresh_reg();
                self.emit(LirInstr::LoadCell {
                    dst: result,
                    cell: cell_reg2,
                });
                Ok(result)
            } else {
                // For simple local variables, store directly
                self.emit(LirInstr::StoreLocal {
                    slot,
                    src: value_reg,
                });
                let result = self.fresh_reg();
                self.emit(LirInstr::LoadLocal { dst: result, slot });
                Ok(result)
            }
        } else {
            Err(format!("Unknown binding: {:?}", target))
        }
    }

    /// Lower a Destructure node: evaluate the value, then destructure into bindings.
    /// Returns a nil register (destructuring is a statement, not an expression).
    pub(super) fn lower_destructure_expr(
        &mut self,
        pattern: &HirPattern,
        value: &Hir,
        _span: &Span,
    ) -> Result<Reg, String> {
        let value_reg = self.lower_expr(value)?;
        self.lower_destructure(pattern, value_reg)?;
        // Destructure produces nil as its expression value
        self.emit_const(LirConst::Nil)
    }

    /// Recursively destructure a value into pattern bindings.
    fn lower_destructure(&mut self, pattern: &HirPattern, value_reg: Reg) -> Result<(), String> {
        match pattern {
            HirPattern::Wildcard => {
                // Discard the value — don't bind it
                Ok(())
            }
            HirPattern::Var(binding) => {
                self.lower_bind_value(*binding, value_reg)?;
                Ok(())
            }
            HirPattern::List { elements, rest } => {
                let mut current = value_reg;
                let has_rest = rest.is_some();

                // Allocate one temp slot for the entire list traversal
                let temp_slot = self.current_func.num_locals;
                self.current_func.num_locals += 1;

                for (i, element) in elements.iter().enumerate() {
                    let is_last = i == elements.len() - 1 && !has_rest;
                    if is_last {
                        // Last fixed element, no rest: just take car
                        let car = self.fresh_reg();
                        self.emit(LirInstr::CarOrNil {
                            dst: car,
                            src: current,
                        });
                        self.lower_destructure(element, car)?;
                    } else {
                        // Store current to temp slot, reload for each extraction
                        self.emit(LirInstr::StoreLocal {
                            slot: temp_slot,
                            src: current,
                        });

                        let load_for_cdr = self.fresh_reg();
                        self.emit(LirInstr::LoadLocal {
                            dst: load_for_cdr,
                            slot: temp_slot,
                        });
                        let cdr = self.fresh_reg();
                        self.emit(LirInstr::CdrOrNil {
                            dst: cdr,
                            src: load_for_cdr,
                        });

                        let load_for_car = self.fresh_reg();
                        self.emit(LirInstr::LoadLocal {
                            dst: load_for_car,
                            slot: temp_slot,
                        });
                        let car = self.fresh_reg();
                        self.emit(LirInstr::CarOrNil {
                            dst: car,
                            src: load_for_car,
                        });

                        self.lower_destructure(element, car)?;
                        current = cdr;
                    }
                }
                // Bind the remaining tail to the rest pattern
                if let Some(rest_pat) = rest {
                    self.lower_destructure(rest_pat, current)?;
                }
                Ok(())
            }
            HirPattern::Array { elements, rest } => {
                // Allocate one temp slot for the array
                let temp_slot = self.current_func.num_locals;
                self.current_func.num_locals += 1;
                self.emit(LirInstr::StoreLocal {
                    slot: temp_slot,
                    src: value_reg,
                });

                for (i, element) in elements.iter().enumerate() {
                    // Reload from slot for each extraction
                    let reloaded = self.fresh_reg();
                    self.emit(LirInstr::LoadLocal {
                        dst: reloaded,
                        slot: temp_slot,
                    });
                    let elem = self.fresh_reg();
                    self.emit(LirInstr::ArrayRefOrNil {
                        dst: elem,
                        src: reloaded,
                        index: i as u16,
                    });
                    self.lower_destructure(element, elem)?;
                }
                // Bind the remaining array slice to the rest pattern.
                // For arrays, we need a slice-from-index operation.
                // Use ArraySliceFrom instruction (to be added).
                if let Some(rest_pat) = rest {
                    let reloaded = self.fresh_reg();
                    self.emit(LirInstr::LoadLocal {
                        dst: reloaded,
                        slot: temp_slot,
                    });
                    let slice = self.fresh_reg();
                    self.emit(LirInstr::ArraySliceFrom {
                        dst: slice,
                        src: reloaded,
                        index: elements.len() as u16,
                    });
                    self.lower_destructure(rest_pat, slice)?;
                }
                Ok(())
            }
            HirPattern::Tuple { elements, rest } => {
                // Tuples are immutable indexed sequences, like arrays
                let temp_slot = self.current_func.num_locals;
                self.current_func.num_locals += 1;
                self.emit(LirInstr::StoreLocal {
                    slot: temp_slot,
                    src: value_reg,
                });

                for (i, element) in elements.iter().enumerate() {
                    let reloaded = self.fresh_reg();
                    self.emit(LirInstr::LoadLocal {
                        dst: reloaded,
                        slot: temp_slot,
                    });
                    let elem = self.fresh_reg();
                    self.emit(LirInstr::ArrayRefOrNil {
                        dst: elem,
                        src: reloaded,
                        index: i as u16,
                    });
                    self.lower_destructure(element, elem)?;
                }
                // Bind the remaining tuple slice to the rest pattern.
                if let Some(rest_pat) = rest {
                    let reloaded = self.fresh_reg();
                    self.emit(LirInstr::LoadLocal {
                        dst: reloaded,
                        slot: temp_slot,
                    });
                    let slice = self.fresh_reg();
                    self.emit(LirInstr::ArraySliceFrom {
                        dst: slice,
                        src: reloaded,
                        index: elements.len() as u16,
                    });
                    self.lower_destructure(rest_pat, slice)?;
                }
                Ok(())
            }
            HirPattern::Struct { entries } => {
                // Structs are immutable key-value maps, like tables
                let temp_slot = self.current_func.num_locals;
                self.current_func.num_locals += 1;
                self.emit(LirInstr::StoreLocal {
                    slot: temp_slot,
                    src: value_reg,
                });

                for (key, sub_pattern) in entries {
                    let reloaded = self.fresh_reg();
                    self.emit(LirInstr::LoadLocal {
                        dst: reloaded,
                        slot: temp_slot,
                    });
                    let elem = self.fresh_reg();
                    let lir_key = match key {
                        PatternKey::Keyword(k) => LirConst::Keyword(k.clone()),
                        PatternKey::Symbol(sid) => LirConst::Symbol(*sid),
                    };
                    self.emit(LirInstr::TableGetOrNil {
                        dst: elem,
                        src: reloaded,
                        key: lir_key,
                    });
                    self.lower_destructure(sub_pattern, elem)?;
                }
                Ok(())
            }
            HirPattern::Table { entries } => {
                let temp_slot = self.current_func.num_locals;
                self.current_func.num_locals += 1;
                self.emit(LirInstr::StoreLocal {
                    slot: temp_slot,
                    src: value_reg,
                });

                for (key, sub_pattern) in entries {
                    let reloaded = self.fresh_reg();
                    self.emit(LirInstr::LoadLocal {
                        dst: reloaded,
                        slot: temp_slot,
                    });
                    let elem = self.fresh_reg();
                    let lir_key = match key {
                        PatternKey::Keyword(k) => LirConst::Keyword(k.clone()),
                        PatternKey::Symbol(sid) => LirConst::Symbol(*sid),
                    };
                    self.emit(LirInstr::TableGetOrNil {
                        dst: elem,
                        src: reloaded,
                        key: lir_key,
                    });
                    self.lower_destructure(sub_pattern, elem)?;
                }
                Ok(())
            }
            _ => Err(format!("unsupported destructuring pattern: {:?}", pattern)),
        }
    }

    /// Store a value into a binding, consuming it from the stack.
    /// Used by lower_destructure.
    fn lower_bind_value(&mut self, binding: Binding, value_reg: Reg) -> Result<Reg, String> {
        // Allocate slot if not already done (Begin pre-pass may have done it)
        let slot = if let Some(&existing_slot) = self.binding_to_slot.get(&binding) {
            existing_slot
        } else {
            self.allocate_slot(binding)
        };

        if self.in_lambda {
            self.upvalue_bindings.insert(binding);
            self.emit(LirInstr::StoreCapture {
                index: slot,
                src: value_reg,
            });
        } else {
            let needs_cell = binding.needs_cell();
            if needs_cell {
                // Cell was already created in Begin pre-pass
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
}
