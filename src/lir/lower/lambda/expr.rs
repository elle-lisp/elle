//! Closure construction: capture collection, `MakeClosure`, and the
//! capture-adopt region accounting that links captured value regions into the
//! new closure's Owned subtree.

use crate::hir::{CaptureInfo, ParamBound};
use crate::lir::lower::*;
use crate::value::Arity;

impl<'a> Lowerer<'a> {
    /// Lower a lambda expression (creates closure with captures).
    ///
    /// `pub(in crate::lir::lower)` preserves the original `pub(super)` reach:
    /// `super` was `lir::lower` when this lived one level up; the caller
    /// (`lower_expr`) still resolves it from that module.
    #[allow(clippy::too_many_arguments)]
    pub(in crate::lir::lower) fn lower_lambda_expr(
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
        doc: Option<std::rc::Rc<str>>,
        origin: Option<crate::syntax::Span>,
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
                // A self-edge is resolved by `LoadSelf` / a self-call (the executing
                // closure), never by loading this env slot, so the slot is a dead
                // placeholder — emit NIL. Keeping the slot (rather than dropping it and
                // renumbering) leaves every following capture's env index unchanged, so
                // a transitive `Capture { index }` into this closure's env still
                // resolves. NIL mints no heap cell: a binding captured only by
                // self-edges is cell-free. (A binding a *sibling* also captures keeps
                // its cell for that sibling, reached through the binding's own slot,
                // not this self-slot.)
                CaptureKind::Recursive { .. } => {
                    self.emit(LirInstr::Const {
                        dst: reg,
                        value: LirConst::Nil,
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

        // Record the self-recursive binding (if any) this closure is the initializer
        // of: the binding it captures as `CaptureKind::Recursive` (a same-binding
        // self-edge). Such a closure is cell-free, but its region lives through the
        // whole recursion (its self-reference borrows the executing closure), so a
        // recursive tail call strands its scope-end release — `tail_callee_defers_release`
        // reads this set to supply it via the runtime deferred release. Recorded BEFORE lowering
        // the body so a self-tail-call inside the body already sees the fact.
        for cap in captures {
            if let crate::hir::CaptureKind::Recursive { binding } = cap.kind {
                self.self_recursive_bindings.insert(binding);
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
            origin,
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
        self.emit_alloc(|region| LirInstr::MakeClosure {
            region,
            dst,
            closure_id,
            captures: capture_regs,
        });

        // Closure-capture region accounting, two modes per capture:
        //
        // - **Owned (forest)**: a capture whose value region is
        //   an interior member of this closure's Owned subtree (`capture_adopt_edges[lambda]`,
        //   the capture cut) is ADOPTED — emit a value-resolved
        //   `AdoptRegion(closure, captured)` linking the captured value's runtime region into
        //   the closure's subtree, so the closure's free subtree-drops it. The member's own
        //   compiler decref is suppressed in `analyze_regions_with` (it is freed only by the
        //   drop, never both). No `IncrefRegion`: the ownership edge is not reference-counted;
        //   the runtime auto-incref over the `Closure` env (which the cascade would balance) is
        //   absorbed by the frozen-RC no-op on the dropped member.
        //
        // - **Shared (baseline)**: every other cross-region capture keeps the per-region-RC
        //   baseline incref of the binding's scope region. For a genuinely-Shared member it is
        //   balanced by the cascade decref when the closure region frees. For a NON-owner
        //   capture of a member that some OTHER closure adopted (an interior member captured
        //   by two closures of one Owned subtree — only reachable once a future cut claims
        //   such webs), the incref is instead inert: the member is RC-frozen, so this
        //   closure's free-time cascade decref no-ops and the OWNER's subtree drop reclaims
        //   the member regardless of its RC. `capture_adopt_edges` is empty without the flag,
        //   so this is the unchanged baseline path.
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
                // (region/adopt.md § "The capture adopt"): a direct local from its binding
                // slot (`LoadLocal`); an upvalue or transitive capture from the constructing
                // function's environment (`LoadCapture`, or the raw cell load for a
                // cell-held binding — `result_region_of` unwraps the cell either way, so
                // both reloads resolve to the captured VALUE's runtime region, exactly as
                // the local path's cell-through-slot load does).
                enum AdoptReload {
                    Slot(u16),
                    Env { index: u16, raw: bool },
                }
                // Each adopt reload with whether it names a CELL (a re-pointed
                // `closure ⊇ cell` edge → `AdoptCellRegion`, the cell's OWN region) or a
                // by-value capture (`closure ⊇ content` → `AdoptRegion`).
                let mut adopt_loads: Vec<(AdoptReload, bool)> = Vec::new();
                // The member regions actually adopted here — checked against `adopt_edges`
                // below so a suppressed-decref member can never be left un-adopted (a leak).
                let mut adopted_members = Vec::new();
                for cap in captures {
                    // A self-edge is a NIL env placeholder (resolved by the executing
                    // closure), so it names no captured value region — nothing to adopt
                    // or incref.
                    if matches!(cap.kind, crate::hir::CaptureKind::Recursive { .. }) {
                        continue;
                    }
                    // Mirror the capture-collection loop's access paths exactly.
                    let needs_cell = self.arena.get(cap.binding).needs_capture();
                    // The member regions of this capture that are children of an adopt edge
                    // into this closure. For a CELL-materialized capture the edge names the
                    // CELL region (re-pointed by `capture_containment_edges`), not the
                    // value's `binding_source_regions` — the closure holds the cell, and
                    // `AdoptCellRegion` will adopt the cell's own region. For a by-value
                    // capture the edge names the value's region. The edge keys the *static*
                    // region; the emit is value-resolved off the reload.
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
                            self.emit(LirInstr::IncrefRegion { region_id });
                        }
                    }
                }
                // The capture-adopt contract: every `adopt_edges` member had its own decref
                // suppressed by `analyze_regions_with` (reclaimed solely by this closure's
                // subtree drop), so each MUST have been adopted above — suppressed yet never
                // adopted LEAKS. The reloads cover every capture kind, so a miss means the
                // edge names a region no capture of this closure holds (an inference bug).
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
                    // Park the closure in a scratch slot (consuming `dst`), then link each
                    // captured value's runtime region into the closure's subtree with a
                    // value-resolved `AdoptRegion` — both operands loaded FRESH so the
                    // instruction's operand consumption (`ensure_binary_on_top` + two pops)
                    // is harmless. Finally restore the closure into `dst` for the caller
                    // (the same store/load roundtrip `emit_decrefs_for` uses for a discarded
                    // result). `dst` itself cannot be the adopt operand: it is the closure
                    // the caller binds next, and the capture registers were already consumed
                    // by `MakeClosure`.
                    let scratch = self.scratch_slot();
                    self.emit(LirInstr::StoreLocal {
                        slot: scratch,
                        src: dst,
                    });
                    for (reload, is_cell) in adopt_loads {
                        let preg = self.fresh_reg();
                        self.emit(LirInstr::LoadLocal {
                            dst: preg,
                            slot: scratch,
                        });
                        let creg = self.fresh_reg();
                        let trace_load = match reload {
                            AdoptReload::Slot(slot) => {
                                self.emit(LirInstr::LoadLocal { dst: creg, slot });
                                ("slot", slot)
                            }
                            AdoptReload::Env { index, raw: false } => {
                                self.emit(LirInstr::LoadCapture { dst: creg, index });
                                ("env", index)
                            }
                            AdoptReload::Env { index, raw: true } => {
                                self.emit(LirInstr::LoadCaptureRaw { dst: creg, index });
                                ("env-raw", index)
                            }
                        };
                        // A cell capture (`closure ⊇ cell`) adopts the cell's OWN region
                        // via `AdoptCellRegion` (`region_of`, not the unwrapped content —
                        // the reload gives the raw cell); a by-value capture adopts the
                        // value's region via `AdoptRegion`.
                        if is_cell {
                            self.emit(LirInstr::AdoptCellRegion {
                                parent: preg,
                                child: creg,
                            });
                        } else {
                            self.emit(LirInstr::AdoptRegion {
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
                    self.emit(LirInstr::LoadLocal { dst, slot: scratch });
                }
            }
        }

        Ok(dst)
    }
}
