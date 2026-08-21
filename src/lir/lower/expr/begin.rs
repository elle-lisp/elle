//! `Begin` lowering and its slot pre-pass.
//!
//! Split out because `lower_begin` is fronted by a structural pre-pass
//! (`collect_preallocate_bindings`) that pre-allocates slots for mutually
//! recursive `Define`/`Destructure` bindings so a capturing lambda can find a
//! sibling's slot before that sibling is lowered — a self-contained concern.

use super::*;

impl<'a> Lowerer<'a> {
    /// Collect Define and Destructure bindings reachable through
    /// structural wrappings (Let, Begin, Loop, Block) without crossing
    /// Lambda or branching boundaries (If, Match, Cond). Used by the
    /// Begin pre-pass to pre-allocate slots for mutual recursion.
    ///
    /// Only scans through Let/Begin/Loop/Block — these are structural
    /// wrappers. Does NOT scan into If/Match/Cond because different
    /// branches may define bindings with overlapping slot allocation.
    fn collect_preallocate_bindings(hir: &Hir, out: &mut Vec<Binding>) {
        match &hir.kind {
            HirKind::Define { binding, .. } => out.push(*binding),
            HirKind::Destructure { pattern, .. } => out.extend(pattern.bindings().bindings),
            HirKind::Lambda { .. } => {}
            HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } => {
                for (_, init) in bindings {
                    Self::collect_preallocate_bindings(init, out);
                }
                Self::collect_preallocate_bindings(body, out);
            }
            HirKind::Begin(exprs) => {
                for e in exprs {
                    Self::collect_preallocate_bindings(e, out);
                }
            }
            HirKind::Loop { bindings, body } => {
                for (_, init) in bindings {
                    Self::collect_preallocate_bindings(init, out);
                }
                Self::collect_preallocate_bindings(body, out);
            }
            HirKind::Block { body, .. } => {
                for e in body {
                    Self::collect_preallocate_bindings(e, out);
                }
            }
            _ => {}
        }
    }

    pub(super) fn lower_begin(&mut self, exprs: &[Hir]) -> Result<Reg, String> {
        // Pre-allocate slots for all local Define and Destructure bindings
        // reachable from this Begin (including inside Let/Loop/If bodies
        // but NOT inside Lambdas). This enables mutual recursion where
        // lambda A captures variable B before B's Define has been lowered.
        let mut bindings_to_preallocate = Vec::new();
        for expr in exprs {
            Self::collect_preallocate_bindings(expr, &mut bindings_to_preallocate);
        }
        for &binding in &bindings_to_preallocate {
            // Allocate slot now so captures can find it
            if !self.binding_to_slot.contains_key(&binding) {
                let needs_capture = self.arena.get(binding).needs_capture();
                let slot = self.allocate_slot(binding);

                // Inside lambdas, only LBox locals live in the closure
                // environment (LoadCapture/StoreCapture). Non-LBox locals
                // use fast local storage (LoadLocal/StoreLocal).
                if self.in_lambda && needs_capture {
                    self.upvalue_bindings.insert(binding);
                }

                // Only create cells for top-level locals (outside lambdas)
                // Inside lambdas, the VM creates cells for locally-defined variables
                // when building the closure environment
                if needs_capture && !self.in_lambda {
                    // Create a cell containing nil
                    // This cell will be captured by nested lambdas
                    // and updated when the Define is lowered.
                    // One region PER cell (`begin_cell_regions`): emitting all
                    // cells against this Begin's single slot orphans all but
                    // the last minted physical region — the shared-slot
                    // capture-cell leak (docs/impl/region/model.md, "one allocation
                    // execution per slot between drops").
                    let region = self.cell_region_for(binding);
                    let nil_reg = self.emit_const(LirConst::Nil)?;
                    let cell_reg = self.fresh_reg();
                    self.emit_alloc_in(region, |region| LirInstr::MakeCaptureCell {
                        region,
                        dst: cell_reg,
                        value: nil_reg,
                    });
                    self.emit(LirInstr::StoreLocal {
                        slot,
                        src: cell_reg,
                    });
                }
            }
        }
        // Now lower all expressions (slots are available for capture lookup)
        // Pop intermediate results to keep the stack clean
        if exprs.is_empty() {
            return self.emit_const(LirConst::Nil);
        }

        let mut last_reg = self.lower_expr(&exprs[0])?;
        for expr in exprs.iter().skip(1) {
            self.discard(last_reg);
            last_reg = self.lower_expr(expr)?;
        }
        Ok(last_reg)
    }
}
