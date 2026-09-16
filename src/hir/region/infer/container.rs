// audited: 2026-09-15
//! What the walk records about a reassigned binding: the 1-slot-container
//! class it falls in, where its binder stores, and who reads it whole.
//!
//! docs/impl/region/bindings.md

use super::*;

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
        val_regions: &[Region],
    ) {
        // MODULE-SCOPE classification, not the raw `in_lambda` flag: a file-letrec
        // (top-level `def`/`var`) binding is program-extent even when the
        // file-letrec runs inside the synthetic `%file-body` whole-module thunk
        // (`compile/whole-module`, where `in_lambda` is spuriously true — the thunk
        // wrapper). Its value's true demise is the file-letrec scope-region
        // teardown, identical to a direct `elle FILE` run, so it must be classified
        // top-level there too. Without this an `elle test` whole-file run routed a
        // reassigned top-level mutable to `local_reassigns`, which keeps the
        // assign-value decrefs; the file-letrec lifts each statement into a dead
        // `__file_expr_N` wrapper whose slot-routed decref then freed the
        // just-stored value while the cell still held it — the
        // `(assign x (pair … x))` UAF (region-toplevel-reassign-thunk-uaf.lisp; the
        // advanced.lisp match-in-loop crash under `elle test`).
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
        // captured binding — defined inside a lambda — is unaffected by the wider
        // recording: its cell is a `populate_env` env cell reached by `StoreCapture`,
        // a path that never consults this set.
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
        map.entry(b).or_default().record(site, val_regions);
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

    /// The reader half of the 1-slot container: when a binding `reader` is
    /// initialised from a WHOLE-VALUE read of a container binding, give the reader
    /// a COUNTED reference of its own instead of aliasing the container's value
    /// uncounted. The container releases what it held at every re-store, so an
    /// uncounted alias is freed under the reader by the next overwrite
    /// (docs/impl/region/bindings.md § "A whole-value read of a 1-slot container
    /// takes a counted reference").
    ///
    /// Realised as Rule 5's "new reference" pass-through: mint a placeholder
    /// region at the read node (it lands in `call_result_regions`, so the reader
    /// carries a value-based `DecrefValueRegion` at its last use) and record the
    /// read site so the lowerer emits the balancing `IncrefValueRegion`. Returns
    /// the placeholder in place of the regions the containers contributed, or the
    /// unmodified `init_regions` when no path is such a read.
    ///
    /// The replacement is per-path, which is what admits a MIXED branch: only the
    /// regions the reading arms contributed are withdrawn, and an arm that
    /// allocates keeps its own — those regions are the only thing extending that
    /// value's last use out to the binder's retain. One `IncrefValueRegion` names
    /// whichever value arrived, so both halves balance (docs/impl/region/bindings.md
    /// § "A branch is a read of whichever arms read").
    ///
    /// The source test is `is_one_slot_container`, which reads the re-store fact
    /// without the realization: a captured cell whose update opcode decrefs the
    /// displaced prior, and an uncelled `@`-mutable local whose drop-on-overwrite
    /// does the same, expose a reader identically. It covers both scopes (fn-local
    /// upvalue read and module-scope cell read) for the same reason.
    ///
    /// Skipped when the reader is itself a capture cell — its own store/overwrite
    /// accounting owns its references (the alias-of-a-mutable-by-a-mutable
    /// pairing) — and for an immediate-valued read (no heap reference to count).
    /// Element reads (`first`/`get`/destructure) never reach here: they are not a
    /// bare `Var`/`DerefCell` of the container, and an element is independently
    /// counted by its parent's alloc-scan (it cascades, never frees under the
    /// reader).
    pub(super) fn counted_cell_read_regions(
        &mut self,
        reader: Binding,
        init: &Hir,
        init_regions: Vec<Region>,
    ) -> Vec<Region> {
        if init_regions.is_empty() || self.arena().get(reader).needs_capture() {
            return init_regions;
        }
        let mut read_regions = Vec::new();
        self.whole_container_read_regions(init, self.arena().get(reader).name, &mut read_regions);
        if read_regions.is_empty() {
            return init_regions;
        }
        let read_r = self.alloc_here(init.id);
        self.call_result_regions.insert(read_r);
        self.counted_cell_read_sites.insert(init.id);
        let mut out = vec![read_r];
        out.extend(
            init_regions
                .into_iter()
                .filter(|r| !read_regions.contains(r)),
        );
        out
    }

    /// Collect into `out` the source regions of every 1-slot container `h`
    /// produces the whole current content of, on any one of its paths. The leaves
    /// are a bare `Var(b)` over such a container and the `DerefCell`-wrapped form
    /// `functionalize` puts around a needs-capture read (the fn-local upvalue); a
    /// leaf contributes exactly `binding_regions[b]`, which is what the walk
    /// returns for it, so `out` is the part of the init's regions the caller
    /// replaces with the placeholder.
    ///
    /// A **branch** is descended arm by arm, each arm being one path
    /// (docs/impl/region/bindings.md § "A branch is a read of whichever arms
    /// read"). The reader's obligation is about the value it ends up holding, and
    /// one `IncrefValueRegion` at the binder names the runtime value — so it
    /// covers whichever arm ran, over one container or several. An arm that is
    /// *not* a read contributes nothing to `out` and so keeps its own regions in
    /// the reader's set, which is what an allocating arm needs: those regions are
    /// the only thing extending its value's last use out to the retain. A `Cond`
    /// clause list with no else has a path that produces no value at all, which
    /// is such an arm — it holds no reference for the retain or the placeholder
    /// release to name.
    ///
    /// `reader_name` excludes the reader's OWN source name, because a binding that
    /// shares a container's name is a **version** of that container rather than a
    /// second name for its content: functionalization's `fresh_version` keeps the
    /// name, so `(let [x (if c x x)] …)` is the SSA phi carrying `x`'s content
    /// forward past a conditional `assign`, and every later read of the name
    /// resolves to it. A version hands the one reference along exactly as a loop
    /// parameter's init edge does (docs/impl/region/bindings.md § "A loop
    /// parameter's init source is not a second holder"); counting it would claim a
    /// second reference for a single holding. So a version arm reads nothing here
    /// and keeps its regions, as any other non-reading arm does. A user rebinding
    /// that shadows the container reads as a version too.
    fn whole_container_read_regions(
        &self,
        h: &Hir,
        reader_name: crate::value::SymbolId,
        out: &mut Vec<Region>,
    ) {
        match &h.kind {
            HirKind::Var(b) => {
                let info = self.arena().get(*b);
                if info.is_one_slot_container() && info.name != reader_name {
                    if let Some(regions) = self.binding_regions.get(b) {
                        out.extend(regions.iter().copied());
                    }
                }
            }
            HirKind::DerefCell { cell } => {
                self.whole_container_read_regions(cell, reader_name, out)
            }
            // A statement wrapper selects a value exactly as a branch arm does:
            // the walk returns the LAST expression's regions and nothing else, so
            // the reader ends up holding what the tail read, and the tail is the
            // one path there is.
            HirKind::Begin(exprs) => {
                if let Some(tail) = exprs.last() {
                    self.whole_container_read_regions(tail, reader_name, out);
                }
            }
            HirKind::If {
                then_branch,
                else_branch,
                ..
            } => {
                self.whole_container_read_regions(then_branch, reader_name, out);
                self.whole_container_read_regions(else_branch, reader_name, out);
            }
            HirKind::Cond {
                clauses,
                else_branch,
            } => {
                for (_, body) in clauses {
                    self.whole_container_read_regions(body, reader_name, out);
                }
                if let Some(e) = else_branch {
                    self.whole_container_read_regions(e, reader_name, out);
                }
            }
            // An unmatched value signals rather than falling through to a value,
            // so a `Match`'s arms are every value-producing path it has. An arm's
            // pattern names are irrelevant — what is asked of the arm is what its
            // BODY produces.
            HirKind::Match { arms, .. } => {
                for (_, _, body) in arms {
                    self.whole_container_read_regions(body, reader_name, out);
                }
            }
            _ => {}
        }
    }
}
