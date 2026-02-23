//! Binding-related lowering: let, letrec, define, set

use super::*;

impl Lowerer {
    pub(super) fn lower_let(
        &mut self,
        bindings: &[(BindingId, Hir)],
        body: &Hir,
    ) -> Result<Reg, String> {
        // Allocate slots and lower initializers
        for (binding_id, init) in bindings {
            let init_reg = self.lower_expr(init)?;
            let slot = self.allocate_slot(*binding_id);

            // Inside lambdas, let-bound variables live in the closure
            // environment and must be accessed via LoadCapture/StoreCapture
            if self.in_lambda {
                self.upvalue_bindings.insert(*binding_id);
            }

            // Check if this binding needs to be wrapped in a cell
            let needs_cell = self
                .bindings
                .get(binding_id)
                .map(|info| info.needs_cell())
                .unwrap_or(false);

            if self.in_lambda {
                // Inside a lambda, use closure environment via StoreCapture.
                // The VM's Call handler already creates LocalCell(NIL) slots
                // for locally-defined variables, so we don't need MakeCell here.
                // StoreCapture handles updating cells automatically.
                self.emit(LirInstr::StoreCapture {
                    index: slot,
                    src: init_reg,
                });
                // StoreCapture (via StoreUpvalue) pops the value, stores it,
                // and pushes it back. For let bindings, we don't need the
                // pushed-back value (the body loads from the closure env),
                // so pop it to keep the stack clean.
                self.emit(LirInstr::Pop { src: init_reg });
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
        self.lower_expr(body)
    }

    pub(super) fn lower_letrec(
        &mut self,
        bindings: &[(BindingId, Hir)],
        body: &Hir,
    ) -> Result<Reg, String> {
        // First allocate all slots with nil (or cells containing nil)
        for (binding_id, _) in bindings {
            let nil_reg = self.emit_const(LirConst::Nil)?;
            let slot = self.allocate_slot(*binding_id);

            // Inside lambdas, letrec-bound variables live in the closure
            // environment and must be accessed via LoadCapture/StoreCapture
            if self.in_lambda {
                self.upvalue_bindings.insert(*binding_id);
            }

            // Check if this binding needs to be wrapped in a cell
            let needs_cell = self
                .bindings
                .get(binding_id)
                .map(|info| info.needs_cell())
                .unwrap_or(false);

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
        for (binding_id, init) in bindings {
            let init_reg = self.lower_expr(init)?;
            let slot = self.binding_to_slot[binding_id];

            // Check if this binding needs cell update
            let needs_cell = self
                .bindings
                .get(binding_id)
                .map(|info| info.needs_cell())
                .unwrap_or(false);

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
        self.lower_expr(body)
    }

    pub(super) fn lower_define(
        &mut self,
        name: crate::value::SymbolId,
        value: &Hir,
    ) -> Result<Reg, String> {
        // For immutable bindings with literal values, record for LoadConst optimization
        // We need to find the BindingId for this global by matching the SymbolId
        if let Some(literal_value) = Self::hir_to_literal_value(value) {
            for (bid, info) in &self.bindings {
                if info.kind == BindingKind::Global && info.name == name && info.is_immutable {
                    self.immutable_values.insert(*bid, literal_value);
                    break;
                }
            }
        }

        let value_reg = self.lower_expr(value)?;
        self.emit(LirInstr::StoreGlobal {
            sym: name,
            src: value_reg,
        });
        Ok(value_reg)
    }

    pub(super) fn lower_local_define(
        &mut self,
        binding: BindingId,
        value: &Hir,
    ) -> Result<Reg, String> {
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
        let needs_cell = self
            .bindings
            .get(&binding)
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

    pub(super) fn lower_set(&mut self, target: &BindingId, value: &Hir) -> Result<Reg, String> {
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

    /// Extract a compile-time literal Value from a HIR node, if it is a literal.
    fn hir_to_literal_value(hir: &Hir) -> Option<Value> {
        match &hir.kind {
            HirKind::Int(n) => Some(Value::int(*n)),
            HirKind::Float(f) => Some(Value::float(*f)),
            HirKind::String(s) => Some(Value::string(s.as_str())),
            HirKind::Bool(b) => Some(Value::bool(*b)),
            HirKind::Nil => Some(Value::NIL),
            HirKind::Keyword(name) => Some(Value::keyword(name)),
            HirKind::EmptyList => Some(Value::EMPTY_LIST),
            _ => None,
        }
    }
}
