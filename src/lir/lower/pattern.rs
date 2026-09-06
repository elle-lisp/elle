// audited: 2026-09-06
// src/lir/lower/AGENTS.md
// docs/match.md
//! Lowering a compiled decision tree to blocks: bindings, guards, arm bodies,
//! and the switch that chooses between them.
//!
//! `ctor` holds the constructor tests a switch branches on; `matching`, `seq`
//! and `keyed` lower a pattern directly, for the binding forms that have no tree.

use super::*;
use crate::hir::decision::{AccessPath, Constructor, DecisionTree};
use crate::hir::{HirPattern, PatternKey, PatternLiteral};

mod ctor;
mod keyed;
mod matching;
mod seq;

/// Does an access path reach a binding through a BORROWED structural element
/// load — `First`/`Rest`/`Index`/`Key`? The match decision tree loads these
/// with intrinsics that carry NO owning reference: the region solver only
/// registers a counted container read for *call-site* `rest()`/`first()`/`get()`,
/// never for pattern loads. A binding reached this way is a BORROWED subview of
/// the scrutinee — passing it as an owned-param call argument lets
/// the callee's release free the caller's still-live scrutinee region.
///
/// `Slice` and `StructRest` (array/struct `& rest` patterns) are excluded: they
/// mint a FRESH OWNED container (`vm/data.rs::handle_array_slice_from`), so a
/// path through them is not a borrow of the scrutinee — a binding reached under
/// one owns its new container, and the charged cascade frees it.
pub(super) fn access_is_borrowed_element(access: &AccessPath) -> bool {
    match access {
        AccessPath::Root => false,
        AccessPath::First(_inner) | AccessPath::Rest(_inner) => true,
        AccessPath::Index(_inner, _) | AccessPath::Key(_inner, _) => true,
        // Slice/StructRest mint a fresh owned container — a path through
        // them (and whatever element loads sit beneath them) is not a
        // borrow of the scrutinee.
        AccessPath::Slice(..) | AccessPath::StructRest(..) => false,
    }
}

impl<'a> Lowerer<'a> {
    // ── Decision tree lowering ─────────────────────────────────────

    /// Emit the no-match path: raise :match-error carrying the scrutinee.
    /// The store and jump after MatchFail are NOT dead: errors are
    /// resumable, and a fiber that catches SIG_ERROR resumes here with
    /// the handler's value pushed — the store makes that value the
    /// match expression's result (same convention as the destructure
    /// instructions).
    pub(super) fn emit_no_match(
        &mut self,
        scrutinee_slot: u16,
        result_slot: u16,
        done_label: Label,
    ) -> Result<(), String> {
        let scrut = self.fresh_reg();
        self.emit(LirInstr::LoadLocal {
            dst: scrut,
            slot: scrutinee_slot,
        });
        let dst = self.fresh_reg();
        self.emit(LirInstr::MatchFail { dst, src: scrut });
        self.emit(LirInstr::StoreLocal {
            slot: result_slot,
            src: dst,
        });
        self.terminate(Terminator::Jump(done_label));
        self.finish_block();
        Ok(())
    }

    /// Lower a compiled decision tree to LIR instructions.
    ///
    /// Walks the tree recursively, emitting constructor tests, bindings,
    /// guard checks, and arm bodies. Each tree node becomes one or more
    /// basic blocks.
    ///
    /// The scrutinee and result live in local slots (not on the operand
    /// stack).  The emitter pre-allocates space for all locals at the
    /// start of the entry block, so StoreLocal never clobbers operand
    /// values from enclosing expressions.
    pub(super) fn lower_decision_tree(
        &mut self,
        tree: &DecisionTree,
        arms: &[(HirPattern, Option<Hir>, Hir)],
        scrutinee_slot: u16,
        result_slot: u16,
        done_label: Label,
        lowered_arms: &mut std::collections::HashMap<usize, Label>,
    ) -> Result<(), String> {
        match tree {
            DecisionTree::Fail => self.emit_no_match(scrutinee_slot, result_slot, done_label),
            DecisionTree::Leaf {
                arm_index,
                bindings,
            } => {
                // Establish bindings by loading values at their access paths.
                // Pop after each store — the value lives in the slot/capture
                // and keeping it on the operand stack would leak intermediates.
                for (binding, access) in bindings {
                    let val_reg = self.load_access_path(access, scrutinee_slot)?;
                    // A borrowed subview of the scrutinee — mark it
                    // (see `destructure_alias_bindings`).
                    if access_is_borrowed_element(access) {
                        self.destructure_alias_bindings.insert(*binding);
                    }
                    let slot = if let Some(&existing) = self.binding_to_slot.get(binding) {
                        existing
                    } else {
                        self.allocate_slot(*binding)
                    };
                    let needs_capture = self.arena.get(*binding).needs_capture();
                    if self.in_lambda && needs_capture {
                        self.upvalue_bindings.insert(*binding);
                        self.emit(LirInstr::StoreCapture {
                            index: slot,
                            src: val_reg,
                        });
                    } else {
                        self.emit(LirInstr::StoreLocal { slot, src: val_reg });
                    }
                }

                // If this arm's body was already lowered (e.g., multiple cases
                // in an or-pattern reaching the same arm), jump to the existing
                // body instead of re-lowering it.  Re-lowering would share
                // binding slots but only initialize cells (MakeCapture) in the
                // first copy, causing "Expected cell, got ..." panics when a
                // later copy runs at runtime.
                if let Some(&body_label) = lowered_arms.get(arm_index) {
                    self.terminate(Terminator::Jump(body_label));
                    self.finish_block();
                    return Ok(());
                }

                // First time lowering this arm — record its label for reuse.
                let body_label = self.fresh_label();
                lowered_arms.insert(*arm_index, body_label);
                self.terminate(Terminator::Jump(body_label));
                self.finish_block();
                self.current_block = BasicBlock::new(body_label);

                // Lower body
                let body = &arms[*arm_index].2;
                let body_reg = self.lower_expr(body)?;
                self.emit(LirInstr::StoreLocal {
                    slot: result_slot,
                    src: body_reg,
                });
                self.terminate(Terminator::Jump(done_label));
                // This arm's relocation point, sealed for the done block to
                // inherit (docs/impl/region/mechanism.md § "The relocation point
                // outlives the block"). An or-pattern's later cases jump to this
                // same body, so the point is sealed once, with the body.
                self.seal_arm_hoists();
                self.finish_block();
                Ok(())
            }
            DecisionTree::Guard {
                arm_index,
                bindings,
                otherwise,
            } => {
                // Establish bindings — pop after each store (same as Leaf).
                for (binding, access) in bindings {
                    let val_reg = self.load_access_path(access, scrutinee_slot)?;
                    // A borrowed subview of the scrutinee — mark it
                    // (see `destructure_alias_bindings`).
                    if access_is_borrowed_element(access) {
                        self.destructure_alias_bindings.insert(*binding);
                    }
                    let slot = if let Some(&existing) = self.binding_to_slot.get(binding) {
                        existing
                    } else {
                        self.allocate_slot(*binding)
                    };
                    let needs_capture = self.arena.get(*binding).needs_capture();
                    if self.in_lambda && needs_capture {
                        self.upvalue_bindings.insert(*binding);
                        self.emit(LirInstr::StoreCapture {
                            index: slot,
                            src: val_reg,
                        });
                    } else {
                        self.emit(LirInstr::StoreLocal { slot, src: val_reg });
                    }
                }
                // Evaluate guard
                let guard_expr = arms[*arm_index]
                    .1
                    .as_ref()
                    .expect("Guard node must have guard expression");
                let guard_reg = self.lower_expr(guard_expr)?;

                let pass_label = self.fresh_label();
                let fail_label = self.fresh_label();
                self.terminate(Terminator::Branch {
                    cond: guard_reg,
                    then_label: pass_label,
                    else_label: fail_label,
                });
                self.finish_block();

                // Guard passed: lower body
                self.current_block = BasicBlock::new(pass_label);
                let body = &arms[*arm_index].2;
                let body_reg = self.lower_expr(body)?;
                self.emit(LirInstr::StoreLocal {
                    slot: result_slot,
                    src: body_reg,
                });
                self.terminate(Terminator::Jump(done_label));
                self.seal_arm_hoists();
                self.finish_block();

                // Guard failed: continue with otherwise
                self.current_block = BasicBlock::new(fail_label);
                self.lower_decision_tree(
                    otherwise,
                    arms,
                    scrutinee_slot,
                    result_slot,
                    done_label,
                    lowered_arms,
                )
            }
            DecisionTree::Switch {
                access,
                cases,
                default,
            } => {
                // Load value at access path, store to temp slot, then pop
                // from the operand stack.  The value lives in the local
                // slot and is reloaded via LoadLocal for each constructor
                // test.
                let value_reg = self.load_access_path(access, scrutinee_slot)?;
                let temp_slot = self.current_func.num_locals;
                self.current_func.num_locals += 1;
                self.emit(LirInstr::StoreLocal {
                    slot: temp_slot,
                    src: value_reg,
                });

                let default_label = self.fresh_label();

                // Emit if-else chain for each constructor
                for (i, (ctor, subtree)) in cases.iter().enumerate() {
                    let match_label = self.fresh_label();
                    let next_label = if i + 1 < cases.len() {
                        self.fresh_label()
                    } else {
                        default_label
                    };

                    // Reload value for this test
                    let reloaded = self.fresh_reg();
                    self.emit(LirInstr::LoadLocal {
                        dst: reloaded,
                        slot: temp_slot,
                    });

                    // Emit constructor test (may create blocks for Array/@array)
                    let test_reg = self.emit_constructor_test(reloaded, ctor)?;
                    self.terminate(Terminator::Branch {
                        cond: test_reg,
                        then_label: match_label,
                        else_label: next_label,
                    });
                    self.finish_block();

                    // Match block: recurse into subtree
                    self.current_block = BasicBlock::new(match_label);
                    self.lower_decision_tree(
                        subtree,
                        arms,
                        scrutinee_slot,
                        result_slot,
                        done_label,
                        lowered_arms,
                    )?;

                    // Start next test block (if not the last case)
                    if i + 1 < cases.len() {
                        self.current_block = BasicBlock::new(next_label);
                    }
                }

                // Default block
                self.current_block = BasicBlock::new(default_label);
                if let Some(def) = default {
                    self.lower_decision_tree(
                        def,
                        arms,
                        scrutinee_slot,
                        result_slot,
                        done_label,
                        lowered_arms,
                    )?;
                } else {
                    // No default → no constructor matched the scrutinee
                    self.emit_no_match(scrutinee_slot, result_slot, done_label)?;
                }

                Ok(())
            }
        }
    }
}
