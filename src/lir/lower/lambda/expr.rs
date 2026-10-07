// audited: 2026-10-06
//! Closure construction: capture collection, `MakeClosure`, the capture adopts, and the lambda's rest-list layout.
//!
//! src/lir/lower/AGENTS.md
//! docs/impl/region/adopt.md
//! docs/impl/region/restlist.md
//!
//! A captured value region that is an interior member of the new closure's
//! Owned subtree is adopted into it; every other capture keeps the baseline
//! incref.

use crate::hir::Lambda;
use crate::lir::lower::*;

impl<'a> Lowerer<'a> {
    /// Lower a lambda expression to a `MakeClosure` over its captures, and
    /// return the register that holds the closure.
    pub(in crate::lir::lower) fn lower_lambda_expr(
        &mut self,
        lambda: &Lambda,
    ) -> Result<Reg, String> {
        // `lower_expr` set this to the Lambda's own id before dispatching here.
        let lambda_id = self.current_hir_id;

        // Collect capture registers
        let mut capture_regs = Vec::new();
        for cap in &lambda.captures {
            use crate::hir::CaptureKind;

            let reg = self.fresh_reg();

            // A binding that needs a capture cell is captured as the cell itself,
            // not the value the cell holds.
            let binding_needs_capture = self.arena.get(cap.binding).needs_capture();

            match cap.kind {
                // `LoadSelf` or a self-call resolves a self-edge to the executing
                // closure, and never loads this env slot, so the slot holds NIL.
                // Keeping the slot leaves every following capture's env index
                // unchanged, so a transitive `Capture { index }` into this closure's
                // env still resolves. NIL mints no heap cell, so a binding captured
                // only by self-edges is cell-free. A binding a sibling also captures
                // keeps its cell for that sibling, reached through the binding's own
                // slot, not this one.
                CaptureKind::Recursive { .. } => {
                    self.emit(InstrRef::Const {
                        dst: reg,
                        value: ConstRef::Nil,
                    });
                    capture_regs.push(reg);
                }
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
                                self.emit(InstrRef::LoadCaptureRaw {
                                    dst: reg,
                                    index: slot,
                                });
                            } else {
                                self.emit(InstrRef::LoadCapture {
                                    dst: reg,
                                    index: slot,
                                });
                            }
                        } else {
                            // Local variables (including those defined inside lambda) use LoadLocal
                            self.emit(InstrRef::LoadLocal { dst: reg, slot });
                        }
                    } else {
                        // The analyzer captured a binding this function never slotted.
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
                            self.emit(InstrRef::LoadCaptureRaw { dst: reg, index });
                        } else {
                            self.emit(InstrRef::LoadCapture { dst: reg, index });
                        }
                    } else {
                        // We're in the main function - this shouldn't happen
                        // (main function doesn't have captures to forward)
                        self.emit(InstrRef::LoadLocal {
                            dst: reg,
                            slot: index,
                        });
                    }
                    capture_regs.push(reg);
                }
            }
        }

        // Record the binding this closure initializes when it captures that binding
        // as `CaptureKind::Recursive`. Such a closure is cell-free, but its region
        // lives through the whole recursion, so a recursive tail call strands its
        // scope-end release. `lower_letrec` and `lower_define` read this set to mark
        // the stranded bindings whose release `tail_callee_defers_release` defers.
        // The set is written before the body lowers, so a self-tail-call inside the
        // body already sees it.
        for cap in &lambda.captures {
            if let crate::hir::CaptureKind::Recursive { binding } = cap.kind {
                self.self_recursive_bindings.insert(binding);
            }
        }

        // Reserve a slot in the module's closure list before lowering the body.
        // This numbers closures in pre-order: a parent's ID is lower than its
        // children's.
        let closure_id = ClosureId(self.closures.len() as u32);
        self.closures.push(None);

        // The gate's verdict on how this lambda builds its rest list
        // (docs/impl/region/restlist.md); a lambda it never judged keeps one
        // region per cell.
        let rest_list_layout = lambda_id.map_or(crate::value::RestListLayout::PerCell, |id| {
            self.region_info.rest_list_layout(id)
        });

        // Lower the lambda body — children get higher IDs
        let nested = self.lower_lambda_body(closure_id, lambda, rest_list_layout)?;

        // Check numeric! assertion after lowering. Eligibility is a question
        // the frozen form answers.
        if lambda.assert_numeric && !nested.view().is_gpu_eligible() {
            return Err("numeric! assertion failed: function is not GPU-eligible".to_string());
        }

        // Fill the reserved slot
        self.closures[closure_id.0 as usize] = Some(nested);

        // Create closure referencing it by ID
        let dst = self.fresh_reg();
        self.emit_alloc(|region| InstrRef::MakeClosure {
            region,
            dst,
            closure_id,
            captures: &capture_regs,
        });

        // Each capture takes one of two region accountings:
        //
        // - A capture whose region is a member of this closure's Owned subtree
        //   (`capture_adopt_edges` at this lambda) is adopted: a value-resolved
        //   adopt links the captured value's runtime region into the closure's
        //   subtree, and no `IncrefRegion` is emitted. The ownership passes
        //   (`apply_ownership`) suppressed the member's own release, so the
        //   closure's subtree drop is its one reclamation.
        //
        // - **Shared (baseline)**: every other cross-region capture keeps the per-region-RC
        //   baseline incref of the binding's scope region. For a genuinely-Shared member it is
        //   balanced by the cascade decref when the closure region frees. For a NON-owner
        //   capture of a member that some OTHER closure adopted (an interior member captured
        //   by two closures of one Owned subtree), the incref is instead inert: the member is
        //   RC-frozen, so this closure's free-time cascade decref no-ops and the OWNER's
        //   subtree drop reclaims the member regardless of its RC. A closure with no adopt
        //   edge takes this path for every capture.
        if let Some(hir_id) = self.current_hir_id {
            if let Some(&closure_region) = self.region_info.alloc_region.get(&hir_id) {
                let adopt_edges = self
                    .region_info
                    .capture_adopt_edges
                    .get(&hir_id)
                    .cloned()
                    .unwrap_or_default();
                // How to RELOAD each adopted captured value for the value-resolved adopt,
                // by the capture's access path — the emit covers EVERY capture kind
                // (docs/impl/region/adopt.md): a direct local from its binding
                // slot (`LoadLocal`); an upvalue or transitive capture from the constructing
                // function's environment (`LoadCapture`, or the raw cell load for a
                // cell-held binding — `result_region_of` unwraps the cell either way, so
                // both reloads resolve to the captured VALUE's runtime region, exactly as
                // the local path's cell-through-slot load does).
                enum AdoptReload {
                    Slot(u16),
                    Env { index: u16, raw: bool },
                }
                // Each adopt reload, and whether it names a cell. A cell capture is a
                // `closure ⊇ cell` edge, adopted with `AdoptCellRegion`. A by-value
                // capture is a `closure ⊇ content` edge, adopted with `AdoptRegion`.
                let mut adopt_loads: Vec<(AdoptReload, bool)> = Vec::new();
                // The member regions adopted here. The check below compares them
                // with `adopt_edges`, so no member whose release was suppressed is
                // left unadopted, which would leak it.
                let mut adopted_members = Vec::new();
                for cap in &lambda.captures {
                    // A self-edge's env slot holds NIL, so it names no captured
                    // region, and there is nothing to adopt or retain.
                    if matches!(cap.kind, crate::hir::CaptureKind::Recursive { .. }) {
                        continue;
                    }
                    // Mirror the capture-collection loop's access paths exactly.
                    let needs_cell = self.arena.get(cap.binding).needs_capture();
                    // The member regions of this capture that an adopt edge links into
                    // this closure. For a capture held in a cell, the edge names the cell's
                    // region (`capture_containment_edges` points it there), because the
                    // closure holds the cell. For a by-value capture, the edge names the
                    // value's region. The edge names the static region; the emitted adopt
                    // reads the runtime region off the reloaded value.
                    let candidate_regions: Vec<crate::hir::region::Region> = if needs_cell {
                        self.cell_region_of_binding(cap.binding)
                            .into_iter()
                            .collect()
                    } else {
                        self.region_info
                            .binding_source_regions
                            .get(&cap.binding)
                            .cloned()
                            .unwrap_or_default()
                    };
                    let matched = candidate_regions
                        .iter()
                        .copied()
                        .filter(|r| adopt_edges.contains(&(*r, closure_region)))
                        .collect::<Vec<_>>();
                    if !matched.is_empty() {
                        let reload = match cap.kind {
                            crate::hir::CaptureKind::Local => {
                                self.binding_to_slot.get(&cap.binding).copied().map(|slot| {
                                    if self.in_lambda
                                        && self.upvalue_bindings.contains(&cap.binding)
                                    {
                                        AdoptReload::Env {
                                            index: slot,
                                            raw: needs_cell,
                                        }
                                    } else {
                                        AdoptReload::Slot(slot)
                                    }
                                })
                            }
                            crate::hir::CaptureKind::Capture { index } => Some(if self.in_lambda {
                                AdoptReload::Env {
                                    index,
                                    raw: needs_cell,
                                }
                            } else {
                                AdoptReload::Slot(index)
                            }),
                            crate::hir::CaptureKind::Recursive { .. } => None,
                        };
                        if let Some(reload) = reload {
                            adopt_loads.push((reload, needs_cell));
                            adopted_members.extend(matched);
                            continue;
                        }
                    }
                    // Baseline incref for every other cross-region capture.
                    if let Some(&cap_region) = self.region_info.binding_region.get(&cap.binding) {
                        if cap_region != closure_region {
                            let region_id = self.static_slot(cap_region);
                            self.emit(InstrRef::IncrefRegion { region_id });
                        }
                    }
                }
                // Every `adopt_edges` member had its own release suppressed by
                // `apply_ownership`, so each must have been adopted above, or it
                // leaks. The reloads cover every capture kind, so a miss means the
                // edge names a region no capture of this closure holds.
                debug_assert!(
                    adopt_edges
                        .iter()
                        .all(|(member, _)| adopted_members.contains(member)),
                    "capture-adopt contract violated: closure region {} has adopt edges {:?} \
                     but only adopted members {:?} — a suppressed-decref member was not \
                     adopted (a leak). Every adopt edge must name a region held by one of \
                     this closure's captures.",
                    closure_region.0,
                    adopt_edges,
                    adopted_members,
                );
                if !adopt_loads.is_empty() {
                    // Park the closure in the scratch slot, then emit one adopt per
                    // captured value. Each adopt consumes both of its operands, so both
                    // are loaded fresh. `dst` cannot be an operand, because the caller
                    // binds it next, and `MakeClosure` already consumed the capture
                    // registers. Last, reload the closure into `dst` for the caller.
                    let scratch = self.scratch_slot();
                    self.emit(InstrRef::StoreLocal {
                        slot: scratch,
                        src: dst,
                    });
                    for (reload, is_cell) in adopt_loads {
                        let preg = self.fresh_reg();
                        self.emit(InstrRef::LoadLocal {
                            dst: preg,
                            slot: scratch,
                        });
                        let creg = self.fresh_reg();
                        let trace_load = match reload {
                            AdoptReload::Slot(slot) => {
                                self.emit(InstrRef::LoadLocal { dst: creg, slot });
                                ("slot", slot)
                            }
                            AdoptReload::Env { index, raw: false } => {
                                self.emit(InstrRef::LoadCapture { dst: creg, index });
                                ("env", index)
                            }
                            AdoptReload::Env { index, raw: true } => {
                                self.emit(InstrRef::LoadCaptureRaw { dst: creg, index });
                                ("env-raw", index)
                            }
                        };
                        // `AdoptCellRegion` adopts the cell's own region, not its
                        // content's, since the reload gives the raw cell. `AdoptRegion`
                        // adopts a by-value capture's region.
                        if is_cell {
                            self.emit(InstrRef::AdoptCellRegion {
                                parent: preg,
                                child: creg,
                            });
                        } else {
                            self.emit(InstrRef::AdoptRegion {
                                parent: preg,
                                child: creg,
                            });
                        }
                        if crate::config::get().has_trace("rc") {
                            eprintln!(
                                "[trace:rc:emit] {} capture closure_region={} {}={}",
                                if is_cell {
                                    "adopt_cell_region"
                                } else {
                                    "adopt_region"
                                },
                                closure_region.0,
                                trace_load.0,
                                trace_load.1
                            );
                        }
                    }
                    self.emit(InstrRef::LoadLocal { dst, slot: scratch });
                }
            }
        }

        Ok(dst)
    }
}
