use super::*;

impl<'a> Lowerer<'a> {
    /// Recursively destructure a value into pattern bindings.
    /// `strict`: if true, use strict (error-signaling) instructions;
    ///           if false, use silent-nil instructions for missing/wrong-type values.
    pub(super) fn lower_destructure(
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
                    // A list rest is a borrowed subview of the scrutinee —
                    // mark its bindings (see `destructure_alias_bindings`).
                    for b in rest_pat.bindings().bindings {
                        self.destructure_alias_bindings.insert(b);
                    }
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
                        PatternKey::Keyword(k) => {
                            LirConst::Keyword(crate::value::keyword::keyword_hash(k))
                        }
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
                        PatternKey::Keyword(k) => {
                            LirConst::Keyword(crate::value::keyword::keyword_hash(k))
                        }
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
                            PatternKey::Keyword(k) => {
                                LirConst::Keyword(crate::value::keyword::keyword_hash(k))
                            }
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
                        PatternKey::Keyword(k) => {
                            LirConst::Keyword(crate::value::keyword::keyword_hash(k))
                        }
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
                            PatternKey::Keyword(k) => {
                                LirConst::Keyword(crate::value::keyword::keyword_hash(k))
                            }
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
}
