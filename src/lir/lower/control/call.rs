// audited: 2026-09-19
//! Lowering a call: the argument loop that decides what each argument owes,
//! and the dispatch to the tail, spliced and ordinary arms.
//!
//! docs/impl/region/rules.md

use super::*;

mod defer;
mod splice;
mod tail;

/// Does this HIR subtree lower a **loop** (a back-edge) in the CURRENT function —
/// a `While`/`Loop` not inside a nested lambda?
///
/// The bytecode VM is stack-based: a call parks each already-lowered argument on
/// the operand stack while it lowers the next. A loop, on re-entering its head,
/// resets the operand stack to its head-block layout — which does not include an
/// earlier argument value parked *below* the loop's working set, so that value is
/// dropped and the call reads whatever sits in its slot instead (a phantom-arity
/// or wrong-value corruption at the call). `if`/`begin`/`match` are forward merges
/// that carry the full stack across, so only a back-edge loop triggers it. When a
/// later argument contains one, `lower_call` spills every argument to a local as
/// it is lowered and reloads them adjacent to the call.
///
/// Nested lambda bodies are separate lowered functions, so their loops never touch
/// this function's operand stack and are not counted (stop at the `Lambda`).
fn hir_contains_loop(h: &Hir) -> bool {
    match &h.kind {
        HirKind::While { .. } | HirKind::Loop { .. } => true,
        HirKind::Lambda { .. } => false,
        _ => {
            let mut found = false;
            h.for_each_child(|c| found |= hir_contains_loop(c));
            found
        }
    }
}

impl<'a> Lowerer<'a> {
    /// Does a `Return` mint already cover the tail call being lowered? True for
    /// ANF's canonical wrap `(let [t (f …)] (return t))`, recorded by `lower_let`
    /// — there the frame names the result and `lower_return`'s mint plus the
    /// binding's `decref_point` carry the whole return convention, so the
    /// post-`TailCall` fall-through retain would be a second, unbalanced
    /// reference (docs/impl/region/mechanism.md § "The return mint is emitted
    /// exactly once"; pinned by `region-native-tail-compound-leak.lisp`).
    fn return_mint_covers_here(&self) -> bool {
        self.current_hir_id
            .is_some_and(|id| self.return_minted_calls.contains(&id))
    }
    pub(in crate::lir::lower) fn lower_call(
        &mut self,
        func: &Hir,
        args: &[CallArg],
        is_tail: bool,
        call_signals: SignalBits,
    ) -> Result<Reg, String> {
        let has_splice = args.iter().any(|a| a.spliced);

        if has_splice {
            return self.lower_spliced_call(func, args, is_tail);
        }

        // === Common path: no spliced args ===
        // Check for intrinsic specialization
        let plain_args: Vec<&Hir> = args.iter().map(|a| &a.expr).collect();
        if let Some(result) = self.try_lower_intrinsic(func, &plain_args)? {
            return Ok(result);
        }

        let mut arg_regs = Vec::new();
        // Stash slots for the borrowed args' retained values: the retain is
        // consumed by the callee's owned-param release only when the callee
        // is a frame-replacing CLOSURE. A NATIVE tail callee borrows its
        // args and releases nothing, so the call falls through to the
        // post-`TailCall` block — which must consume the retain itself
        // (below) or every borrowed arg pins its region's rc by one per
        // call, an unbounded over-keep
        // (region-const-tail-move-borrow-uaf.lisp, witness (c)). The slot
        // stash keeps the RETAINED value addressable there: re-lowering the
        // arg instead would re-READ a cell a callee-run closure (`apply`)
        // may have reassigned, releasing the wrong value.
        let mut borrowed_arg_slots = Vec::new();
        // If a LATER argument lowers a loop, its back-edge resets the operand
        // stack and drops any earlier argument value parked there (see
        // `hir_contains_loop`). Park every argument in a local as it is lowered
        // and reload them adjacent to the call, so no argument value survives on
        // the operand stack across the loop.
        let spill_across_loop =
            args.len() >= 2 && args.iter().skip(1).any(|a| hir_contains_loop(&a.expr));
        let mut arg_spill_slots: Vec<Option<u16>> = Vec::new();
        // Regions an earlier argument of this tail call already MOVED. The
        // frame holds one reference per region and the callee releases once
        // per owned parameter, so only the first occurrence is funded by the
        // move; a later one must be minted exactly as a borrowed argument is
        // (docs/impl/region/rules.md Rule 5).
        let mut moved_arg_regions: rustc_hash::FxHashSet<crate::hir::region::Region> =
            rustc_hash::FxHashSet::default();
        // An argument past the callee's fixed parameters is collected into the
        // rest parameter's fresh list instead of becoming an owned param, and
        // that list's own allocation scan and surplus release balance it
        // without a mint — so a repeat landing there must NOT take one. Only a
        // callee this compilation resolved says so; anything else keeps the
        // mint, which is the leak-preserving direction.
        let rest_from = self
            .current_hir_id
            .and_then(|id| self.region_info.tail_callee_facts.get(&id))
            .map(|f| f.fixed_params);
        // A dynamic `emit` whose payload this body releases nowhere. The park
        // owes the body one reference of every value it yields, and the shape
        // that carries it depends on position (docs/impl/region/park.md
        // § "What yields is the emit OPERATION, not the `Emit` node"): a TAIL
        // call already mints one for a borrowed argument, which the suspending
        // exit leaves standing and the replayed fall-through releases, so only
        // a non-tail call owes a mint of its own. Routed through `borrowed`
        // below, whose retain / private stash / post-call release is the exact
        // shape `lower_emit` uses at the terminator.
        let emit_payload_arg = (!is_tail
            && self
                .current_hir_id
                .is_some_and(|id| self.region_info.borrowed_emit_payloads.contains(&id)))
        .then_some(crate::hir::region::EMIT_PAYLOAD_ARG);
        for (index, arg) in args.iter().enumerate() {
            // For a tail call, a BORROWED arg must be handed the callee a
            // fresh owning reference (see the move-on-tail-call comment at
            // the `is_tail` block below for the ownership argument). The
            // retain must ORDER BEFORE the arg node's own `emit_decrefs_for`:
            // when the borrowed arg is a `@`-mutable param read, that node's
            // last-use release is a `DecrefCellRegion` of the param's OWN cell,
            // whose cascade frees the cell's contents — the very value being
            // moved. A retain emitted after it reads a freed page (the reassigned
            // mutable-param double-release UAF, region-mutable-reassign-param.lisp).
            // So defer this node's decrefs, emit the retain, then emit the
            // deferred decrefs — the retain now precedes the cell's cascade-free.
            //
            // A REPEATED owned argument takes the same treatment for a
            // different reason: the move handed over one reference and this
            // occurrence is a second owned parameter to release. Only an owned
            // occupant of a fixed parameter both consumes the move and can be
            // starved by an earlier one — a borrowed argument mints for itself,
            // and a rest position is balanced by the collected list.
            let mut leaf_regions = rustc_hash::FxHashSet::default();
            if is_tail {
                self.arg_leaf_regions(&arg.expr, &mut leaf_regions);
            }
            let borrowed_leaf = is_tail && self.tail_arg_is_borrowed(&arg.expr);
            let takes_the_move = is_tail && !borrowed_leaf && rest_from.is_none_or(|n| index < n);
            let repeated =
                takes_the_move && leaf_regions.iter().any(|r| moved_arg_regions.contains(r));
            if takes_the_move && !repeated {
                moved_arg_regions.extend(leaf_regions);
            }
            let borrowed = borrowed_leaf || repeated || emit_payload_arg == Some(index);
            if borrowed {
                self.deferred_decref_points.insert(arg.expr.id);
            }
            let mut reg = self.lower_expr(&arg.expr)?;
            // Emit the retain HERE, while this arg's value is on top of the
            // operand stack — the emitter's `IncrefValueRegion` peeks the
            // stack top, so deferring it until after the remaining args and
            // the func are pushed would force `ensure_on_top` to `DupN` the
            // value up, orphaning it and corrupting the tail call's argument
            // layout. The single-arg case tolerated the late incref; a
            // multi-arg tail call (e.g. a `(struct :k v …)` whose values are
            // cell-backed upvalues, as stdlib's export struct is) does not.
            // `IncrefValueRegion` does not pop, so the arg stays in place for
            // the rest of the arg/func pushes and the `TailCall`.
            if borrowed {
                self.emit(LirInstr::IncrefValueRegion { src: reg });
                // Stash-and-reload: `StoreLocal` consumes the value (the
                // emitter auto-pops), so reload it as the arg actually
                // handed to the call — same value, layout intact.
                let slot = self.current_func.num_locals;
                self.current_func.num_locals += 1;
                self.emit(LirInstr::StoreLocal { slot, src: reg });
                let reloaded = self.fresh_reg();
                self.emit(LirInstr::LoadLocal {
                    dst: reloaded,
                    slot,
                });
                reg = reloaded;
                borrowed_arg_slots.push(slot);
                self.deferred_decref_points.remove(&arg.expr.id);
                self.emit_decrefs_for(arg.expr.id, Some(reg));
            }
            // Park this argument off the operand stack (a `StoreLocal` auto-pops
            // it) so a later argument's loop cannot clobber it; the value flows
            // through the local unchanged, immediates included.
            if spill_across_loop {
                let slot = self.current_func.num_locals;
                self.current_func.num_locals += 1;
                self.emit(LirInstr::StoreLocal { slot, src: reg });
                arg_spill_slots.push(Some(slot));
            } else {
                arg_spill_slots.push(None);
            }
            arg_regs.push(reg);
        }
        // Reload the parked arguments in order, so they sit on the stack below
        // the callee — the `[arg0..argN, func]` layout `Call` expects — freshly
        // loaded past the loop that would otherwise have clobbered them.
        for (i, slot) in arg_spill_slots.iter().enumerate() {
            if let Some(slot) = *slot {
                let reloaded = self.fresh_reg();
                self.emit(LirInstr::LoadLocal {
                    dst: reloaded,
                    slot,
                });
                arg_regs[i] = reloaded;
            }
        }
        // Lower the callee. A self-reference here lowers to `LoadSelf` (the
        // executing closure) exactly as in value position, so the call re-enters
        // the current code+env — the self-call re-dispatch (`lower_var`).
        let func_reg = self.lower_expr(func)?;

        // Determine if the compiler verified arity for this call.
        // True when the callee is a primitive binding that hasn't been
        // shadowed or mutated (the analyzer would have errored on arity
        // mismatch, so if compilation succeeded the arity is correct).
        let arity_checked = if let HirKind::Var(binding) = &func.kind {
            let bi = self.arena.get(*binding);
            bi.is_primitive
                && bi.is_immutable
                && !bi.is_mutated
                && self
                    .immutable_values
                    .get(binding)
                    .is_some_and(|v| v.is_native_fn())
        } else {
            false
        };

        if is_tail {
            return self.lower_tail_call(
                func,
                args,
                arg_regs,
                func_reg,
                arity_checked,
                borrowed_arg_slots,
            );
        }

        let dst = self.fresh_reg();
        // A call needs the CPS suspending convention (a resumable
        // continuation) if the callee can SUSPEND and later RESUME —
        // every signal the fiber scheduler parks on and wakes: a plain
        // yield, an io request, and a structured-concurrency wait. SIG_IO
        // and SIG_WAIT matter on their own because signal narrowing can
        // resolve an `(emit :io …)` / `(emit :wait …)` to just that bit,
        // dropping the SIG_YIELD `emit`'s static signal carries — so a
        // wrapper like `emit-wait` / `ev/join`, whose narrowed signal is
        // SIG_WAIT alone, would otherwise compile to a plain Call with no
        // continuation frame, and the code after the wait would be lost on
        // resume (the whole async scheduler's `handle-wait` path). SIG_ERROR
        // / SIG_HALT are excluded: they unwind or terminate, never resume.
        // Pinned by tests/elle/wasm-wait-call-resumes.lisp.
        if call_signals.intersects(
            crate::signals::SIG_YIELD
                .union(crate::signals::SIG_DEBUG)
                .union(crate::signals::SIG_IO)
                .union(crate::signals::SIG_WAIT),
        ) {
            self.emit_alloc(|region| LirInstr::SuspendingCall {
                region,
                dst,
                func: func_reg,
                args: arg_regs,
                arity_checked,
            });
        } else {
            self.emit_alloc(|region| LirInstr::Call {
                region,
                dst,
                func: func_reg,
                args: arg_regs,
                arity_checked,
            });
        }
        // The body reference a dynamic `emit` park owes, released here:
        // the primitive parks at the instruction after this call, so this
        // is the continuation past the suspend — the release a fiber
        // abandoned while suspended never reaches and the discard discharge
        // stands in for (docs/impl/region/park.md). The stash is private
        // to this site, and the
        // `LoadLocal`/`DecrefValueRegion` pair is push-pop-neutral around
        // the result the call left on top. Empty for every other non-tail
        // call: only the emit payload sets `borrowed` off tail position.
        //
        // The nil stamp and the table entry are what make this release run
        // ONCE when the signal turns out to be terminal. There the catcher
        // consumes the delivery the raise minted and this retain answers to
        // the continuation alone — reached by a restart's replay, or, for a
        // fiber nobody restarts, by the abandoned-frame walk off exactly
        // this table (docs/impl/region/mechanism.md § "An abandoned frame
        // runs the releases it still owes"). A frame that takes both routes
        // reloads the stamp on the second and no-ops. A SUSPENDING raise
        // records the slot too and is unaffected: its parked payload is what
        // the walk protects, so the walk passes the slot over.
        for &slot in &borrowed_arg_slots {
            let v = self.fresh_reg();
            self.emit(LirInstr::LoadLocal { dst: v, slot });
            self.emit(LirInstr::DecrefValueRegion { src: v });
            if let Ok(nil_reg) = self.emit_const(crate::lir::LirConst::Nil) {
                self.emit(LirInstr::StoreLocal { slot, src: nil_reg });
            }
            if !self.current_func.frame_release_slots.contains(&slot) {
                self.current_func.frame_release_slots.push(slot);
            }
        }
        // After ANF (`src/hir/anf.rs`), every consumer position
        // for a Call has a synthetic `Let` binding owning the
        // result. The enclosing `lower_let` / `lower_letrec` /
        // `lower_define` records `region_to_slot[r]` so
        // `emit_decrefs_for` at the Call's `decref_point` can emit
        // `LoadLocal slot + DecrefValueRegion` — no shadow
        // stash slot needed at the Call site.
        Ok(dst)
    }
}
