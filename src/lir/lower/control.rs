// audited: 2026-10-13
//! Control flow lowering: the tail-argument ownership predicates, intrinsic
//! specialization, `eval`, `emit`, and the call path. `and`/`or` and `match`
//! lower in the submodules beside it.
//!
//! docs/impl/lir.md
//! docs/impl/region/park.md

use super::*;
use crate::hir::CallArg;
use crate::value::fiber::SignalBits;

mod call;
mod matcharms;
mod shortcircuit;

impl<'a> Lowerer<'a> {
    /// Whether a tail-call argument is BORROWED: held by a reference this
    /// activation does not own, so pure-moving it into the callee would
    /// over-free it (see the move-on-tail-call comment in `lower_call`). The
    /// routes and their argument are docs/impl/region/rules.md Rule 5; this is
    /// a **structural ownership-location** question, NOT true-escape.
    ///
    /// The routes, by leaf:
    ///
    /// - a **captured upvalue** of the current lambda (`upvalue_bindings`): the
    ///   closure env owns the capture-incref;
    /// - a **pattern element** (`destructure_alias_bindings`): the scrutinee
    ///   owns the page it lives in;
    /// - a **compile-time-constant heap value** (`immutable_values`): a stdlib
    ///   export closure like `+`/`inc`/`map`, or a `begin-for-syntax` value. It
    ///   lowers to `ValueConst`, so the frame holds NO reference at all. The
    ///   argument is user-reachable as `(defn f [xs] (map inc xs))`, where the
    ///   `inc` ARG is moved and `map` as the CALLEE is not
    ///   (region-const-tail-move-borrow-uaf.lisp). This route is not gated on
    ///   `in_lambda`: a top-level tail call moves a constant it does not own all
    ///   the same;
    /// - a **renamed borrow** (`borrowed_alias_bindings`): a `let`, `letrec` or
    ///   `def` binding whose initializer passed this test, and whose binder took
    ///   no counted reference (`note_borrowed_alias`).
    ///
    /// `EscapeInfo` is deliberately NOT read here. Among tail arguments its
    /// escape set is a strict superset of the borrowed set: a born-here value
    /// that merely flows to a tail *escapes* but is *owned*. Minting for those
    /// owned arguments double-releases across a fiber suspend/resume, which
    /// tests/lang/contracts.lisp witnessed as a phantom `DecrefRegion`.
    ///
    /// After ANF a variable argument is a plain `Var(b)`, or `DerefCell(Var(b))`
    /// for a binding that `needs_capture()`. The predicate looks THROUGH the
    /// `DerefCell` to the `Var`. Matching only the bare `Var` lets a cell-backed
    /// top-level binding tail-pass without the fresh incref
    /// (region-tail-move-toplevel-uaf.lisp).
    ///
    /// An argument can also be a BRANCH/PHI compound (`or`/`and`/`if`/`cond`/
    /// `match`) whose runtime value is one of its value-producing leaves. A
    /// short-circuit `(or borrowed-upvalue fresh)` hands the callee the borrowed
    /// capture whenever it is truthy. Missing that pure-moves the capture into an
    /// owned-param callee, whose release drains the env's count to a premature
    /// free (region-or-tail-move-borrow-uaf.lisp). So the argument is borrowed
    /// when ANY leaf is. The retain is value-gated (`IncrefValueRegion` resolves
    /// the runtime value's region), so one retain names whichever leaf ran. An
    /// all-owned compound stays a pure move.
    fn tail_arg_is_borrowed(&self, arg: &Hir) -> bool {
        self.arg_leaf_is_borrowed(arg)
    }

    /// Whether ANY value-producing leaf of `arg` is borrowed: the phi-transparent
    /// core of [`Self::tail_arg_is_borrowed`]. A branch/phi compound routes its
    /// value to specific children, and this recurses into exactly those, the
    /// leaves `mark_tail_calls` reads. Those are every `or`/`and` operand (any can
    /// short-circuit), both `if` branches, and each `cond`/`match` body, never the
    /// test, scrutinee or guard. A non-compound leaf is borrowed when it is a bare
    /// `Var`/`DerefCell(Var)` that one of the routes names.
    fn arg_leaf_is_borrowed(&self, arg: &Hir) -> bool {
        match &arg.kind {
            // Look through the `DerefCell` wrapper `functionalize` adds around a
            // needs-capture binding read; the borrowed atom underneath is a `Var`.
            HirKind::DerefCell { cell } => self.arg_leaf_is_borrowed(cell),
            // The `in_lambda` gate applies to the UPVALUE route only: at top
            // level `upvalue_bindings` names bindings some lambda captures, but
            // a top-level read of one is a plain owned slot load. The CONST
            // route is position-independent (the frame never owns a constant).
            HirKind::Var(binding) => {
                (self.in_lambda && self.upvalue_bindings.contains(binding))
                    || self.destructure_alias_bindings.contains(binding)
                    || self.borrowed_alias_bindings.contains(binding)
                    || self
                        .immutable_values
                        .get(binding)
                        .is_some_and(|v| v.is_heap())
            }
            HirKind::Or(exprs) | HirKind::And(exprs) => {
                exprs.iter().any(|e| self.arg_leaf_is_borrowed(e))
            }
            HirKind::If {
                then_branch,
                else_branch,
                ..
            } => self.arg_leaf_is_borrowed(then_branch) || self.arg_leaf_is_borrowed(else_branch),
            HirKind::Cond {
                clauses,
                else_branch,
            } => {
                clauses
                    .iter()
                    .any(|(_, body)| self.arg_leaf_is_borrowed(body))
                    || else_branch
                        .as_ref()
                        .is_some_and(|e| self.arg_leaf_is_borrowed(e))
            }
            HirKind::Match { arms, .. } => arms
                .iter()
                .any(|(_, _, body)| self.arg_leaf_is_borrowed(body)),
            _ => false,
        }
    }

    /// Record `binding` as borrowed when its binder hands it a value this
    /// activation holds no reference to. That is so when `init` would be a
    /// borrowed tail argument in the binding's place, and the binder took no
    /// counted reference of its own: no counted read, no counted cell init
    /// (docs/impl/region/reads.md). Every binder calls this before it lowers the
    /// body that names the binding.
    pub(in crate::lir::lower) fn note_borrowed_alias(&mut self, binding: Binding, init: &Hir) {
        let counted = self.region_info.counted_cell_read_sites.contains(&init.id)
            || self.region_info.counted_cell_init_sites.contains(&init.id);
        if !counted && self.arg_leaf_is_borrowed(init) {
            self.borrowed_alias_bindings.insert(binding);
        }
    }

    /// The regions a tail argument's value-producing leaves may name: the
    /// coordinate the move is accounted in.
    ///
    /// A tail call moves ONE reference per region, because that is all the frame
    /// holds, while the callee releases once per owned parameter. So an argument
    /// list must be read for repetition, and repetition is a fact about regions
    /// rather than about syntax. `(f x y)` after `(let [y x] …)` names one region
    /// through two bindings, and a check keyed on binding identity misses it
    /// (docs/impl/region/rules.md Rule 5, `region-tail-repeated-arg-uaf.lisp`).
    ///
    /// Walks the same value-producing leaves as [`Self::arg_leaf_is_borrowed`],
    /// for the same reason: a branch/phi compound hands the callee whichever leaf
    /// runs. An allocating leaf contributes nothing, because a fresh region per
    /// evaluation cannot be an earlier argument's.
    fn arg_leaf_regions(
        &self,
        arg: &Hir,
        out: &mut rustc_hash::FxHashSet<crate::hir::region::Region>,
    ) {
        match &arg.kind {
            HirKind::DerefCell { cell } => self.arg_leaf_regions(cell, out),
            HirKind::Var(binding) => {
                for &r in self
                    .region_info
                    .binding_source_regions
                    .get(binding)
                    .into_iter()
                    .flatten()
                {
                    out.insert(self.region_info.merged_root(r));
                }
            }
            HirKind::Or(exprs) | HirKind::And(exprs) => {
                for e in exprs {
                    self.arg_leaf_regions(e, out);
                }
            }
            HirKind::If {
                then_branch,
                else_branch,
                ..
            } => {
                self.arg_leaf_regions(then_branch, out);
                self.arg_leaf_regions(else_branch, out);
            }
            HirKind::Cond {
                clauses,
                else_branch,
            } => {
                for (_, body) in clauses {
                    self.arg_leaf_regions(body, out);
                }
                if let Some(e) = else_branch {
                    self.arg_leaf_regions(e, out);
                }
            }
            HirKind::Match { arms, .. } => {
                for (_, _, body) in arms {
                    self.arg_leaf_regions(body, out);
                }
            }
            _ => {}
        }
    }

    /// Try to lower a call as an intrinsic operation.
    ///
    /// Returns `Some(result_reg)` if the call was specialized, `None` to
    /// fall through to generic call. Only specializes when:
    /// - The function is a global variable reference
    /// - The global is not mutated (so it still holds the original primitive)
    /// - The SymbolId maps to a known intrinsic
    /// - The argument count matches (2 for binary/compare, 1 for unary)
    fn try_lower_intrinsic(&mut self, func: &Hir, args: &[&Hir]) -> Result<Option<Reg>, String> {
        // Must be a variable reference
        let HirKind::Var(binding) = &func.kind else {
            return Ok(None);
        };

        // Must be an immutable binding that hasn't been mutated
        let bi = self.arena.get(*binding);
        if !bi.is_immutable || bi.is_mutated {
            return Ok(None);
        }

        let sym = bi.name;

        let Some(&intrinsic) = self.intrinsics.get(&sym) else {
            return Ok(None);
        };

        match intrinsic {
            IntrinsicOp::Conversion(op) => {
                if args.len() != 1 {
                    return Ok(None); // 2-arg (integer str radix) falls through to Call
                }
                let src = self.lower_expr(args[0])?;
                let dst = self.fresh_reg();
                self.emit(InstrRef::Convert { dst, op, src });
                Ok(Some(dst))
            }
        }
    }

    pub(super) fn lower_eval(&mut self, expr: &Hir, env: &Hir) -> Result<Reg, String> {
        let env_reg = self.lower_expr(env)?;
        let expr_reg = self.lower_expr(expr)?;
        let dst = self.fresh_reg();
        self.emit(InstrRef::Eval {
            dst,
            expr: expr_reg,
            env: env_reg,
        });
        // Eval's result lives in a region the outer compilation
        // didn't allocate; `emit_decrefs_for` uses `region_to_slot`
        // (recorded by the enclosing binding site after ANF) and
        // gates the runtime decref on the actual region.
        Ok(dst)
    }

    pub(super) fn lower_emit(
        &mut self,
        signal: crate::value::fiber::SignalBits,
        value: &Hir,
    ) -> Result<Reg, String> {
        // Nothing is emitted here to mark the fiber boundary: the park's runtime
        // retain covers the payload (`handle_emit`).
        let value_reg = self.lower_expr(value)?;

        // A fiber body owns one reference of every value it yields
        // (docs/impl/region/park.md). The park's own `EmitEscape` retain is the
        // DELIVERY reference, which the resumer's release of the resume result
        // consumes. So what a discarded fiber's discharge stands in for is the
        // body's separate reference, released by the continuation past this
        // suspend. Where the payload is a borrow this body releases nowhere,
        // that reference does not exist until it is minted here.
        //
        // Only a SUSPENDING signal parks a continuation to release it in, and
        // only a non-terminal parked signal reaches the discharge. An error
        // leaves through the unwind path, and a halt promotes the fiber to
        // `:dead`. So neither runs any instruction below this one, and a
        // reference minted for it would be stranded, one per emit. Both are
        // terminal, and the resume's own park retain pins a terminal result's
        // payload instead (`incref_signal_region`, `with_child_fiber` step 6a).
        let suspends = !signal.intersects(crate::value::SIG_ERROR)
            && !signal.intersects(crate::value::SIG_HALT);
        let borrow_slot = self
            .current_hir_id
            .filter(|id| suspends && self.region_info.borrowed_emit_payloads.contains(id))
            .map(|_| self.retain_emit_payload(value_reg));

        let resume_label = self.fresh_label();

        self.terminate(Terminator::Emit {
            signal,
            value: value_reg,
            resume_label,
        });

        self.start_new_block(resume_label);

        // The balancing release, first in the continuation the resume replays.
        // For a fiber nobody resumes again, the discharge stands in for it while
        // the park holds its payload. An abort that raises in place over the
        // park, or a squelch boundary that discards it, takes the payload out of
        // the signal slot, and then only the walk can reach this release. So it
        // is a recorded value route: the nil stamp is the receipt that keeps a
        // restart's replay and the walk from both running it
        // (docs/impl/region/unwind.md).
        if let Some(slot) = borrow_slot {
            self.emit_recorded_slot_value_release(slot);
        }

        let dst = self.fresh_reg();
        self.emit(InstrRef::LoadResumeValue { dst });
        // The resume value crosses from the resumer uncounted: `resume_suspended`
        // pushes it onto this frame's stack and takes no reference for it. Mint
        // the reference this body holds it by, so a later park cannot leave the
        // resumer's release freeing a value this frame still reads. The release
        // of the binding that names the `Emit` gives it back. An unnamed `Emit`
        // is a returning tail, which no slot releases, so it takes none
        // (`unfunded_resume_values`).
        if self
            .current_hir_id
            .is_some_and(|id| self.region_info.unfunded_resume_values.contains(&id))
        {
            self.emit(InstrRef::IncrefValueRegion { src: dst });
        }

        Ok(dst)
    }

    /// Retain the payload of an `Emit` whose body owns no reference of it, and
    /// park a copy in a local slot the continuation can release it through.
    ///
    /// The operand stack is saved and restored across the suspend, and locals live
    /// at its base (`Code::reserved_locals`). So a slot is the one route to the
    /// value after the resume, because the `Emit` consumes the value register
    /// itself. The slot is private to this site, so nothing else can be reading
    /// it when the release loads it. `IncrefValueRegion` peeks rather than
    /// consumes, leaving `value_reg` live for the terminator; the round-trip
    /// through the slot restores the emitter's stack entry for it.
    fn retain_emit_payload(&mut self, value_reg: Reg) -> u16 {
        self.emit(InstrRef::IncrefValueRegion { src: value_reg });
        let slot = self.fresh_local();
        self.emit(InstrRef::StoreLocal {
            slot,
            src: value_reg,
        });
        self.emit(InstrRef::LoadLocal {
            dst: value_reg,
            slot,
        });
        slot
    }
}
