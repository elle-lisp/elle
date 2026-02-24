//! Binding-related lowering: let, letrec, define, set

use super::*;

impl Lowerer {
    pub(super) fn lower_let(
        &mut self,
        bindings: &[(Binding, Hir)],
        body: &Hir,
    ) -> Result<Reg, String> {
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
        bindings: &[(Binding, Hir)],
        body: &Hir,
    ) -> Result<Reg, String> {
        // First allocate all slots with nil (or cells containing nil)
        for (binding, _) in bindings {
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
        for (binding, init) in bindings {
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
        self.lower_expr(body)
    }

    pub(super) fn lower_define(&mut self, binding: Binding, value: &Hir) -> Result<Reg, String> {
        if binding.is_global() {
            let sym = binding.name();

            // For immutable bindings with literal values, record for LoadConst optimization
            if binding.is_immutable() {
                if let Some(literal_value) = Self::hir_to_literal_value(value) {
                    self.immutable_values.insert(binding, literal_value);
                }
            }

            let value_reg = self.lower_expr(value)?;
            self.emit(LirInstr::StoreGlobal {
                sym,
                src: value_reg,
            });
            Ok(value_reg)
        } else {
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
    }

    pub(super) fn lower_set(&mut self, target: &Binding, value: &Hir) -> Result<Reg, String> {
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
        } else if target.is_global() {
            self.emit(LirInstr::StoreGlobal {
                sym: target.name(),
                src: value_reg,
            });
        } else {
            return Err(format!("Unknown binding: {:?}", target));
        }
        Ok(value_reg)
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
                        // Need both car and cdr. Dup first so we have
                        // two copies — CdrOrNil consumes the original, CarOrNil
                        // consumes the dup.
                        let dup = self.fresh_reg();
                        self.emit(LirInstr::Dup {
                            dst: dup,
                            src: current,
                        });
                        let cdr = self.fresh_reg();
                        self.emit(LirInstr::CdrOrNil {
                            dst: cdr,
                            src: current,
                        });
                        let car = self.fresh_reg();
                        self.emit(LirInstr::CarOrNil { dst: car, src: dup });
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
                let mut current = value_reg;
                let need_rest = rest.is_some();
                for (i, element) in elements.iter().enumerate() {
                    let is_last = i == elements.len() - 1 && !need_rest;
                    let src = if is_last {
                        // Last element, no rest: consume the array directly
                        current
                    } else {
                        // Not last (or has rest): dup the array
                        let dup = self.fresh_reg();
                        self.emit(LirInstr::Dup {
                            dst: dup,
                            src: current,
                        });
                        let src = current;
                        current = dup;
                        src
                    };
                    let elem = self.fresh_reg();
                    self.emit(LirInstr::ArrayRefOrNil {
                        dst: elem,
                        src,
                        index: i as u16,
                    });
                    self.lower_destructure(element, elem)?;
                }
                // Bind the remaining array slice to the rest pattern.
                // For arrays, we need a slice-from-index operation.
                // Use ArraySliceFrom instruction (to be added).
                if let Some(rest_pat) = rest {
                    let slice = self.fresh_reg();
                    self.emit(LirInstr::ArraySliceFrom {
                        dst: slice,
                        src: current,
                        index: elements.len() as u16,
                    });
                    self.lower_destructure(rest_pat, slice)?;
                }
                Ok(())
            }
            _ => Err(format!("unsupported destructuring pattern: {:?}", pattern)),
        }
    }

    /// Store a value into a binding, consuming it from the stack.
    /// Used by lower_destructure.
    fn lower_bind_value(&mut self, binding: Binding, value_reg: Reg) -> Result<Reg, String> {
        if binding.is_global() {
            self.emit(LirInstr::StoreGlobal {
                sym: binding.name(),
                src: value_reg,
            });
            // Pop the pushed-back value — destructuring doesn't need it
            // as an expression result.
            self.emit(LirInstr::Pop { src: value_reg });
            Ok(value_reg)
        } else {
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
