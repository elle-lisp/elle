// audited: 2026-10-06
//! Lowering a binding form's destructure: the extraction each pattern shape emits, and the slot each value lands in.
//!
//! src/lir/lower/AGENTS.md
//! docs/destructuring.md

use super::*;
use crate::lir::lower::access::{excluded_keys, pattern_key_const};

/// A cursor over the collections one destructure's pattern builds, counting in
/// the order this file reaches them.
///
/// The solver keys each placeholder on the same order
/// (`HirPattern::building_rests`), so a build site takes its region by
/// position — the one handle a collection no name of the program holds has.
///
/// docs/impl/region/anchors.md
pub(in crate::lir::lower) struct RestBuilds(usize);

impl RestBuilds {
    /// A cursor at the pattern's first build.
    pub(in crate::lir::lower) fn new() -> Self {
        RestBuilds(0)
    }

    /// The index of the build site just reached, advancing past it.
    fn next(&mut self) -> usize {
        let index = self.0;
        self.0 += 1;
        index
    }
}

impl<'a> Lowerer<'a> {
    /// Recursively destructure a value into pattern bindings.
    /// `strict`: if true, use strict (error-signaling) instructions;
    ///           if false, use silent-nil instructions for missing/wrong-type values.
    /// `builds`: the cursor the rest builds take their placeholder region from.
    pub(super) fn lower_destructure(
        &mut self,
        pattern: &HirPattern,
        value_reg: Reg,
        strict: bool,
        builds: &mut RestBuilds,
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
                let temp_slot = self.fresh_local();

                for (i, element) in elements.iter().enumerate() {
                    let is_last = i == elements.len() - 1 && !has_rest;
                    if is_last {
                        // Last fixed element, no rest: just take head
                        let head = self.fresh_reg();
                        if strict {
                            self.emit(InstrRef::FirstDestructure {
                                dst: head,
                                src: current,
                            });
                        } else {
                            self.emit(InstrRef::FirstOrNil {
                                dst: head,
                                src: current,
                            });
                        }
                        self.lower_destructure(element, head, strict, builds)?;
                    } else {
                        // Store current to temp slot, reload for each extraction
                        self.emit(InstrRef::StoreLocal {
                            slot: temp_slot,
                            src: current,
                        });

                        let load_for_cdr = self.fresh_reg();
                        self.emit(InstrRef::LoadLocal {
                            dst: load_for_cdr,
                            slot: temp_slot,
                        });
                        let tail = self.fresh_reg();
                        if strict {
                            self.emit(InstrRef::RestDestructure {
                                dst: tail,
                                src: load_for_cdr,
                            });
                        } else {
                            self.emit(InstrRef::RestOrNil {
                                dst: tail,
                                src: load_for_cdr,
                            });
                        }

                        let load_for_car = self.fresh_reg();
                        self.emit(InstrRef::LoadLocal {
                            dst: load_for_car,
                            slot: temp_slot,
                        });
                        let head = self.fresh_reg();
                        if strict {
                            self.emit(InstrRef::FirstDestructure {
                                dst: head,
                                src: load_for_car,
                            });
                        } else {
                            self.emit(InstrRef::FirstOrNil {
                                dst: head,
                                src: load_for_car,
                            });
                        }

                        self.lower_destructure(element, head, strict, builds)?;
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
                    self.lower_destructure(rest_pat, current, strict, builds)?;
                }
                Ok(())
            }
            HirPattern::Array { elements, rest } => {
                // Allocate one temp slot for the array
                let temp_slot = self.fresh_local();
                self.emit(InstrRef::StoreLocal {
                    slot: temp_slot,
                    src: value_reg,
                });

                for (i, element) in elements.iter().enumerate() {
                    // Reload from slot for each extraction
                    let reloaded = self.fresh_reg();
                    self.emit(InstrRef::LoadLocal {
                        dst: reloaded,
                        slot: temp_slot,
                    });
                    let elem = self.fresh_reg();
                    if strict {
                        self.emit(InstrRef::ArrayMutRefDestructure {
                            dst: elem,
                            src: reloaded,
                            index: i as u16,
                        });
                    } else {
                        self.emit(InstrRef::ArrayMutRefOrNil {
                            dst: elem,
                            src: reloaded,
                            index: i as u16,
                        });
                    }
                    self.lower_destructure(element, elem, strict, builds)?;
                }
                if let Some(rest_pat) = rest.as_deref().filter(|_| pattern.own_rest_builds()) {
                    let slice = self.build_array_rest(temp_slot, elements.len(), builds);
                    self.lower_destructure(rest_pat, slice, strict, builds)?;
                }
                Ok(())
            }
            HirPattern::Tuple { elements, rest } => {
                // Arrays are immutable indexed sequences
                let temp_slot = self.fresh_local();
                self.emit(InstrRef::StoreLocal {
                    slot: temp_slot,
                    src: value_reg,
                });

                for (i, element) in elements.iter().enumerate() {
                    let reloaded = self.fresh_reg();
                    self.emit(InstrRef::LoadLocal {
                        dst: reloaded,
                        slot: temp_slot,
                    });
                    let elem = self.fresh_reg();
                    if strict {
                        self.emit(InstrRef::ArrayMutRefDestructure {
                            dst: elem,
                            src: reloaded,
                            index: i as u16,
                        });
                    } else {
                        self.emit(InstrRef::ArrayMutRefOrNil {
                            dst: elem,
                            src: reloaded,
                            index: i as u16,
                        });
                    }
                    self.lower_destructure(element, elem, strict, builds)?;
                }
                // Bind the remaining array slice to the rest pattern.
                if let Some(rest_pat) = rest.as_deref().filter(|_| pattern.own_rest_builds()) {
                    let slice = self.build_array_rest(temp_slot, elements.len(), builds);
                    self.lower_destructure(rest_pat, slice, strict, builds)?;
                }
                Ok(())
            }
            HirPattern::NamedStruct { entries } => {
                // &named parameter destructuring: missing keys always produce nil (not errors).
                let temp_slot = self.fresh_local();
                self.emit(InstrRef::StoreLocal {
                    slot: temp_slot,
                    src: value_reg,
                });

                for (key, sub_pattern) in entries {
                    let reloaded = self.fresh_reg();
                    self.emit(InstrRef::LoadLocal {
                        dst: reloaded,
                        slot: temp_slot,
                    });
                    let elem = self.fresh_reg();
                    let lir_key = pattern_key_const(key);
                    self.emit(InstrRef::StructGetOrNil {
                        dst: elem,
                        src: reloaded,
                        key: lir_key,
                    });
                    self.lower_destructure(sub_pattern, elem, false, builds)?;
                }
                Ok(())
            }
            HirPattern::Struct { entries, rest } => {
                // Structs are immutable key-value maps
                let temp_slot = self.fresh_local();
                self.emit(InstrRef::StoreLocal {
                    slot: temp_slot,
                    src: value_reg,
                });

                for (key, sub_pattern) in entries.iter() {
                    let reloaded = self.fresh_reg();
                    self.emit(InstrRef::LoadLocal {
                        dst: reloaded,
                        slot: temp_slot,
                    });
                    let elem = self.fresh_reg();
                    let lir_key = pattern_key_const(key);
                    if strict {
                        self.emit(InstrRef::StructGetDestructure {
                            dst: elem,
                            src: reloaded,
                            key: lir_key,
                        });
                    } else {
                        self.emit(InstrRef::StructGetOrNil {
                            dst: elem,
                            src: reloaded,
                            key: lir_key,
                        });
                    }
                    self.lower_destructure(sub_pattern, elem, strict, builds)?;
                }

                if let Some(rest_pat) = rest.as_deref().filter(|_| pattern.own_rest_builds()) {
                    let rest_reg = self.build_keyed_rest(temp_slot, entries, builds);
                    self.lower_destructure(rest_pat, rest_reg, strict, builds)?;
                }

                Ok(())
            }
            HirPattern::Table { entries, rest } => {
                let temp_slot = self.fresh_local();
                self.emit(InstrRef::StoreLocal {
                    slot: temp_slot,
                    src: value_reg,
                });

                for (key, sub_pattern) in entries.iter() {
                    let reloaded = self.fresh_reg();
                    self.emit(InstrRef::LoadLocal {
                        dst: reloaded,
                        slot: temp_slot,
                    });
                    let elem = self.fresh_reg();
                    let lir_key = pattern_key_const(key);
                    if strict {
                        self.emit(InstrRef::StructGetDestructure {
                            dst: elem,
                            src: reloaded,
                            key: lir_key,
                        });
                    } else {
                        self.emit(InstrRef::StructGetOrNil {
                            dst: elem,
                            src: reloaded,
                            key: lir_key,
                        });
                    }
                    self.lower_destructure(sub_pattern, elem, strict, builds)?;
                }

                if let Some(rest_pat) = rest.as_deref().filter(|_| pattern.own_rest_builds()) {
                    let rest_reg = self.build_keyed_rest(temp_slot, entries, builds);
                    self.lower_destructure(rest_pat, rest_reg, strict, builds)?;
                }

                Ok(())
            }
            _ => Err(format!("unsupported destructuring pattern: {:?}", pattern)),
        }
    }

    /// Build the array a sequence pattern's rest collects — every element from
    /// `index` on — and park it against the placeholder `builds` names next.
    fn build_array_rest(&mut self, temp_slot: u16, index: usize, builds: &mut RestBuilds) -> Reg {
        let reloaded = self.fresh_reg();
        self.emit(InstrRef::LoadLocal {
            dst: reloaded,
            slot: temp_slot,
        });
        let slice = self.fresh_reg();
        self.emit(InstrRef::ArrayMutSliceFrom {
            dst: slice,
            src: reloaded,
            index: index as u16,
        });
        let at = builds.next();
        self.park_rest_collection_at(at, slice)
    }

    /// Build the struct a keyed pattern's rest collects — every key `entries`
    /// did not name — and park it against the placeholder `builds` names next.
    fn build_keyed_rest(
        &mut self,
        temp_slot: u16,
        entries: &[(PatternKey, HirPattern)],
        builds: &mut RestBuilds,
    ) -> Reg {
        let reloaded = self.fresh_reg();
        self.emit(InstrRef::LoadLocal {
            dst: reloaded,
            slot: temp_slot,
        });
        let rest_reg = self.fresh_reg();
        let keys = excluded_keys(entries.iter().map(|(key, _)| key));
        self.emit(InstrRef::StructRest {
            dst: rest_reg,
            src: reloaded,
            exclude_keys: ConstList::new(&keys),
        });
        let at = builds.next();
        self.park_rest_collection_at(at, rest_reg)
    }
}
