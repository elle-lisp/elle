// audited: 2026-09-08
//! Control flow lowering: the tail-argument ownership predicates, intrinsic
//! specialization, `eval`, `emit`, and the call path; `and`/`or` and `match`
//! lower in the submodules beside it.
//!
//! docs/impl/lir.md

use super::*;
use crate::hir::CallArg;
use crate::value::fiber::SignalBits;

mod call;
mod matcharms;
mod shortcircuit;

impl<'a> Lowerer<'a> {
    /// Whether a tail-call argument is BORROWED — held by a reference this
    /// activation does not own, so pure-moving it into the callee would
    /// over-free it (see the move-on-tail-call comment in `lower_call`).
    ///
    /// This is a **structural ownership-location** question, NOT true-escape.
    /// Two routes make an argument borrowed:
    ///
    /// - a **captured upvalue** of the current lambda (`upvalue_bindings`) —
    ///   the closure env owns the capture-incref, so this frame has no
    ///   transferable owning reference;
    /// - a **compile-time-constant heap value** (`immutable_values` — a stdlib
    ///   export closure like `+`/`inc`/`map`, a `begin-for-syntax` value). A
    ///   known-constant binding is deliberately never captured
    ///   (hir/analyze/scopes.rs) and lowers to `LoadConst`, so the frame holds
    ///   NO reference at all — the owning references belong to the env that
    ///   seeded the constant. Pure-moving it drains that env's region rc by one
    ///   per call to a premature free; user-reachable as
    ///   `(defn f [xs] (map inc xs))` (the `inc` ARG is moved; `map` as the
    ///   CALLEE is not). An immediate constant (int, keyword, native-fn) has no
    ///   region, so only heap constants qualify
    ///   (region-const-tail-move-borrow-uaf.lisp). Unlike the upvalue route this
    ///   is position-independent — a top-level tail call moves a constant it
    ///   doesn't own all the same — so it is not gated on `in_lambda`.
    ///
    /// The authoritative escape
    /// analysis (`EscapeInfo`) is deliberately NOT used here: among tail-args its
    /// escape set is a strict superset of the borrowed set (a born-here value that
    /// merely flows to a tail *escapes* but is *owned*), and minting for those
    /// owned-escaping args double-releases across a fiber suspend/resume — a
    /// phantom `DecrefRegion` / use-after-free witnessed on `contracts.lisp`. The
    /// env-ownership fact is structural capture, which `is_captured`/`upvalue_bindings`
    /// answer exactly and escape does not; this is the structural-only role of
    /// lexical capture (see `hir::escape` and `docs/impl/escape.md`).
    ///
    /// This borrowed/mint compensation is **transitional value-RC machinery, not a
    /// permanent fixture**: it exists only because today's model mints per
    /// value-escape event. A future ownership-forest model subsumes it — an
    /// intra-fiber captured upvalue lives in an Owned subtree reclaimed by drop
    /// (no mint, no over-free), and only genuine cross-fiber Shared regions keep
    /// edge-RC. The structural capture hint persists (demoted to layout-only);
    /// this predicate does not.
    ///
    /// After ANF a call argument is atomic, and a variable reference takes one of
    /// two shapes (see `functionalize`): a plain `Var(b)`, or `DerefCell(Var(b))`
    /// for a binding that `needs_capture()`. BOTH are borrowed when `b` is a
    /// captured upvalue, so we look THROUGH the `DerefCell` wrapper to the `Var`
    /// (matching only the bare `Var` let a cell-backed top-level binding tail-pass
    /// without the fresh incref — region-tail-move-toplevel-uaf.lisp).
    ///
    /// An argument can also be a BRANCH/PHI compound (`or`/`and`/`if`/`cond`/
    /// `match`) whose runtime value is one of its value-producing leaves — and a
    /// short-circuit `(or borrowed-upvalue fresh)` selects the borrowed operand at
    /// runtime, so the value handed to the callee is the borrowed capture even
    /// though the fresh operand looks owned. Missing this pure-moves the capture
    /// into an owned-param callee, whose release drains the env's capture RC to a
    /// premature free (region-or-tail-move-borrow-uaf.lisp — the phi sibling of
    /// region-tail-move-borrow-uaf.lisp). So look through each compound to its
    /// value-producing leaves and treat the argument as borrowed iff ANY leaf is.
    /// The retain and the operand releases are value-gated
    /// (`IncrefValueRegion`/`DecrefValueRegion` resolve the runtime value's
    /// region), so the single retain balances every arm — it matches the callee's
    /// release on the borrow arm, and on an owned arm it cancels against that
    /// operand's own value-gated release. Precision is preserved: a leaf is
    /// borrowed only when it is a genuine captured upvalue, so an all-owned
    /// compound stays a pure move (never the contracts.lisp owned-escaping
    /// double-release the base case avoids by not using `EscapeInfo`).
    fn tail_arg_is_borrowed(&self, arg: &Hir) -> bool {
        self.arg_leaf_is_borrowed(arg)
    }

    /// Whether ANY value-producing leaf of `arg` is a borrowed captured upvalue —
    /// the phi-transparent core of [`Self::tail_arg_is_borrowed`]. A branch/phi
    /// compound routes tail position (and thus the returned value) to specific
    /// children; recurse into exactly those, mirroring `mark_tail_calls` /
    /// `return_incref`'s notion of a value-producing leaf: every `or`/`and`
    /// operand (any can short-circuit), both `if` branches, each `cond`/`match`
    /// body (never the test/scrutinee/guard). A non-compound leaf is borrowed iff
    /// it is a bare `Var`/`DerefCell(Var)` naming a captured upvalue.
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

    /// The regions a tail argument's value-producing leaves may name — the
    /// coordinate the move is accounted in.
    ///
    /// A tail call moves ONE reference per region, because that is all the frame
    /// holds, while the callee releases once per owned parameter. So an argument
    /// list must be read for repetition, and repetition is a fact about regions
    /// rather than about syntax: `(f x y)` after `(let [y x] …)` names one region
    /// through two bindings, and a check keyed on binding identity misses it
    /// (docs/impl/region/rules.md Rule 5, `region-tail-repeated-arg-uaf.lisp`).
    ///
    /// Walks the same value-producing leaves as [`Self::arg_leaf_is_borrowed`],
    /// for the same reason: a branch/phi compound hands the callee whichever leaf
    /// runs. An allocating leaf contributes nothing — a fresh region per
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
                self.emit(LirInstr::Convert { dst, op, src });
                Ok(Some(dst))
            }
        }
    }

    pub(super) fn lower_eval(&mut self, expr: &Hir, env: &Hir) -> Result<Reg, String> {
        let env_reg = self.lower_expr(env)?;
        let expr_reg = self.lower_expr(expr)?;
        let dst = self.fresh_reg();
        self.emit(LirInstr::Eval {
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
        // Region inference stamps yield-bound allocations with the Parent
        // region via alloc_region, so the value the resumer reads is already
        // born where it must outlive this fiber's suspension. Nothing is
        // emitted here to mark the boundary.
        let value_reg = self.lower_expr(value)?;

        // A fiber body owns one reference of every value it yields
        // (docs/impl/region/owner.md § "Park/unpark symmetry"). The park's own
        // `EmitEscape` retain is the DELIVERY reference — the resumer's release of
        // the resume result consumes it — so what a discarded fiber's discharge
        // stands in for is the body's separate reference, released by the
        // continuation past this suspend. Where the payload is a borrow this body
        // releases nowhere, that reference does not exist until it is minted here.
        //
        // Only a SUSPENDING signal parks a continuation to release it in, and only
        // a non-terminal parked signal reaches the discharge: an error leaves
        // through the unwind path and a halt promotes the fiber to `:dead`, so
        // neither instruction below this one ever runs and a reference minted for
        // it would be stranded, one per emit. Both are terminal, and a terminal
        // result's payload is pinned by the resume's own park retain instead
        // (`incref_signal_region`, `with_child_fiber` step 6a).
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

        // The balancing release, first in the continuation the resume replays —
        // and, for a fiber nobody resumes again, the one the discharge stands in
        // for. The slot is written on every execution of this `Emit`, so a park
        // inside a loop reads its own iteration's value and needs no nil-stamp.
        if let Some(slot) = borrow_slot {
            let val_reg = self.fresh_reg();
            self.emit(LirInstr::LoadLocal { dst: val_reg, slot });
            self.emit(LirInstr::DecrefValueRegion { src: val_reg });
        }

        let dst = self.fresh_reg();
        self.emit(LirInstr::LoadResumeValue { dst });
        // The resume value crosses from the resumer uncounted — `resume_suspended`
        // pushes it onto this frame's stack and takes no reference for it. Mint the
        // reference this body holds it by, so a later park cannot leave the resumer's
        // release freeing a value this frame still reads; the `Emit`'s own
        // call-result `DecrefValueRegion` gives it back. Skipped where the frame's
        // return transfer already funds one for the same region
        // (`unfunded_resume_values`).
        if self
            .current_hir_id
            .is_some_and(|id| self.region_info.unfunded_resume_values.contains(&id))
        {
            self.emit(LirInstr::IncrefValueRegion { src: dst });
        }

        Ok(dst)
    }

    /// Retain the payload of an `Emit` whose body owns no reference of it, and
    /// park a copy in a local slot the continuation can release it through.
    ///
    /// The operand stack is saved and restored across the suspend, and locals live
    /// at its base (`Code::reserved_locals`), so a slot is the one route to the
    /// value after the resume — the value register itself is consumed by the
    /// `Emit`. The slot is private to this site, so nothing else can be reading it
    /// when the release loads it. `IncrefValueRegion` peeks rather than consumes,
    /// leaving `value_reg` live for the terminator; the round-trip through the slot
    /// restores the emitter's stack entry for it.
    fn retain_emit_payload(&mut self, value_reg: Reg) -> u16 {
        self.emit(LirInstr::IncrefValueRegion { src: value_reg });
        let slot = self.current_func.num_locals;
        self.current_func.num_locals += 1;
        self.emit(LirInstr::StoreLocal {
            slot,
            src: value_reg,
        });
        self.emit(LirInstr::LoadLocal {
            dst: value_reg,
            slot,
        });
        slot
    }
}
