//! Expression lowering - the main `lower_expr` dispatch

use super::*;

mod begin;
mod block;
mod boundary;
mod intrinsic;
mod loops;
mod var;

impl<'a> Lowerer<'a> {
    /// Lower a HIR expression to LIR
    pub(super) fn lower_expr(&mut self, hir: &Hir) -> Result<Reg, String> {
        let saved_span = self.current_span.clone();
        let saved_hir_id = self.current_hir_id;
        self.current_span = hir.span.clone();
        self.current_hir_id = Some(hir.id);

        // Per-path branch compensation: if this node is a branch arm body whose
        // sibling arm holds a live-in region's `decref_point`, free that region at
        // this arm's head (it would otherwise leak on this path). Emitted into the
        // arm's basic block, before the arm body — hence before any tail call.
        self.emit_branch_compensation(hir.id);

        let result = match &hir.kind {
            HirKind::Nil => self.emit_const(LirConst::Nil),
            HirKind::EmptyList => self.emit_const(LirConst::EmptyList),
            HirKind::Bool(b) => self.emit_const(LirConst::Bool(*b)),
            HirKind::Int(n) => self.emit_const(LirConst::Int(*n)),
            HirKind::Float(f) => self.emit_const(LirConst::Float(*f)),
            HirKind::String(s) => {
                // A string literal is an ordinary allocation (not a pool load):
                // materialize it fresh into its OWN solver-assigned region. The
                // region is resolved from `current_hir_id` (this String node),
                // which the solver gave a region via `alloc_here`. `emit_alloc`
                // stamps the region (arming its `DecrefRegion` at `decref_point`).
                let dst = self.fresh_reg();
                let template = crate::value::ConstTemplate::String(s.clone());
                self.emit_alloc(|region| LirInstr::MaterializeConst {
                    dst,
                    template,
                    region,
                });
                Ok(dst)
            }
            HirKind::Keyword(name) => self.emit_const(LirConst::Keyword(name.clone())),

            HirKind::Var(binding) => self.lower_var(binding, &hir.span),
            HirKind::Let { bindings, body } => self.lower_let(bindings, body, hir.id),
            HirKind::Letrec { bindings, body } => self.lower_letrec(bindings, body, hir.id),
            HirKind::Lambda {
                params,
                num_required,
                rest_param,
                vararg_kind,
                captures,
                body,
                num_locals,
                inferred_signals,
                param_bounds,
                doc,
                syntax,
                assert_numeric,
            } => self.lower_lambda_expr(
                params,
                *num_required,
                rest_param.as_ref(),
                vararg_kind,
                captures,
                body,
                *num_locals,
                inferred_signals,
                param_bounds,
                doc.clone(),
                syntax.clone(),
                *assert_numeric,
            ),

            HirKind::If {
                cond,
                then_branch,
                else_branch,
            } => self.lower_if(cond, then_branch, else_branch),

            HirKind::Begin(exprs) => self.lower_begin(exprs),
            HirKind::Block { block_id, body, .. } => self.lower_block(block_id, body),
            HirKind::Break { block_id, value } => self.lower_break(block_id, value),

            HirKind::Call {
                func,
                args,
                is_tail,
            } => self.lower_call(func, args.as_slice(), *is_tail, hir.signal.bits),

            HirKind::Assign { target, value } => self.lower_assign(target, value),
            HirKind::Define { binding, value } => self.lower_define(*binding, value),
            HirKind::Destructure {
                pattern,
                value,
                strict,
            } => self.lower_destructure_expr(pattern, value, *strict, &hir.span),

            HirKind::While { cond, body } => self.lower_while(cond, body, hir.id),
            HirKind::Loop { bindings, body } => self.lower_loop(bindings, body, hir.id),
            HirKind::Recur { args } => self.lower_recur(args),

            HirKind::And(exprs) => self.lower_and(exprs),
            HirKind::Or(exprs) => self.lower_or(exprs),

            HirKind::Emit { signal, value } => self.lower_emit(*signal, value),
            HirKind::Quote(value) => self.emit_value_const(*value),
            HirKind::QuoteConst(template) => {
                // Quoted compound data is an ordinary allocation: materialize a
                // FRESH structure from the template into this literal's OWN
                // solver-assigned region each execution (docs/impl/region/model.md
                // § "Constants lower as ordinary allocations"). `emit_alloc` stamps the
                // region (arming its `DecrefRegion` at `decref_point`), exactly
                // like `HirKind::String`.
                let dst = self.fresh_reg();
                let template = template.clone();
                self.emit_alloc(|region| LirInstr::MaterializeConst {
                    dst,
                    template,
                    region,
                });
                Ok(dst)
            }
            HirKind::Cond {
                clauses,
                else_branch,
            } => self.lower_cond(clauses, else_branch),

            HirKind::Match { value, arms } => self.lower_match(value, arms),
            HirKind::Eval { expr, env } => self.lower_eval(expr, env),
            HirKind::Parameterize { bindings, body } => self.lower_parameterize(bindings, body),

            HirKind::MakeCell { value } => self.lower_make_cell(value),
            HirKind::DerefCell { cell } => self.lower_deref_cell(cell),
            HirKind::SetCell { cell, value } => self.lower_set_cell(cell, value),

            HirKind::Intrinsic { op, args } => self.lower_intrinsic(*op, args),

            HirKind::Return { value } => self.lower_return(value),

            HirKind::Error => Err(format!(
                "internal: error poison node in lowerer at {}",
                hir.span
            )),
        };

        // Emit IncrefRegion for cross-region references at this node,
        // then DecrefRegion for every region whose `decref_point` HirId is
        // this node: the lowerer is driven by per-region last-use, not by
        // scope exits.
        if let Ok(result_reg) = result {
            self.emit_increfs_for(hir.id);
            // A caller may defer this node's decrefs to emit them itself at
            // a better point (e.g. `lower_let` emits a binding init's decref
            // only after storing the init value into the slot the decref
            // reloads — otherwise it decrefs the slot's stamped `nil` and the
            // value leaks).
            if !self.deferred_decref_points.contains(&hir.id) {
                self.emit_decrefs_for(hir.id, Some(result_reg));
            }
            // Per-arm sibling-arm releases (used-in-multiple-arms): after the
            // node's own decrefs, so the release follows the arm's use of the value.
            self.emit_arm_decrefs(hir.id);
        }

        self.current_span = saved_span;
        self.current_hir_id = saved_hir_id;
        result
    }
}
