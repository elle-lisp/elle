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
    fn collect_preallocate_bindings(hir: &Hir, out: &mut Vec<(Binding, bool)>) {
        match &hir.kind {
            HirKind::Define { binding, value } => {
                out.push((*binding, matches!(value.kind, HirKind::Lambda { .. })))
            }
            HirKind::Destructure { pattern, .. } => {
                out.extend(pattern.bindings().bindings.iter().map(|&b| (b, false)))
            }
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

    pub(super) fn lower_begin(&mut self, hir_id: HirId, exprs: &[Hir]) -> Result<Reg, String> {
        // Pre-allocate slots for all local Define and Destructure bindings
        // reachable from this Begin (including inside Let/Loop/If bodies
        // but NOT inside Lambdas). This enables mutual recursion where
        // lambda A captures variable B before B's Define has been lowered.
        let mut bindings_to_preallocate = Vec::new();
        for expr in exprs {
            Self::collect_preallocate_bindings(expr, &mut bindings_to_preallocate);
        }
        for &(binding, init_is_lambda) in &bindings_to_preallocate {
            // Allocate slot now so captures can find it
            if !self.binding_to_slot.contains_key(&binding) {
                // A COMPILED forward cell lives in the binding's own stack slot:
                // every captured binding at top level, and inside a lambda the
                // recursive-closure shape — immutable, never mutated,
                // lambda-initialized. That is the shape a run of local `defn`s
                // takes, and it is what gives the closure-cycle merge a
                // static-slot cell to collapse with its SCC
                // (docs/impl/region/letrec.md). Every other captured binding
                // keeps the `populate_env` env-cell route, where the VM builds
                // the cell as it builds the closure environment.
                if self
                    .arena
                    .get(binding)
                    .compiled_forward_cell(init_is_lambda, self.in_lambda)
                {
                    self.allocate_compiled_cell_slot(binding)?;
                    continue;
                }
                let needs_capture = self.arena.get(binding).needs_capture();
                self.allocate_slot(binding);

                // Inside lambdas, only LBox locals live in the closure
                // environment (LoadCapture/StoreCapture). Non-LBox locals
                // use fast local storage (LoadLocal/StoreLocal).
                if self.in_lambda && needs_capture {
                    self.upvalue_bindings.insert(binding);
                }
            }
        }
        // Now lower all expressions (slots are available for capture lookup)
        // Pop intermediate results to keep the stack clean
        if exprs.is_empty() {
            return self.emit_const(LirConst::Nil);
        }

        // A `Begin` that prebound forward cells is a mutual-recursion cycle's
        // BINDING SCOPE, so the releases it emits after its last expression are
        // stranded by a frame-replacing tail call there exactly as a `Letrec`'s
        // are — `(dv n)` closing a run of local `defn`s is the everyday case, and
        // the merged arena's drop is what it takes out. Marked here, before the
        // body is lowered, so the tail call itself sees it. A `Begin` that
        // prebound nothing is no scope: it emits no scope-end release for a tail
        // call to strand, and marking one there would defer a release that also
        // fires live.
        if self.region_info.begin_cell_regions.contains_key(&hir_id) {
            if let Some(last) = exprs.last() {
                self.mark_body_tail_strands(hir_id, last);
            }
        }

        let mut last_reg = self.lower_expr(&exprs[0])?;
        for expr in exprs.iter().skip(1) {
            self.discard(last_reg);
            last_reg = self.lower_expr(expr)?;
        }
        Ok(last_reg)
    }
}
