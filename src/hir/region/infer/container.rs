// audited: 2026-11-17
//! What the walk records about a reassigned binding: the 1-slot-container
//! class it falls in, and where its binder stores.
//!
//! docs/impl/region/bindings.md

use super::*;
use crate::hir::region::reads::for_each_path;

mod reader;

impl RegionInference {
    /// Record a reassignment of a non-capture (stack-local) mutable binding,
    /// classifying it as MODULE-SCOPE (`top_level_reassigns` — program/module
    /// extent, the file-letrec mutable class) or FN-LOCAL (`local_reassigns` — a
    /// value that shares its enclosing scope region, freed by scope demise). The
    /// split is by `is_file_scope`, NOT raw `in_lambda`, so a file-letrec mutable
    /// stays module-scope even inside the `%file-body` whole-module thunk. Both
    /// become drop-on-overwrite + suppressed-decref in the post-pass, differing
    /// only in which decrefs are suppressed. Capture-cell bindings (`needs_capture`)
    /// are excluded from both — their RC is owned by `handle_update_capture` /
    /// `handle_store_upvalue` — and recorded in `captured_reassigns` instead,
    /// whatever scope the write sits in.
    pub(super) fn record_top_level_reassign(
        &mut self,
        b: Binding,
        site: HirId,
        value: &Hir,
        val_regions: &[Region],
    ) {
        // A read of `b` is counted only while the arena calls `b` a 1-slot
        // container, so functionalization marks every target it keeps.
        debug_assert!(
            self.arena().get(b).is_mutated && !self.arena().get(b).is_immutable,
            "a re-stored binding reads mutated and not immutable"
        );
        // MODULE-SCOPE classification, not the raw `in_lambda` flag: a file-letrec
        // (top-level `def`/`var`) binding is program-extent even when the
        // file-letrec runs inside the synthetic `%file-body` whole-module thunk
        // (`compile/whole-module`, where `in_lambda` is true only because of the
        // thunk wrapper). Its value's demise is the file-letrec scope-region
        // teardown, as in a direct `elle FILE` run. Classified fn-local instead, a
        // reassigned top-level mutable keeps its assign-value decrefs, and the dead
        // `__file_expr_N` wrapper the file-letrec lifts each statement into frees
        // the just-stored value while the cell still holds it
        // (region-toplevel-reassign-thunk-uaf.lisp and
        // region-toplevel-mutable-reassign.lisp pin this).
        let module_scope = !self.in_lambda() || self.arena().get(b).is_file_scope;
        // Capture-cell bindings are excluded from BOTH container maps: their RC is
        // owned by `handle_update_capture`, not the 1-slot-container model. They are
        // recorded separately so the lowerer drops the init's alloc reference at the
        // define (the cell content changes; routing that decref through the cell slot
        // is a UAF).
        //
        // Recorded regardless of the scope classification above: `module_scope` reads
        // the scope THIS WRITE sits in, and a cell is just as repointed by an `assign`
        // inside a closure the defining scope encloses — the shape
        // `(begin (var x …) (defn f () (assign x …)) (f))`, where the binding owns a
        // compiled cell yet every write is in a lambda. Gating on the write site
        // classifies such a binding fn-local, leaves the cell-slot routing in place,
        // and frees the reassigned value under the frame that hands it back
        // (region-capture-cell-closure-reassign-uaf.lisp). A genuinely fn-local
        // captured binding — defined inside a lambda, its cell a `populate_env` env
        // cell — is recorded too, and needs to be for the same reason: its binder
        // routes the init's release through the cell's slot unless this set names it
        // (`lower_define`, `lower_let`), and the env cell's release clamp skips a
        // binding the set names (`pin_cell_release_after_routed_releases`).
        if self.arena().get(b).needs_capture() {
            self.captured_reassigns.insert(b);
            return;
        }
        // Genuine fn-local reassigns go to `local_reassigns` — same container model,
        // but the post-pass keeps the assign-value decrefs (the cell's final value
        // is freed at scope exit, not a program root). Module-scope reassigns go to
        // `top_level_reassigns` (final value freed by the file-letrec scope-region
        // teardown).
        let map = if module_scope {
            &mut self.top_level_reassigns
        } else {
            &mut self.local_reassigns
        };
        map.entry(b)
            .or_default()
            .record(site, val_regions, reads_a_cell(value));
    }

    /// Record that a binder stores `init`'s value into `b`'s slot — the one
    /// position a counted-init retain can take, since the value is on the
    /// operand stack there and nowhere else (`binder_init_sites`). Called from
    /// the `Let`/`Letrec`/`Define` arms, which mirror the three lowering sites
    /// that emit it. A second, DIFFERENT binder for the same binding marks the
    /// entry ambiguous; an inline re-walk of the same binder re-records the same
    /// HirId and leaves it alone.
    pub(super) fn record_binder_init_site(&mut self, b: Binding, init: HirId) {
        self.binder_init_sites
            .entry(b)
            .and_modify(|slot| {
                if *slot != Some(init) {
                    *slot = None;
                }
            })
            .or_insert(Some(init));
    }
}

/// Does `h` produce, on some path, the whole content of a cell? A `DerefCell`
/// borrows the reference the cell keeps, so a store of its value has no
/// producer reference to adopt (docs/impl/region/bindings.md).
fn reads_a_cell(h: &Hir) -> bool {
    if matches!(h.kind, HirKind::DerefCell { .. }) {
        return true;
    }
    let mut reads = false;
    for_each_path(h, |p| reads |= reads_a_cell(p));
    reads
}
