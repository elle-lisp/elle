// audited: 2026-09-14
//! The COMPILED capture cells the walk mirrors from the lowerer's pre-pass, so
//! each cell's region outlives the binding it holds.
//!
//! docs/impl/region/cells.md

use super::*;

impl RegionInference {
    /// Mirror of `lower_begin`'s collect_preallocate_bindings: a Begin
    /// emits MakeCaptureCell at its HirId iff some reachable Define or
    /// Destructure binding (reachable via Let/Begin/Loop/Block, NOT via
    /// If/Match/Cond/Lambda) takes a COMPILED forward cell
    /// (`BindingInner::compiled_forward_cell`).
    pub(super) fn begin_has_capturable_binding(&self, exprs: &[Hir]) -> bool {
        let mut capturable = Vec::new();
        Self::collect_begin_capturable_bindings(
            self.arena(),
            self.in_lambda(),
            exprs,
            &mut capturable,
        );
        !capturable.is_empty()
    }

    /// Mirror of `lower_begin`'s collect_preallocate_bindings: collect
    /// every Define/Destructure binding reachable via Let/Begin/Loop/Block
    /// (NOT via If/Match/Cond/Lambda) that takes a COMPILED forward cell.
    /// Each of these gets a MakeCaptureCell at the Begin's HirId during
    /// lowering, so the Begin's alloc region must outlive each binding's
    /// last use. This populates `binding_regions[b]` with the Begin's
    /// alloc region so the post-pass `decref_point` extension covers them.
    pub(super) fn collect_begin_capturable_bindings(
        arena: &BindingArena,
        in_lambda: bool,
        exprs: &[Hir],
        out: &mut Vec<Binding>,
    ) {
        fn celled(arena: &BindingArena, b: Binding, init_is_lambda: bool, in_lambda: bool) -> bool {
            arena
                .get(b)
                .compiled_forward_cell(init_is_lambda, in_lambda)
        }
        fn walk(arena: &BindingArena, in_lambda: bool, h: &Hir, out: &mut Vec<Binding>) {
            match &h.kind {
                HirKind::Define { binding, value }
                    if celled(
                        arena,
                        *binding,
                        matches!(value.kind, HirKind::Lambda { .. }),
                        in_lambda,
                    ) =>
                {
                    out.push(*binding);
                }
                HirKind::Destructure { pattern, .. } => {
                    for b in &pattern.bindings().bindings {
                        if celled(arena, *b, false, in_lambda) {
                            out.push(*b);
                        }
                    }
                }
                HirKind::Lambda { .. } => {}
                HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } => {
                    for (_, init) in bindings {
                        walk(arena, in_lambda, init, out);
                    }
                    walk(arena, in_lambda, body, out);
                }
                HirKind::Loop { bindings, body } => {
                    for (_, init) in bindings {
                        walk(arena, in_lambda, init, out);
                    }
                    walk(arena, in_lambda, body, out);
                }
                HirKind::Begin(es) => {
                    for e in es {
                        walk(arena, in_lambda, e, out);
                    }
                }
                HirKind::Block { body, .. } => {
                    for e in body {
                        walk(arena, in_lambda, e, out);
                    }
                }
                _ => {}
            }
        }
        for e in exprs {
            walk(arena, in_lambda, e, out);
        }
    }

    /// Record the region of one COMPILED capture cell a scope arm minted, and
    /// the binding it belongs to. The two writes travel together so no arm can
    /// mint a cell region without also taking the binding off the env-cell
    /// route (`env_cell_placeholder`).
    pub(super) fn record_compiled_cell(
        &mut self,
        scope_id: HirId,
        binding: Binding,
        cell_region: Region,
    ) {
        self.begin_cell_regions
            .entry(scope_id)
            .or_default()
            .push((binding, cell_region));
        self.compiled_cell_bindings.insert(binding);
    }
}
