// audited: 2026-09-19
//! Lowering a spliced call: the args array the convention builds, consumes,
//! and reclaims, and the retain its native fall-through owes.
//!
//! docs/impl/region/mechanism.md

use super::*;

impl<'a> Lowerer<'a> {
    /// The splice path: lower every argument into one `@array`, then call
    /// through it. `lower_call` routes here whenever any argument is spliced.
    pub(super) fn lower_spliced_call(
        &mut self,
        func: &Hir,
        args: &[CallArg],
        is_tail: bool,
    ) -> Result<Reg, String> {
        // Lower all args first
        let mut lowered: Vec<(Reg, bool)> = Vec::new();
        for arg in args {
            let reg = self.lower_expr(&arg.expr)?;
            lowered.push((reg, arg.spliced));
        }
        // Callee: a self-reference lowers to `LoadSelf` (self-call re-dispatch),
        // as in the non-splice path.
        let func_reg = self.lower_expr(func)?;

        // The args array is the operand stack spelled as a heap value: the
        // calling convention builds it, the call consumes it, and no binding
        // of the program ever names it. So it takes a managed slot of its own
        // rather than the call's — the call maps its own mint over that slot,
        // and a static slot names one allocation execution between drops — and
        // the release is the runtime's, carried on the call instruction
        // (docs/impl/region/mechanism.md § "A spliced call's arguments come out
        // of an array the convention owns").
        let args_region = self.fresh_managed_region();
        // A frame abandoned between the array's construction and the call —
        // an `ArrayMutExtend` over a source that is not a sequence raises
        // there — still has the slot mapped, so the walk reclaims it off this
        // record (§ "An abandoned frame runs the releases it still owes"). A
        // frame that reached the call does not: the call took the slot, and
        // the walk's own take then finds nothing. The slot is minted per call
        // site, so it can enter the table only once.
        self.current_func.frame_release_regions.push(args_region);

        // Build the args array incrementally
        // Start with MakeArrayMut of the first run of non-spliced args
        let mut args_reg: Option<Reg> = None;

        for (reg, spliced) in &lowered {
            match (args_reg, spliced) {
                (None, false) => {
                    // First arg, not spliced: create array with one element
                    let dst = self.fresh_reg();
                    self.emit_alloc_with_slot(args_region, |region| LirInstr::MakeArrayMut {
                        region,
                        dst,
                        elements: vec![*reg],
                    });
                    args_reg = Some(dst);
                }
                (None, true) => {
                    // First arg, spliced: create empty array, then extend
                    let empty = self.fresh_reg();
                    self.emit_alloc_with_slot(args_region, |region| LirInstr::MakeArrayMut {
                        region,
                        dst: empty,
                        elements: vec![],
                    });
                    let dst = self.fresh_reg();
                    self.emit(LirInstr::ArrayMutExtend {
                        dst,
                        array: empty,
                        source: *reg,
                    });
                    args_reg = Some(dst);
                }
                (Some(arr), false) => {
                    let dst = self.fresh_reg();
                    self.emit(LirInstr::ArrayMutPush {
                        dst,
                        array: arr,
                        value: *reg,
                    });
                    args_reg = Some(dst);
                }
                (Some(arr), true) => {
                    let dst = self.fresh_reg();
                    self.emit(LirInstr::ArrayMutExtend {
                        dst,
                        array: arr,
                        source: *reg,
                    });
                    args_reg = Some(dst);
                }
            }
        }

        let final_args = args_reg.unwrap_or_else(|| {
            let dst = self.fresh_reg();
            self.emit_alloc_with_slot(args_region, |region| LirInstr::MakeArrayMut {
                region,
                dst,
                elements: vec![],
            });
            dst
        });

        if is_tail {
            self.emit_pending_free_regions();
            self.emit_alloc(|region| LirInstr::TailCallArrayMut {
                region,
                func: func_reg,
                args: final_args,
                args_region,
            });
            // A spliced call moves nothing: the array holds one counted
            // reference per element and the callee mints its own, so every
            // release the frame owes past this frame replacement is an
            // ordinary release the relocation carries rather than an
            // ownership move. Hence NO argument expressions in the exempt
            // set — a spliced argument was consumed into the array, and the
            // source the splice read is handed to no one. The callee's own
            // region and this call's result placeholder still are.
            if let Some(call_id) = self.current_hir_id {
                let operands = [final_args, func_reg];
                self.open_tail_exit_hoist(call_id, func, &[], &operands);
            }
            // ReturnValue retain on the native-completion fall-through —
            // the splice/`apply` mirror of the non-splice `TailCall` arm
            // above. A splice tail call to a heap pass-through native
            // (`(first ;argv)`, `(get ;argv)`, an `apply`'d accessor) needs
            // the same retain or its result is freed under the caller's
            // borrow — nothing outlives the call to hold it, the args array
            // being reclaimed by the call itself
            // (region-splice-tail-return.lisp; docs/impl/region/rules.md
            // Rules 4/5). Dead for a frame-replacing closure tail call;
            // no-op for an immediate result. The emitter peeks the operand
            // stack top — the native's pushed result. Stands down under the
            // same one-mint rule as the non-splice arm when ANF named this
            // call's result and a `Return` mints for it.
            let dst = self.fresh_reg();
            if !self.return_mint_covers_here() {
                self.emit(LirInstr::IncrefValueRegion { src: dst });
            }
            Ok(dst)
        } else {
            let dst = self.fresh_reg();
            self.emit_alloc(|region| LirInstr::CallArrayMut {
                region,
                dst,
                func: func_reg,
                args: final_args,
                args_region,
            });
            // ANF binds this Call's result; the binding's slot is
            // recorded in `region_to_slot` by the enclosing
            // `lower_let` / `lower_letrec` / `lower_define`.
            Ok(dst)
        }
    }
}
