//! Binding-related lowering: let, letrec, define, set

use super::*;
use crate::hir::PatternKey;

impl<'a> Lowerer<'a> {
    pub(super) fn lower_let(
        &mut self,
        bindings: &[(Binding, Hir)],
        body: &Hir,
        hir_id: HirId,
    ) -> Result<Reg, String> {
        let region_id = if self.region_scope_check(hir_id) {
            self.scope_region_id(hir_id)
        } else {
            None
        };
        if let Some(rid) = region_id {
            self.active_region_ids.push(rid);
        }

        // Allocate slots and lower initializers
        for (binding, init) in bindings {
            self.try_seed_immutable(*binding, init);

            // Allocate the binding's slot BEFORE lowering the init,
            // and register `region_to_slot[r] = slot` so that the
            // `emit_decrefs_for(init.id)` call inside `lower_expr` —
            // which fires for unused bindings whose `free_at` is the
            // init's own HirId — can find the slot it needs to load
            // the value from for `ReleaseValueRegion`. Without this
            // pre-allocation, an unused let-bound Call result leaks
            // because the slot only exists after `lower_expr` returns.
            //
            // `allocate_slot` stamps the slot with `StoreLocal(slot,
            // nil)` so the slot is always a valid Value at the point
            // we'd load from it; the init's actual value overwrites
            // the nil shortly after.
            let slot = self.allocate_slot(*binding);
            self.record_region_slot(init.id, slot);
            let init_reg = self.lower_expr(init)?;
            let needs_capture = self.arena.get(*binding).needs_capture();

            if self.in_lambda && needs_capture {
                self.upvalue_bindings.insert(*binding);
                self.emit(LirInstr::StoreCapture {
                    index: slot,
                    src: init_reg,
                });
            } else if self.in_lambda {
                self.emit_binding_store(slot, init_reg);
            } else {
                if needs_capture {
                    let cell_reg = self.fresh_reg();
                    self.emit_alloc(LirInstr::MakeCaptureCell {
                        dst: cell_reg,
                        value: init_reg,
                    });
                    self.emit_binding_store(slot, cell_reg);
                } else {
                    self.emit_binding_store(slot, init_reg);
                }
            }
        }
        // For tail calls in scoped lets, emit FreeRegion before the
        // tail call via the pending mechanism.
        let tail_scoped = region_id.is_some() && Self::body_is_tail_call(body);
        if let Some(rid) = region_id {
            if tail_scoped {
                self.pending_free_regions.push(rid);
            }
        }
        let result = self.lower_expr(body)?;
        // Pop region from active stack BEFORE deciding whether to emit
        // FreeRegion. If the region is still in the stack, an outer scope
        // also uses it and will emit its own FreeRegion — emitting one here
        // would double-decref and free the region prematurely.
        if region_id.is_some() {
            self.active_region_ids.pop();
        }
        if tail_scoped {
            self.pending_free_regions.pop();
        }
        // Region-demise `DecrefRegion` is now emitted by
        // `lower_expr`'s `emit_decrefs_for` at each region's `free_at`
        // HirId (impl step 13). The scope-based DecrefRegion emission
        // here is gone.
        let _ = region_id;
        Ok(result)
    }

    pub(super) fn lower_letrec(
        &mut self,
        bindings: &[(Binding, Hir)],
        body: &Hir,
        hir_id: HirId,
    ) -> Result<Reg, String> {
        let region_id = if self.region_scope_check(hir_id) {
            self.scope_region_id(hir_id)
        } else {
            None
        };
        if let Some(rid) = region_id {
            self.active_region_ids.push(rid);
        }

        // First allocate all slots with nil (or cells containing nil)
        for (binding, _) in bindings.iter() {
            let nil_reg = self.emit_const(LirConst::Nil)?;
            let slot = self.allocate_slot(*binding);

            // Check if this binding needs to be wrapped in a cell
            let needs_capture = self.arena.get(*binding).needs_capture();

            if self.in_lambda && needs_capture {
                self.upvalue_bindings.insert(*binding);
                self.emit(LirInstr::StoreCapture {
                    index: slot,
                    src: nil_reg,
                });
            } else if self.in_lambda {
                self.emit_binding_store(slot, nil_reg);
            } else if needs_capture {
                let cell_reg = self.fresh_reg();
                self.emit_alloc(LirInstr::MakeCaptureCell {
                    dst: cell_reg,
                    value: nil_reg,
                });
                self.emit_binding_store(slot, cell_reg);
            } else {
                self.emit_binding_store(slot, nil_reg);
            }
        }
        // Then initialize
        for (binding, init) in bindings.iter() {
            // Set function context for lambdas so that
            // body_escapes_heap_values can detect self-tail-calls.
            if let HirKind::Lambda { params, .. } = &init.kind {
                self.current_function_binding = Some(*binding);
                self.current_function_params = Some(params.clone());
            }
            let slot = self.binding_to_slot[binding];
            // Record the slot BEFORE lowering the init so
            // `emit_decrefs_for(init.id)` inside `lower_expr` can
            // find it (matches `lower_let`'s ordering).
            self.record_region_slot(init.id, slot);
            let init_reg = self.lower_expr(init)?;
            self.current_function_binding = None;
            self.current_function_params = None;

            // Seed immutable_values after init so subsequent bindings
            // and the body can use LoadConst for this constant.
            // Skip nil inits — letrec destructure leaves are initialized
            // to nil here and later updated by a Destructure node in the body.
            // For non-nil inits, evict any stale value first (file-scope
            // duplicate names may reuse the same Binding identity).
            if !matches!(init.kind, HirKind::Nil) {
                self.immutable_values.remove(binding);
                self.try_seed_immutable(*binding, init);
            }

            // Check if this binding needs cell update
            let needs_capture = self.arena.get(*binding).needs_capture();

            let is_upvalue = self.upvalue_bindings.contains(binding);

            if self.in_lambda && is_upvalue {
                self.emit(LirInstr::StoreCapture {
                    index: slot,
                    src: init_reg,
                });
            } else if self.in_lambda {
                self.emit_binding_store(slot, init_reg);
            } else if needs_capture {
                let cell_reg = self.fresh_reg();
                self.emit(LirInstr::LoadLocal {
                    dst: cell_reg,
                    slot,
                });
                self.emit(LirInstr::StoreCaptureCell {
                    cell: cell_reg,
                    value: init_reg,
                });
            } else {
                self.emit_binding_store(slot, init_reg);
            }
        }
        let tail_scoped = region_id.is_some() && Self::body_is_tail_call(body);
        if let Some(rid) = region_id {
            if tail_scoped {
                self.pending_free_regions.push(rid);
            }
        }
        let result = self.lower_expr(body)?;
        // Pop first, then check if region is still in stack (shared with
        // outer scope). See lower_let for rationale.
        if region_id.is_some() {
            self.active_region_ids.pop();
        }
        if tail_scoped {
            self.pending_free_regions.pop();
        }
        // Region-demise DecrefRegion emission is in `lower_expr` (step 13).
        let _ = region_id;
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

        // Check if this binding needs to be wrapped in a cell
        let needs_capture = self.arena.get(binding).needs_capture();

        // Only LBox-wrapped locals need upvalue treatment inside lambdas
        if self.in_lambda && needs_capture {
            self.upvalue_bindings.insert(binding);
        }

        // Set function context for lambdas so that
        // body_escapes_heap_values can detect self-tail-calls.
        if let HirKind::Lambda { params, .. } = &value.kind {
            self.current_function_binding = Some(binding);
            self.current_function_params = Some(params.clone());
        }

        // Record the slot BEFORE lowering the value so
        // `emit_decrefs_for(value.id)` inside `lower_expr` can find
        // it (matches `lower_let`'s ordering).
        self.record_region_slot(value.id, slot);

        // Now lower the value (which can reference the binding)
        let value_reg = self.lower_expr(value)?;
        self.current_function_binding = None;
        self.current_function_params = None;

        // Seed immutable_values for constant definitions
        self.try_seed_immutable(binding, value);

        if self.in_lambda && needs_capture {
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
        } else if self.in_lambda {
            self.emit_binding_store(slot, value_reg);
            let result = self.fresh_reg();
            self.emit(LirInstr::LoadLocal { dst: result, slot });
            Ok(result)
        } else if needs_capture {
            // The cell was already created in the Begin pre-pass
            let cell_reg = self.fresh_reg();
            self.emit(LirInstr::LoadLocal {
                dst: cell_reg,
                slot,
            });
            self.emit(LirInstr::StoreCaptureCell {
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
            self.emit(LirInstr::LoadCaptureCell {
                dst: result,
                cell: cell_reg2,
            });
            Ok(result)
        } else {
            self.emit_binding_store(slot, value_reg);
            let result = self.fresh_reg();
            self.emit(LirInstr::LoadLocal { dst: result, slot });
            Ok(result)
        }
    }

    pub(super) fn lower_assign(&mut self, target: &Binding, value: &Hir) -> Result<Reg, String> {
        // Evict stale constant — this binding is being mutated.
        self.immutable_values.remove(target);

        let value_reg = self.lower_expr(value)?;

        // Check if this binding needs cell update
        let needs_capture = self.arena.get(*target).needs_capture();

        // Check if this is an upvalue (capture or parameter) or a local
        let is_upvalue = self.upvalue_bindings.contains(target);

        if let Some(&slot) = self.binding_to_slot.get(target) {
            if self.in_lambda && is_upvalue && needs_capture {
                // For LBox upvalues, use StoreCapture (updates cell) + LoadCapture (unwraps)
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
            } else if needs_capture {
                // For local variables that need cells, load the cell and update it
                let cell_reg = self.fresh_reg();
                self.emit(LirInstr::LoadLocal {
                    dst: cell_reg,
                    slot,
                });
                self.emit(LirInstr::StoreCaptureCell {
                    cell: cell_reg,
                    value: value_reg,
                });
                let cell_reg2 = self.fresh_reg();
                self.emit(LirInstr::LoadLocal {
                    dst: cell_reg2,
                    slot,
                });
                let result = self.fresh_reg();
                self.emit(LirInstr::LoadCaptureCell {
                    dst: result,
                    cell: cell_reg2,
                });
                Ok(result)
            } else {
                // Drop-on-overwrite: decref_and_free the old value,
                // incref the new value. Aliasing is safe because
                // aliased values have refcount > 0 and won't be freed.
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
    /// `strict`: if true, missing/wrong-type values signal error; if false, produce nil.
    pub(super) fn lower_destructure_expr(
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

    /// Recursively destructure a value into pattern bindings.
    /// `strict`: if true, use strict (error-signaling) instructions;
    ///           if false, use silent-nil instructions for missing/wrong-type values.
    fn lower_destructure(
        &mut self,
        pattern: &HirPattern,
        value_reg: Reg,
        strict: bool,
    ) -> Result<(), String> {
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
                        // Last fixed element, no rest: just take head
                        let head = self.fresh_reg();
                        if strict {
                            self.emit(LirInstr::FirstDestructure {
                                dst: head,
                                src: current,
                            });
                        } else {
                            self.emit(LirInstr::FirstOrNil {
                                dst: head,
                                src: current,
                            });
                        }
                        self.lower_destructure(element, head, strict)?;
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
                        let tail = self.fresh_reg();
                        if strict {
                            self.emit(LirInstr::RestDestructure {
                                dst: tail,
                                src: load_for_cdr,
                            });
                        } else {
                            self.emit(LirInstr::RestOrNil {
                                dst: tail,
                                src: load_for_cdr,
                            });
                        }

                        let load_for_car = self.fresh_reg();
                        self.emit(LirInstr::LoadLocal {
                            dst: load_for_car,
                            slot: temp_slot,
                        });
                        let head = self.fresh_reg();
                        if strict {
                            self.emit(LirInstr::FirstDestructure {
                                dst: head,
                                src: load_for_car,
                            });
                        } else {
                            self.emit(LirInstr::FirstOrNil {
                                dst: head,
                                src: load_for_car,
                            });
                        }

                        self.lower_destructure(element, head, strict)?;
                        current = tail;
                    }
                }
                // Bind the remaining tail to the rest pattern
                if let Some(rest_pat) = rest {
                    self.lower_destructure(rest_pat, current, strict)?;
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
                    if strict {
                        self.emit(LirInstr::ArrayMutRefDestructure {
                            dst: elem,
                            src: reloaded,
                            index: i as u16,
                        });
                    } else {
                        self.emit(LirInstr::ArrayMutRefOrNil {
                            dst: elem,
                            src: reloaded,
                            index: i as u16,
                        });
                    }
                    self.lower_destructure(element, elem, strict)?;
                }
                if let Some(rest_pat) = rest {
                    let reloaded = self.fresh_reg();
                    self.emit(LirInstr::LoadLocal {
                        dst: reloaded,
                        slot: temp_slot,
                    });
                    let slice = self.fresh_reg();
                    self.emit(LirInstr::ArrayMutSliceFrom {
                        dst: slice,
                        src: reloaded,
                        index: elements.len() as u16,
                    });
                    self.lower_destructure(rest_pat, slice, strict)?;
                }
                Ok(())
            }
            HirPattern::Tuple { elements, rest } => {
                // Arrays are immutable indexed sequences
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
                    if strict {
                        self.emit(LirInstr::ArrayMutRefDestructure {
                            dst: elem,
                            src: reloaded,
                            index: i as u16,
                        });
                    } else {
                        self.emit(LirInstr::ArrayMutRefOrNil {
                            dst: elem,
                            src: reloaded,
                            index: i as u16,
                        });
                    }
                    self.lower_destructure(element, elem, strict)?;
                }
                // Bind the remaining array slice to the rest pattern.
                if let Some(rest_pat) = rest {
                    let reloaded = self.fresh_reg();
                    self.emit(LirInstr::LoadLocal {
                        dst: reloaded,
                        slot: temp_slot,
                    });
                    let slice = self.fresh_reg();
                    self.emit(LirInstr::ArrayMutSliceFrom {
                        dst: slice,
                        src: reloaded,
                        index: elements.len() as u16,
                    });
                    self.lower_destructure(rest_pat, slice, strict)?;
                }
                Ok(())
            }
            HirPattern::NamedStruct { entries } => {
                // &named parameter destructuring: missing keys always produce nil (not errors).
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
                    self.emit(LirInstr::StructGetOrNil {
                        dst: elem,
                        src: reloaded,
                        key: lir_key,
                    });
                    self.lower_destructure(sub_pattern, elem, false)?;
                }
                Ok(())
            }
            HirPattern::Struct { entries, rest } => {
                // Structs are immutable key-value maps
                let temp_slot = self.current_func.num_locals;
                self.current_func.num_locals += 1;
                self.emit(LirInstr::StoreLocal {
                    slot: temp_slot,
                    src: value_reg,
                });

                for (key, sub_pattern) in entries.iter() {
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
                    if strict {
                        self.emit(LirInstr::StructGetDestructure {
                            dst: elem,
                            src: reloaded,
                            key: lir_key,
                        });
                    } else {
                        self.emit(LirInstr::StructGetOrNil {
                            dst: elem,
                            src: reloaded,
                            key: lir_key,
                        });
                    }
                    self.lower_destructure(sub_pattern, elem, strict)?;
                }

                if let Some(rest_pat) = rest {
                    let reloaded = self.fresh_reg();
                    self.emit(LirInstr::LoadLocal {
                        dst: reloaded,
                        slot: temp_slot,
                    });
                    let rest_reg = self.fresh_reg();
                    let exclude: Vec<LirConst> = entries
                        .iter()
                        .map(|(key, _)| match key {
                            PatternKey::Keyword(k) => LirConst::Keyword(k.clone()),
                            PatternKey::Symbol(sid) => LirConst::Symbol(*sid),
                        })
                        .collect();
                    self.emit(LirInstr::StructRest {
                        dst: rest_reg,
                        src: reloaded,
                        exclude_keys: exclude,
                    });
                    self.lower_destructure(rest_pat, rest_reg, strict)?;
                }

                Ok(())
            }
            HirPattern::Table { entries, rest } => {
                let temp_slot = self.current_func.num_locals;
                self.current_func.num_locals += 1;
                self.emit(LirInstr::StoreLocal {
                    slot: temp_slot,
                    src: value_reg,
                });

                for (key, sub_pattern) in entries.iter() {
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
                    if strict {
                        self.emit(LirInstr::StructGetDestructure {
                            dst: elem,
                            src: reloaded,
                            key: lir_key,
                        });
                    } else {
                        self.emit(LirInstr::StructGetOrNil {
                            dst: elem,
                            src: reloaded,
                            key: lir_key,
                        });
                    }
                    self.lower_destructure(sub_pattern, elem, strict)?;
                }

                if let Some(rest_pat) = rest {
                    let reloaded = self.fresh_reg();
                    self.emit(LirInstr::LoadLocal {
                        dst: reloaded,
                        slot: temp_slot,
                    });
                    let rest_reg = self.fresh_reg();
                    let exclude: Vec<LirConst> = entries
                        .iter()
                        .map(|(key, _)| match key {
                            PatternKey::Keyword(k) => LirConst::Keyword(k.clone()),
                            PatternKey::Symbol(sid) => LirConst::Symbol(*sid),
                        })
                        .collect();
                    self.emit(LirInstr::StructRest {
                        dst: rest_reg,
                        src: reloaded,
                        exclude_keys: exclude,
                    });
                    self.lower_destructure(rest_pat, rest_reg, strict)?;
                }

                Ok(())
            }
            _ => Err(format!("unsupported destructuring pattern: {:?}", pattern)),
        }
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
    pub(super) fn lower_make_cell(&mut self, value: &Hir) -> Result<Reg, String> {
        self.lower_expr(value)
    }

    /// Lower DerefCell — currently transparent (just lowers the inner cell expr).
    ///
    /// See `lower_make_cell` for the double-handling contract. The lowerer's
    /// `lower_var` already unwraps cells for needs_capture bindings, so
    /// DerefCell's child (a Var) produces the unwrapped value directly.
    pub(super) fn lower_deref_cell(&mut self, cell: &Hir) -> Result<Reg, String> {
        self.lower_expr(cell)
    }

    /// Lower SetCell — delegates to lower_assign since the lowerer already
    /// handles cell stores. The cell child must be a Var.
    ///
    /// See `lower_make_cell` for the double-handling contract. The lowerer's
    /// `lower_assign` already stores through cells for needs_capture bindings.
    pub(super) fn lower_set_cell(&mut self, cell: &Hir, value: &Hir) -> Result<Reg, String> {
        if let HirKind::Var(binding) = &cell.kind {
            self.lower_assign(binding, value)
        } else {
            Err("SetCell: cell must be a Var".to_string())
        }
    }

    /// Store a value into a binding, consuming it from the stack.
    /// Used by lower_destructure.
    fn lower_bind_value(&mut self, binding: Binding, value_reg: Reg) -> Result<Reg, String> {
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
